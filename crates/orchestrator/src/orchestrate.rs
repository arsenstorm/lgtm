//! A bounded tool loop per event: when a task under a goal ends, a model gets
//! the LGTM tools over MCP and takes a few steps — inspect, create dependent
//! work, message, retry, approve, or leave the goal to a person. Every action
//! goes through the same HTTP endpoints a person uses, so LGTM validates it.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use lgtm_protocol::{Executor, Goal, Task, TaskEvent, TaskStatus};
use serde::Serialize;
use serde_json::{json, Value};

use crate::state::{App, State};

/// A loop that cannot finish in this long has lost the event it was asked
/// about; the next one will build a fresher context anyway.
const ASK_TIMEOUT: Duration = Duration::from_secs(300);
/// Enough turns to inspect, act two or three times, and write the summary.
const MAX_TURNS: &str = "12";
/// A utility call is one turn of one model; the runner caps its own at the
/// same 90s, and the caller waits 120s for either.
const ONE_SHOT_TIMEOUT: Duration = Duration::from_secs(90);

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

pub(crate) fn on_path(binary: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).is_file())
}

/// What one pass of the model produced: the words for a person, the ids the
/// screen turns into cards, and the tools it called on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answered {
    pub text: String,
    pub refs: Vec<String>,
    pub steps: Vec<Step>,
    pub worked_ms: u64,
}

/// One tool call the model made, named as the MCP server names it.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub tool: String,
    /// The string arguments joined, so "task_inspect ead67d7d" reads as a line.
    pub detail: String,
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
        Ok(answered) => record(
            &app,
            &task_id,
            "summary",
            answered.text,
            true,
            String::new(),
        ),
        Err(note) => {
            tracing::warn!(task = %task_id, %note, "orchestrator failed");
            record(&app, &task_id, "error", String::new(), false, note)
        }
    }
}

/// One question, one bounded pass over the read-only workspace tools, one
/// answer. No goal, no task: `lgtm mcp` sees `LGTM_ASK` and serves only
/// reads, so a question can never create or approve work.
pub async fn answer_question(app: Arc<App>, question: String) -> Result<Answered, String> {
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
         Answer from the lgtm tools (goals_list, tasks_list, sessions_list, todos_list, \
         activity, task_inspect, runner_list). Reply with one short sentence, under 25 words, \
         that sums up the answer for a person: no headings, no lists, no markdown, no ids, and \
         no describing items one by one. Then end with one line that starts with `refs:` \
         followed by every task id, todo id and runner name the answer is about, separated by \
         spaces; the person's screen turns each one into a card, and the cards carry the detail."
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
) -> Result<Answered, String> {
    let started = Instant::now();
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
    let (text, steps) =
        read(executor, &stdout).ok_or_else(|| format!("{binary} answered nothing"))?;
    let (text, refs) = split_refs(&text);
    Ok(Answered {
        text,
        refs,
        steps,
        worked_ms: started.elapsed().as_millis() as u64,
    })
}

/// One model call with no tools, run on this host: the inference lane's
/// fallback for an orchestrator with no runner connected. Same shape as
/// [`ask`], minus the MCP server — a utility call has nothing to inspect.
pub(crate) async fn one_shot(
    executor: Executor,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    let binary = executor.binary();
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(one_shot_args(executor, system, prompt))
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(ONE_SHOT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("{binary} did not answer in {}s", ONE_SHOT_TIMEOUT.as_secs()))?
        .map_err(|err| format!("{binary} did not run: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    read(executor, &stdout)
        .map(|(text, _)| text)
        .ok_or_else(|| format!("{binary} answered nothing"))
}

/// `codex exec` has no system-prompt flag, so the instructions lead the
/// prompt there. Both shapes are what [`read`] already reads.
fn one_shot_args(executor: Executor, system: &str, prompt: &str) -> Vec<String> {
    match executor {
        Executor::Claude => vec![
            "-p".into(),
            prompt.into(),
            "--append-system-prompt".into(),
            system.into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
        ],
        Executor::Codex => vec![
            "exec".into(),
            "--json".into(),
            "--sandbox".into(),
            "read-only".into(),
            format!("{system}\n\n{prompt}"),
        ],
    }
}

pub fn args(executor: Executor, prompt: &str, exe: &Path) -> Vec<String> {
    let owned = |args: &[&str]| args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    match executor {
        Executor::Claude => {
            let mut args = owned(&[
                "-p",
                prompt,
                "--output-format",
                "stream-json",
                "--verbose",
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

/// The answer and the tool calls, read line by line out of either executor's
/// stream: Claude's `assistant` and `result` lines, Codex's completed items.
fn read(executor: Executor, stdout: &str) -> Option<(String, Vec<Step>)> {
    let mut steps = Vec::new();
    let mut said = Vec::new();
    let lines = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok());
    for value in lines {
        match executor {
            Executor::Claude => {
                if value.get("type").and_then(Value::as_str) == Some("result") {
                    if let Some(result) = value.get("result").and_then(Value::as_str) {
                        said = vec![result.to_string()];
                    }
                }
                let blocks = value.pointer("/message/content").and_then(Value::as_array);
                for block in blocks.into_iter().flatten() {
                    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    // Only the workspace tools are the agent's work; the rest
                    // (ToolSearch, say) is the harness loading them.
                    let name = block.get("name").and_then(Value::as_str);
                    if let Some(name) = name.filter(|name| name.starts_with("mcp__")) {
                        steps.push(step(name, block.get("input")));
                    }
                }
            }
            Executor::Codex => {
                if let Some(text) = agent_message(&value) {
                    said.push(text.to_string());
                }
                if let Some(item) = completed_item(&value, "mcp_tool_call") {
                    if let Some(tool) = item.get("tool").and_then(Value::as_str) {
                        steps.push(step(tool, item.get("arguments")));
                    }
                }
            }
        }
    }
    let text = said.join("\n");
    (!text.is_empty()).then_some((text, steps))
}

/// `mcp__lgtm__task_inspect` and `task_inspect` are the same tool.
fn step(name: &str, input: Option<&Value>) -> Step {
    let detail = input
        .and_then(Value::as_object)
        .map(|object| {
            object
                .values()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    Step {
        tool: name.rsplit("__").next().unwrap_or(name).to_string(),
        detail,
    }
}

/// The `item` of an `item.completed` line, when it is of the given kind.
fn completed_item<'a>(value: &'a Value, kind: &str) -> Option<&'a Value> {
    if value.get("type")?.as_str()? != "item.completed" {
        return None;
    }
    let item = value.get("item")?;
    (item.get("type")?.as_str()? == kind).then_some(item)
}

/// The prose for a person, and the ids the model listed on its closing
/// `refs:` line so the screen can draw a card for each without the prose
/// having to carry them.
fn split_refs(text: &str) -> (String, Vec<String>) {
    let trimmed = text.trim();
    let (prose, last) = match trimmed.rsplit_once('\n') {
        Some((prose, last)) => (prose, last),
        None => ("", trimmed),
    };
    let last = last.trim();
    let Some(listed) = last
        .strip_prefix("refs:")
        .or_else(|| last.strip_prefix("Refs:"))
    else {
        return (trimmed.to_string(), Vec::new());
    };
    let refs = listed
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|r| r.trim_matches('`'))
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect();
    (prose.trim().to_string(), refs)
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
