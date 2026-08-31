//! A bounded tool loop per event: when a task under a goal ends, a model gets
//! the LGTM tools over MCP and takes a few steps — inspect, create dependent
//! work, message, retry, approve, or leave the goal to a person. Every action
//! goes through the same HTTP endpoints a person uses, so LGTM validates it.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use lgtm_protocol::{Executor, Goal, Task, TaskEvent, TaskStatus};
use serde_json::{json, Value};

use crate::state::{App, State};

/// A loop that cannot finish in this long has lost the event it was asked
/// about; the next one will build a fresher context anyway.
const ASK_TIMEOUT: Duration = Duration::from_secs(300);
/// Enough turns to inspect, act two or three times, and write the summary.
const MAX_TURNS: &str = "12";

/// What `--orchestrate` was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Whichever agent this machine has.
    Auto,
    One(Executor),
}

/// The executor the loop will run on, or `None` when there is none to run.
pub fn pick(choice: Choice) -> Option<Executor> {
    resolve(choice, on_path)
}

fn resolve(choice: Choice, found: impl Fn(&str) -> bool) -> Option<Executor> {
    if let Choice::One(executor) = choice {
        return Some(executor);
    }
    let picked = [Executor::Claude, Executor::Codex]
        .into_iter()
        .find(|executor| found(executor.binary()));
    if picked.is_none() {
        tracing::warn!("--orchestrate auto found neither claude nor codex; orchestration is off");
    }
    picked
}

fn on_path(binary: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).is_file())
}

/// The goal and the task whose end started this loop, read under the lock and
/// rendered to text outside it.
pub struct Context {
    pub goal: Goal,
    pub subject: Task,
}

/// `None` when the task has no goal, which is every task the loop must keep
/// its hands off.
pub fn build_context(state: &State, task_id: &str) -> Option<Context> {
    let subject = state.tasks.get(task_id)?.task.clone();
    let goal = state.goals.get(subject.spec.goal.as_deref()?)?.clone();
    Some(Context { goal, subject })
}

pub fn prompt(ctx: &Context) -> String {
    let task = &ctx.subject;
    let ending = match &task.error {
        Some(error) => format!(" with: {error}"),
        None => String::new(),
    };
    format!(
        "You are the shared engineering agent for this workspace, orchestrating work toward \
         one goal in a software repository.\n\n\
         Goal: {}\n\n\
         Task {} just ended as {}{ending}.\n\n\
         {INSTRUCTION}",
        ctx.goal.objective,
        task.id,
        status_word(task.status),
    )
}

const INSTRUCTION: &str = "Use the lgtm tools: inspect this goal, and check goals_list, \
     tasks_list, and activity for what the rest of the workspace is doing before creating work; \
     you may only act on tasks under this goal; call `wait` with a reason when a person is \
     needed; finish with one paragraph for the developer.";

/// The wire spelling, so the model reads the same words the API returns.
fn status_word(status: TaskStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The whole loop for one event: read the state, run the model with the LGTM
/// tools, record what it said.
pub async fn run(app: Arc<App>, task_id: String) {
    let Some(executor) = app.orchestrate else {
        return;
    };
    let Some(ctx) = ({
        let state = app.state.lock().unwrap();
        build_context(&state, &task_id)
    }) else {
        return;
    };
    let goal = ctx.goal.id.clone();
    app.orchestrating.lock().unwrap().insert(goal.clone());
    let answer = ask(executor, &prompt(&ctx), &env(&app, &ctx)).await;
    app.orchestrating.lock().unwrap().remove(&goal);
    match answer {
        Ok(text) => record(&app, &task_id, "summary", text, true, String::new()),
        Err(note) => {
            tracing::warn!(task = %task_id, %note, "orchestrator failed");
            record(&app, &task_id, "error", String::new(), false, note)
        }
    }
}

/// One question, one bounded pass over the read-only workspace tools, one
/// answer. No goal, no task: `lgtm mcp` sees `LGTM_ASK` and serves only
/// reads, so a question can never create or approve work.
pub async fn answer_question(app: Arc<App>, question: String) -> Result<String, String> {
    let Some(executor) = app.orchestrate else {
        return Err("--orchestrate is not configured".into());
    };
    let env = [
        ("LGTM_ORCHESTRATOR", app.base_url.clone()),
        ("LGTM_TOKEN", app.token.clone()),
        ("LGTM_ASK", "1".to_string()),
    ];
    ask(executor, &ask_prompt(&question), &env).await
}

fn ask_prompt(question: &str) -> String {
    format!(
        "You are the shared engineering agent for this workspace. A person asks:\n\n\
         {question}\n\n\
         Answer from the lgtm tools (goals_list, tasks_list, sessions_list, activity, \
         task_inspect, runner_list). Name people by name and tasks by id. A few short \
         paragraphs at most."
    )
}

fn record(app: &App, task_id: &str, action: &str, reason: String, applied: bool, note: String) {
    let mut state = app.state.lock().unwrap();
    let changed = state.apply_event(
        task_id,
        TaskEvent::Orchestrated {
            action: action.into(),
            reason,
            applied,
            note,
        },
    );
    app.persist_ids(&mut state, &changed);
}

/// What `lgtm mcp` reads to answer for this loop.
fn env(app: &App, ctx: &Context) -> [(&'static str, String); 5] {
    [
        ("LGTM_ORCHESTRATOR", app.base_url.clone()),
        ("LGTM_TOKEN", app.token.clone()),
        ("LGTM_GOAL_ID", ctx.goal.id.clone()),
        ("LGTM_TASK_ID", ctx.subject.id.clone()),
        ("LGTM_REPOSITORY", ctx.goal.repository.clone()),
    ]
}

/// Runs the model in a temporary directory: it works through the LGTM tools
/// and has no repository of its own to touch.
async fn ask(
    executor: Executor,
    prompt: &str,
    env: &[(&'static str, String)],
) -> Result<String, String> {
    let binary = executor.binary();
    let exe = std::env::current_exe().map_err(|err| format!("no path to lgtm mcp: {err}"))?;
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(args(executor, prompt, &exe))
        .envs(env.iter().map(|(name, value)| (*name, value)))
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(ASK_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("{binary} did not answer in {}s", ASK_TIMEOUT.as_secs()))?
        .map_err(|err| format!("{binary} did not run: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    answer(executor, &stdout).ok_or_else(|| format!("{binary} answered nothing"))
}

pub fn args(executor: Executor, prompt: &str, exe: &Path) -> Vec<String> {
    let owned = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    match executor {
        Executor::Claude => {
            let mut args = owned(&[
                "-p",
                prompt,
                "--output-format",
                "json",
                "--max-turns",
                MAX_TURNS,
                "--allowedTools",
                "mcp__lgtm__*",
                "--permission-mode",
                "default",
                "--mcp-config",
            ]);
            args.push(mcp_config(exe));
            args
        }
        Executor::Codex => owned(&[
            "exec",
            "--json",
            "--sandbox",
            "read-only",
            "-c",
            &format!("mcp_servers.lgtm.command=\"{}\"", exe.display()),
            "-c",
            "mcp_servers.lgtm.args=[\"mcp\"]",
            prompt,
        ]),
    }
}

/// The MCP server is this same binary: the orchestrator and `lgtm` are one
/// command.
fn mcp_config(exe: &Path) -> String {
    json!({ "mcpServers": { "lgtm": { "command": exe.display().to_string(), "args": ["mcp"] } } })
        .to_string()
}

fn answer(executor: Executor, stdout: &str) -> Option<String> {
    if executor == Executor::Claude {
        let value: Value = serde_json::from_str(stdout.trim()).ok()?;
        return Some(value.get("result")?.as_str()?.to_string());
    }
    let text = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| agent_message(&value).map(str::to_string))
        .collect::<Vec<String>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

/// codex has moved its messages about between versions: the event is either
/// the line itself or nested under `item` or `msg`, and its text is under
/// `text` or `message`.
fn agent_message(value: &Value) -> Option<&str> {
    let event = ["item", "msg"]
        .iter()
        .find_map(|key| value.get(key))
        .unwrap_or(value);
    if event.get("type").and_then(Value::as_str) != Some("agent_message") {
        return None;
    }
    event.get("text").or_else(|| event.get("message"))?.as_str()
}

#[cfg(test)]
#[path = "orchestrate_tests.rs"]
mod tests;
