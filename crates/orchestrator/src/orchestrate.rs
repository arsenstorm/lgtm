//! One decision per event: when a task under a goal ends, a model reads the
//! goal's state and names the next step, and LGTM checks it before acting.
//! There is no conversation — every decision starts from the stored state.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use lgtm_protocol::{
    knowledge_block, Executor, Goal, Memory, OrchestratorMessage, Review, StoredEvent, Task,
    TaskEvent, TaskId, TaskKind, TaskSpec, TaskStatus,
};
use serde::Deserialize;
use serde_json::Value;

use crate::commands::RetryInto;
use crate::state::{App, CmdError, State};

/// A model that cannot answer in this long has lost the event it was asked
/// about; the next one will build a fresher context anyway.
const ASK_TIMEOUT: Duration = Duration::from_secs(300);
/// How much of the subject's chatter the prompt carries.
const RECENT_EVENTS: usize = 30;

/// What the model is allowed to ask for; LGTM checks each one before acting.
#[derive(Deserialize, Debug, PartialEq)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    Approve {
        reason: String,
    },
    Retry {
        reason: String,
    },
    Message {
        text: String,
        reason: String,
    },
    CreateTask {
        title: String,
        prompt: String,
        #[serde(default)]
        depends_on: Vec<TaskId>,
        reason: String,
    },
    Wait {
        reason: String,
    },
}

/// The state the model sees, built under the lock and rendered to text
/// outside it.
pub struct Context {
    pub goal: Goal,
    pub tasks: Vec<Task>,
    pub subject: Task,
    pub subject_events: Vec<StoredEvent>,
    pub memories: Vec<Memory>,
}

/// `None` when the task has no goal, which is every task the orchestration
/// loop must keep its hands off.
pub fn build_context(state: &State, task_id: &str) -> Option<Context> {
    let rec = state.tasks.get(task_id)?;
    let subject = rec.task.clone();
    let goal = state.goals.get(subject.spec.goal.as_deref()?)?.clone();
    Some(Context {
        tasks: state.goal_tasks(&goal.id).into_iter().cloned().collect(),
        subject,
        subject_events: rec.events.clone(),
        memories: state.memories_for(&goal.repository),
        goal,
    })
}

pub fn prompt(ctx: &Context) -> String {
    let tasks: String = ctx.tasks.iter().map(task_line).collect();
    format!(
        "{}You are orchestrating work toward one goal in a software repository.\n\n\
         Goal: {}\n\nTasks under this goal:\n{tasks}\n{}\n{INSTRUCTION}",
        knowledge_block(&ctx.memories),
        ctx.goal.objective,
        subject_block(ctx),
    )
}

fn task_line(task: &Task) -> String {
    let first = task.spec.prompt.lines().next().unwrap_or_default();
    let deps = match task.spec.depends_on.is_empty() {
        true => String::new(),
        false => format!(" (depends on {})", task.spec.depends_on.join(", ")),
    };
    format!(
        "- {} [{}] {first}{deps}\n",
        task.id,
        status_word(task.status)
    )
}

/// Everything about the task whose end triggered this decision.
fn subject_block(ctx: &Context) -> String {
    let task = &ctx.subject;
    let mut out = format!(
        "\nThe task that just ended is {} [{}]:\n{}\n",
        task.id,
        status_word(task.status),
        task.spec.prompt,
    );
    if let Some(error) = &task.error {
        out.push_str(&format!("Error: {error}\n"));
    }
    out.push_str(&result_block(task));
    out.push_str(&format!(
        "Attempts: {}, cost so far ${:.2}\n",
        task.executions.len(),
        task.executions.last().map_or(0.0, |exec| exec.cost_usd),
    ));
    out.push_str(&recent(&ctx.subject_events));
    if !task.scratchpad.is_empty() {
        out.push_str(&format!("Its notes:\n{}\n", task.scratchpad));
    }
    out
}

fn result_block(task: &Task) -> String {
    let Some(result) = &task.result else {
        return String::new();
    };
    let checks: String = result
        .validation
        .iter()
        .map(|check| {
            let word = if check.ok { "passed" } else { "failed" };
            format!("- check {} {word}\n", check.name)
        })
        .collect();
    let findings: String = result
        .review
        .iter()
        .flat_map(|review| &review.findings)
        .map(|finding| format!("- {:?} {}\n", finding.severity, finding.message))
        .collect();
    format!("Checks:\n{checks}Review findings:\n{findings}")
}

/// The last few things the agent said or ran, oldest first.
fn recent(events: &[StoredEvent]) -> String {
    let mut lines: Vec<String> = events
        .iter()
        .rev()
        .filter_map(|stored| match &stored.event {
            TaskEvent::Progress { text } => Some(text.clone()),
            TaskEvent::Command { command } => Some(format!("$ {command}")),
            _ => None,
        })
        .take(RECENT_EVENTS)
        .collect();
    lines.reverse();
    match lines.is_empty() {
        true => String::new(),
        false => format!("Recent activity:\n{}\n", lines.join("\n")),
    }
}

const INSTRUCTION: &str = r#"
Decide ONE next step for the goal. Answer with a single ```json fenced block and nothing after it, in exactly one of these shapes:
```json
{"action": "approve", "reason": "why"}
{"action": "retry", "reason": "why"}
{"action": "message", "text": "what to tell the agent", "reason": "why"}
{"action": "create_task", "title": "One line", "prompt": "Full instructions for a coding agent", "depends_on": ["task-id"], "reason": "why"}
{"action": "wait", "reason": "why"}
```
Prefer "wait" when a person should look at this. Use "approve" only when the checks passed and no blocking review finding is left. Use "retry" for a crash or a timeout. Use "message" for a review finding the agent can fix itself. Use "create_task" only for work the goal needs that no task above covers."#;

/// The wire spelling, so the model reads the same words the API returns.
fn status_word(status: TaskStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn parse_decision(text: &str) -> Result<Decision, String> {
    let json = last_json_block(text).unwrap_or_else(|| text.trim());
    serde_json::from_str(json).map_err(|err| format!("answer was not a decision: {err}"))
}

/// The last ```json block, or the last fence holding something object-shaped.
fn last_json_block(text: &str) -> Option<&str> {
    let mut found = None;
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let tagged = after.starts_with("json");
        let body = after.strip_prefix("json").unwrap_or(after);
        let Some(end) = body.find("```") else { break };
        let block = &body[..end];
        if tagged || block.trim_start().starts_with('{') {
            found = Some(block);
        }
        rest = &body[end + 3..];
    }
    found
}

/// Performs `decision` if the state still allows it. `Err` is the refusal a
/// reader sees on the event log; it is never a reason to stop the loop.
pub fn apply(
    state: &mut State,
    task_id: &str,
    decision: &Decision,
    github: Option<&lgtm_github::GitHub>,
) -> Result<Vec<TaskId>, String> {
    match decision {
        Decision::Wait { .. } => Ok(Vec::new()),
        Decision::Approve { .. } => approve(state, task_id, github),
        Decision::Retry { .. } => state
            .retry(
                task_id,
                RetryInto {
                    runner: None,
                    executor: None,
                },
            )
            .map(|(_, changed)| changed)
            .map_err(refusal),
        Decision::Message { text, .. } => state
            .message(task_id, text.clone())
            .map(|(_, changed)| changed)
            .map_err(refusal),
        Decision::CreateTask {
            title,
            prompt,
            depends_on,
            ..
        } => create_task(state, task_id, format!("{title}\n\n{prompt}"), depends_on),
    }
}

/// The same cleanliness policy demands of itself, checked here because the
/// model can ask for an approval the diff has not earned.
fn approve(
    state: &mut State,
    task_id: &str,
    github: Option<&lgtm_github::GitHub>,
) -> Result<Vec<TaskId>, String> {
    let task = state.tasks.get(task_id).ok_or("unknown task")?.task.clone();
    if task.status != TaskStatus::AwaitingReview {
        return Err(format!("task is {}", status_word(task.status)));
    }
    let result = task.result.as_ref().ok_or("task has no result")?;
    if result.validation_failed() {
        return Err("checks failed".into());
    }
    if result.review.as_ref().is_some_and(Review::has_blocking) {
        return Err("blocking review findings".into());
    }
    let token = crate::state::push_token(github, &task);
    state
        .command(
            task_id,
            &[TaskStatus::AwaitingReview],
            "task is not awaiting review",
            |task_id| OrchestratorMessage::Push { task_id, token },
        )
        .map(|_| Vec::new())
        .map_err(refusal)
}

fn create_task(
    state: &mut State,
    task_id: &str,
    prompt: String,
    depends_on: &[TaskId],
) -> Result<Vec<TaskId>, String> {
    let subject = state.tasks.get(task_id).ok_or("unknown task")?.task.clone();
    let goal = subject.spec.goal.clone().ok_or("task has no goal")?;
    let foreign = depends_on.iter().find(|id| {
        !state
            .tasks
            .get(*id)
            .is_some_and(|rec| rec.task.spec.goal.as_ref() == Some(&goal))
    });
    if let Some(id) = foreign {
        return Err(format!("{id} is not a task under this goal"));
    }
    let spec = TaskSpec {
        repository: subject.spec.repository,
        base_branch: subject.spec.base_branch,
        prompt,
        executor: subject.spec.executor,
        runner: None,
        issue: None,
        linear: None,
        kind: TaskKind::Run,
        parent: None,
        depends_on: depends_on.to_vec(),
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: subject.spec.sandbox,
        requirements: subject.spec.requirements,
        review_executor: None,
        model: None,
        goal: Some(goal),
        allowed_hosts: Vec::new(),
    };
    state.create_task(spec).map(|(_, changed)| changed)
}

fn refusal(err: CmdError) -> String {
    match err {
        CmdError::NotFound => "task not found".into(),
        CmdError::Conflict(msg) => msg,
    }
}

/// The whole loop for one event: read the state, ask, check, act, record.
pub async fn run(app: Arc<App>, task_id: String) {
    let Some(executor) = app.orchestrate else {
        return;
    };
    let Some(text) = ({
        let state = app.state.lock().unwrap();
        build_context(&state, &task_id).map(|ctx| prompt(&ctx))
    }) else {
        return;
    };
    let answer = match ask(executor, &text).await {
        Ok(answer) => answer,
        Err(note) => return failed(&app, &task_id, note),
    };
    match parse_decision(&answer) {
        Ok(decision) => act(&app, &task_id, &decision),
        Err(note) => failed(&app, &task_id, note),
    }
}

fn act(app: &App, task_id: &str, decision: &Decision) {
    let (action, reason) = described(decision);
    let mut state = app.state.lock().unwrap();
    let (applied, note, mut changed) =
        match apply(&mut state, task_id, decision, app.github.as_ref()) {
            Ok(changed) => (true, String::new(), changed),
            Err(note) => (false, note, Vec::new()),
        };
    tracing::info!(task = %task_id, %action, applied, %note, "orchestrator decided");
    changed.extend(state.apply_event(
        task_id,
        TaskEvent::Orchestrated {
            action: action.into(),
            reason,
            applied,
            note,
        },
    ));
    app.persist_ids(&mut state, &changed);
}

/// A spawn, timeout or parse failure is a decision that did not happen, and
/// is recorded so it is never silent.
fn failed(app: &App, task_id: &str, note: String) {
    tracing::warn!(task = %task_id, %note, "orchestrator failed");
    let mut state = app.state.lock().unwrap();
    let changed = state.apply_event(
        task_id,
        TaskEvent::Orchestrated {
            action: "error".into(),
            reason: String::new(),
            applied: false,
            note,
        },
    );
    app.persist_ids(&mut state, &changed);
}

fn described(decision: &Decision) -> (&'static str, String) {
    match decision {
        Decision::Approve { reason } => ("approve", reason.clone()),
        Decision::Retry { reason } => ("retry", reason.clone()),
        Decision::Message { reason, .. } => ("message", reason.clone()),
        Decision::CreateTask { reason, .. } => ("create_task", reason.clone()),
        Decision::Wait { reason } => ("wait", reason.clone()),
    }
}

/// Runs the model read-only, in a temporary directory: it decides from the
/// text it is given and has no repository of its own to touch.
async fn ask(executor: Executor, prompt: &str) -> Result<String, String> {
    let binary = executor.binary();
    let mut cmd = tokio::process::Command::new(binary);
    match executor {
        Executor::Claude => cmd.args([
            "-p",
            prompt,
            "--output-format",
            "json",
            "--permission-mode",
            "default",
        ]),
        Executor::Codex => cmd.args(["exec", "--sandbox", "read-only", "--json", prompt]),
    };
    cmd.current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(ASK_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("{binary} did not answer in {}s", ASK_TIMEOUT.as_secs()))?
        .map_err(|err| format!("{binary} did not run: {err}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    answer(executor, &stdout).ok_or_else(|| format!("{binary} answered nothing"))
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
