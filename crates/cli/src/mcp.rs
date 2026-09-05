//! `lgtm mcp`: a stdio MCP server that hands one agent run the task's
//! context — the repository's memories, todos and scratchpads, and the task's
//! own notes.
//! The runner registers it with claude and codex for every run.
//!
//! With `LGTM_GOAL_ID` set it is the orchestration loop's server instead, and
//! carries the tools that inspect and act on a whole goal. Every one of those
//! goes through the endpoints a person uses, so LGTM validates them the same
//! way, and every call lands on the ended task's event log. The loop may read
//! the whole workspace but may only act on the goal that woke it.
//!
//! With `LGTM_ASK` set it answers `lgtm ask`, which reads and never writes.

use std::collections::HashMap;

use anyhow::Result;
use lgtm_client::{Client, Orchestrated, Retry, ScratchpadPatch};
use lgtm_protocol::{Task, TaskKind, TaskSpec, TodoStatus};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL: &str = "2024-11-05";
/// How much of a task's chatter `task_inspect` carries.
const RECENT_EVENTS: usize = 20;
/// How much of the workspace `tasks_list` shows before a model drowns in it.
const TASK_LINES: usize = 50;
/// The tools a task's own run gets, in the order they are listed.
const RUN_TOOLS: [&str; 12] = [
    "memories_list",
    "memory_propose",
    "todos_list",
    "todo_create",
    "scratchpad_read",
    "scratchpad_write",
    "scratchpads_list",
    "scratchpad_open",
    "scratchpad_create",
    "scratchpad_update",
    "scratchpad_archive",
    "request_network",
];
/// The goal tools `lgtm ask` keeps: both only read.
const ASK_GOAL_TOOLS: [&str; 2] = ["task_inspect", "runner_list"];
/// The scratchpad tools `lgtm ask` keeps: both only read.
const ASK_SCRATCHPAD_TOOLS: [&str; 2] = ["scratchpads_list", "scratchpad_open"];

/// Which run this server answers for. The harness spawns it with no
/// arguments of its own, so the runner passes this in the environment.
pub enum Env {
    /// One agent run inside a task.
    Run { task_id: String, repository: String },
    /// The orchestration loop for one goal: run tools plus goal and
    /// workspace tools.
    Orchestrate {
        task_id: String,
        repository: String,
        goal_id: String,
    },
    /// `lgtm ask`: workspace reads only, so a question can never create,
    /// message, or approve work.
    Ask,
}

pub async fn serve(client: &Client) -> Result<i32> {
    let env = require_env();
    // An orchestration pass names its goal on every call, so the orchestrator
    // — not just `under_goal` below — holds it to that goal's tasks.
    let client = &match &env {
        Env::Orchestrate { goal_id, .. } => client.clone().scoped_to_goal(goal_id.clone()),
        _ => client.clone(),
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(reply) = handle(&request, client, &env).await else {
            continue;
        };
        stdout.write_all(format!("{reply}\n").as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(0)
}

fn require_env() -> Env {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    match (var("LGTM_TASK_ID"), var("LGTM_REPOSITORY")) {
        // The task vars outrank LGTM_ASK: a stray exported LGTM_ASK must
        // never strip a real run or loop pass down to the ask reads.
        (Some(task_id), Some(repository)) => match var("LGTM_GOAL_ID") {
            Some(goal_id) => Env::Orchestrate {
                task_id,
                repository,
                goal_id,
            },
            None => Env::Run {
                task_id,
                repository,
            },
        },
        _ if var("LGTM_ASK").is_some() => Env::Ask,
        _ => {
            eprintln!("lgtm mcp needs LGTM_TASK_ID and LGTM_REPOSITORY; the runner sets them");
            std::process::exit(2);
        }
    }
}

/// One JSON-RPC request in, one response out — or `None` for a notification,
/// which must not be answered.
async fn handle(request: &Value, client: &Client, env: &Env) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    Some(match reply(method, &params, client, env).await {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

async fn reply(method: &str, params: &Value, client: &Client, env: &Env) -> Result<Value, Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "lgtm", "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tools(env) })),
        "ping" => Ok(json!({})),
        "tools/call" => Ok(match called(params, client, env).await {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(err) => json!({
                "content": [{ "type": "text", "text": format!("{err:#}") }],
                "isError": true,
            }),
        }),
        _ => Err(json!({ "code": -32601, "message": format!("no such method: {method}") })),
    }
}

/// The call, plus the record of it a person can read on the ended task. The
/// record is best-effort: a step that worked is not undone because logging it
/// failed.
async fn called(params: &Value, client: &Client, env: &Env) -> Result<String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let done = call(name, &args, client, env).await;
    if let Env::Orchestrate { task_id, .. } = env {
        let outcome = match &done {
            Ok(text) => text.clone(),
            Err(err) => format!("{err:#}"),
        };
        let _ = client
            .orchestrated(
                task_id,
                &Orchestrated {
                    action: name,
                    reason: text(&args, "reason"),
                    applied: done.is_ok(),
                    note: first_line(&outcome),
                },
            )
            .await;
    }
    done
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

async fn call(name: &str, args: &Value, client: &Client, env: &Env) -> Result<String> {
    match env {
        Env::Ask => ask_call(name, args, client).await,
        Env::Run {
            task_id,
            repository,
        } => run_call(name, args, client, task_id, repository).await,
        Env::Orchestrate {
            task_id,
            repository,
            goal_id,
        } => match RUN_TOOLS.contains(&name) {
            true => run_call(name, args, client, task_id, repository).await,
            false => orchestration_call(name, args, client, goal_id).await,
        },
    }
}

/// `lgtm ask`: the workspace reads, and the two goal tools that only read.
/// `todos_list` here spans the workspace, unlike the run tool of the same
/// name, because "what is everyone working on" includes work nobody started.
async fn ask_call(name: &str, args: &Value, client: &Client) -> Result<String> {
    match name {
        "task_inspect" => task_inspect(client, string(args, "task_id")?).await,
        "runner_list" => runner_list(client).await,
        "todos_list" => workspace_todos(client).await,
        "scratchpads_list" => scratchpads_list(client, None).await,
        "scratchpad_open" => scratchpad_open(client, args).await,
        _ => workspace_call(name, args, client).await,
    }
}

async fn workspace_todos(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut todos = client.todos(None).await?;
    todos.retain(|todo| todo.status == TodoStatus::Open);
    todos.sort_by_key(|todo| std::cmp::Reverse(todo.created_at));
    Ok(joined(todos.iter().map(|todo| {
        format!(
            "{} {} {} {}",
            todo.id,
            owner(&owners, todo.created_by.as_deref()),
            todo.repository.as_deref().map(repo_short).unwrap_or("-"),
            todo.title,
        )
    })))
}

async fn run_call(
    name: &str,
    args: &Value,
    client: &Client,
    task_id: &str,
    repository: &str,
) -> Result<String> {
    let repository = Some(repository);
    match name {
        "memories_list" => Ok(joined(
            client
                .memories(repository, false)
                .await?
                .iter()
                .map(|m| format!("- {}", m.content)),
        )),
        "memory_propose" => propose(client, repository, task_id, args).await,
        "todos_list" => Ok(joined(
            client
                .todos(repository)
                .await?
                .iter()
                .filter(|todo| todo.status == TodoStatus::Open)
                .map(|todo| format!("{}  {}", todo.id, todo.title)),
        )),
        "todo_create" => {
            let todo = client
                .create_todo(
                    repository,
                    string(args, "title")?,
                    text(args, "description"),
                    lgtm_protocol::Priority::default(),
                    None,
                    &[],
                )
                .await?;
            Ok(todo.id)
        }
        "scratchpad_read" => Ok(client.task(task_id).await?.task.scratchpad),
        "scratchpad_write" => {
            client
                .set_scratchpad(task_id, string(args, "content")?)
                .await?;
            Ok("notes saved".to_string())
        }
        "scratchpads_list" => scratchpads_list(client, repository).await,
        "scratchpad_open" => scratchpad_open(client, args).await,
        "scratchpad_create" => {
            let pad = client
                .create_scratchpad(repository, string(args, "title")?, text(args, "content"))
                .await?;
            Ok(scratchpad_link(&pad.id))
        }
        "scratchpad_update" => {
            let patch = ScratchpadPatch {
                title: args
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                content: args
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                archived: None,
            };
            client
                .update_scratchpad(scratchpad_id(string(args, "link")?), &patch)
                .await?;
            Ok("scratchpad saved".to_string())
        }
        "scratchpad_archive" => {
            let patch = ScratchpadPatch {
                archived: Some(true),
                ..ScratchpadPatch::default()
            };
            client
                .update_scratchpad(scratchpad_id(string(args, "link")?), &patch)
                .await?;
            Ok("scratchpad archived".to_string())
        }
        "request_network" => request_network(client, task_id, args).await,
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

/// Only reachable with `LGTM_GOAL_ID`, so a plain agent run cannot drive the
/// goal it is one task of.
async fn orchestration_call(
    name: &str,
    args: &Value,
    client: &Client,
    goal: &str,
) -> Result<String> {
    match name {
        "goal_inspect" => goal_inspect(client, goal).await,
        "task_inspect" => task_inspect(client, string(args, "task_id")?).await,
        "task_create" => task_create(client, goal, args).await,
        "task_message" => {
            let task = string(args, "task_id")?;
            under_goal(client, task, goal).await?;
            client.tell(task, string(args, "text")?).await?;
            Ok("sent".to_string())
        }
        "task_retry" => {
            let into = Retry {
                runner: args.get("runner").and_then(Value::as_str).map(String::from),
                executor: serde_json::from_value(args.get("executor").cloned().unwrap_or_default())
                    .ok(),
            };
            let task = string(args, "task_id")?;
            under_goal(client, task, goal).await?;
            client.retry(task, &into).await?;
            Ok("requeued".to_string())
        }
        "task_approve" => {
            let task = string(args, "task_id")?;
            under_goal(client, task, goal).await?;
            client.approve_as_orchestrator(task).await?;
            Ok("approved".to_string())
        }
        "runner_list" => runner_list(client).await,
        "wait" => {
            client
                .set_attention(goal, Some(string(args, "reason")?))
                .await?;
            Ok("recorded".to_string())
        }
        _ => workspace_call(name, args, client).await,
    }
}

/// The pass was woken by one goal and may only act on that goal's tasks;
/// reading another goal's task is what the workspace tools are for. The
/// orchestrator enforces the same rule from the goal header; this check is
/// here to fail the model faster, and with a sentence it can act on.
async fn under_goal(client: &Client, task_id: &str, goal: &str) -> Result<()> {
    let detail = client.task(task_id).await?;
    if detail.task.spec.goal.as_deref() != Some(goal) {
        anyhow::bail!(
            "task {task_id} is under another goal; this pass may only act on tasks under goal {goal}"
        );
    }
    Ok(())
}

async fn runner_list(client: &Client) -> Result<String> {
    Ok(joined(client.runners().await?.iter().map(|runner| {
        let executors: Vec<&str> = runner.info.executors.iter().map(|e| e.binary()).collect();
        format!(
            "{} {} running {}/{}",
            runner.info.name,
            executors.join(","),
            runner.running.len(),
            runner.info.slots,
        )
    })))
}

/// Reads across the whole workspace: the loop is told what else is happening
/// before it decides, and `lgtm ask` gets nothing but these.
async fn workspace_call(name: &str, args: &Value, client: &Client) -> Result<String> {
    match name {
        "goals_list" => goals_list(client).await,
        "sessions_list" => sessions_list(client).await,
        "tasks_list" => tasks_list(client).await,
        "activity" => activity(client, args).await,
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

/// `created_by` is an id; a person reads names, so every workspace line
/// resolves it once per call.
async fn owners(client: &Client) -> Result<HashMap<String, String>> {
    Ok(client
        .users()
        .await?
        .into_iter()
        .map(|user| (user.id, user.name))
        .collect())
}

fn owner<'a>(owners: &'a HashMap<String, String>, id: Option<&'a str>) -> &'a str {
    match id {
        Some(id) => owners.get(id).map(String::as_str).unwrap_or(id),
        None => "-",
    }
}

/// The clone URL is too long for a line that already carries a prompt.
fn repo_short(url: &str) -> &str {
    let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last)
}

/// Compact enough to prefix every activity line: minutes, then hours, then
/// days.
fn age(now: u64, at: u64) -> String {
    let minutes = now.saturating_sub(at) / 60_000;
    match minutes {
        0..=59 => format!("{minutes}m"),
        60..=2879 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

async fn goals_list(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut goals = client.goals().await?;
    goals.sort_by_key(|summary| std::cmp::Reverse(summary.goal.created_at));
    Ok(joined(goals.iter().map(|summary| {
        let goal = &summary.goal;
        let line = format!(
            "{} {} {} {} \"{}\"",
            goal.id,
            status_word(summary.status),
            owner(&owners, goal.created_by.as_deref()),
            summary.tasks.total(),
            first_line(&goal.objective),
        );
        match &goal.attention {
            Some(why) => format!("{line}\n  needs a person: {why}"),
            None => line,
        }
    })))
}

async fn sessions_list(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut sessions = client.sessions(None).await?;
    sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
    Ok(joined(sessions.iter().map(|session| {
        let title = match session.title.is_empty() {
            true => "-",
            false => &session.title,
        };
        format!(
            "{} {} {} \"{title}\"",
            session.id,
            owner(&owners, session.created_by.as_deref()),
            repo_short(&session.repository),
        )
    })))
}

async fn tasks_list(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut tasks = client.tasks().await?;
    tasks.sort_by_key(|task| std::cmp::Reverse(task.created_at));
    let all: Vec<&Task> = tasks.iter().collect();
    Ok(joined(tasks.iter().take(TASK_LINES).map(|task| {
        let mut line = format!(
            "{} {} {} {} \"{}\"",
            task.id,
            status_word(task.status),
            owner(&owners, task.created_by.as_deref()),
            repo_short(&task.spec.repository),
            first_line(&task.spec.prompt),
        );
        if !task.status.is_terminal() {
            for overlap in lgtm_protocol::overlaps(task, &all) {
                line.push_str(&format!(
                    " [overlaps {}: {} files]",
                    overlap.task,
                    overlap.files.len()
                ));
            }
        }
        line
    })))
}

async fn activity(client: &Client, args: &Value) -> Result<String> {
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(30) as u32;
    let now = crate::now_ms();
    Ok(joined(client.activity(limit).await?.iter().map(|line| {
        let detail = match line.detail.is_empty() {
            true => String::new(),
            false => format!(": {}", line.detail),
        };
        format!(
            "{} {} {} {} {}{detail}",
            age(now, line.at),
            line.task,
            line.owner.as_deref().unwrap_or("-"),
            repo_short(&line.repository),
            line.event,
        )
    })))
}

async fn goal_inspect(client: &Client, goal: &str) -> Result<String> {
    let detail = client.goal(goal).await?;
    let status = serde_json::to_value(detail.summary.status)?;
    let head = format!(
        "{}\nstatus: {}\n",
        detail.summary.goal.objective,
        status.as_str().unwrap_or_default()
    );
    Ok(head + &joined(detail.tasks.iter().map(task_line)))
}

fn task_line(task: &Task) -> String {
    let error = match &task.error {
        Some(error) => format!(" [error: {}]", first_line(error)),
        None => String::new(),
    };
    format!(
        "{} {} {}@{} \"{}\"{error}",
        task.id,
        status_word(task.status),
        task.spec.executor.binary(),
        task.runner.as_deref().unwrap_or("-"),
        first_line(&task.spec.prompt),
    )
}

/// The wire spelling, so the model reads the same words the API returns.
fn status_word(status: impl serde::Serialize) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

async fn task_inspect(client: &Client, id: &str) -> Result<String> {
    let detail = client.task(id).await?;
    let task = &detail.task;
    let mut out = format!(
        "{} {}\n{}\n",
        task.id,
        status_word(task.status),
        task.spec.prompt
    );
    for exec in &task.executions {
        out.push_str(&format!(
            "attempt {} {} {}\n",
            exec.attempt,
            status_word(exec.status),
            exec.error.as_deref().unwrap_or("")
        ));
    }
    out.push_str(&result_block(task));
    out.push_str(&recent(&detail.events));
    if !task.scratchpad.is_empty() {
        out.push_str(&format!("notes:\n{}\n", task.scratchpad));
    }
    Ok(out)
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
            format!("check {} {word}\n", check.name)
        })
        .collect();
    let findings: String = result
        .review
        .iter()
        .flat_map(|review| &review.findings)
        .filter(|finding| finding.severity == lgtm_protocol::Severity::Blocking)
        .map(|finding| format!("blocking: {}\n", finding.message))
        .collect();
    format!(
        "{checks}{findings}changed files: {}\n",
        result.changed_files.join(", ")
    )
}

/// The last few things the agent said or ran, oldest first.
fn recent(events: &[lgtm_protocol::StoredEvent]) -> String {
    let mut lines: Vec<String> = events
        .iter()
        .rev()
        .filter_map(|stored| match &stored.event {
            lgtm_protocol::TaskEvent::Progress { text } => Some(text.clone()),
            lgtm_protocol::TaskEvent::Command { command } => Some(format!("$ {command}")),
            _ => None,
        })
        .take(RECENT_EVENTS)
        .collect();
    lines.reverse();
    match lines.is_empty() {
        true => String::new(),
        false => format!("recent:\n{}\n", lines.join("\n")),
    }
}

/// Repository and base branch come from the goal, never from the model: a
/// task it invents must land where the goal's work already is.
async fn task_create(client: &Client, goal: &str, args: &Value) -> Result<String> {
    let detail = client.goal(goal).await?;
    let first = detail
        .tasks
        .first()
        .ok_or_else(|| anyhow::anyhow!("the goal has no task to copy a base branch from"))?;
    let spec = TaskSpec {
        repository: detail.summary.goal.repository.clone(),
        base_branch: first.spec.base_branch.clone(),
        prompt: format!("{}\n\n{}", string(args, "title")?, string(args, "prompt")?),
        executor: first.spec.executor,
        runner: None,
        issue: None,
        linear: None,
        kind: TaskKind::Run,
        parent: None,
        depends_on: serde_json::from_value(args.get("depends_on").cloned().unwrap_or(json!([])))?,
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: first.spec.sandbox,
        requirements: first.spec.requirements.clone(),
        review_executor: None,
        model: None,
        reasoning_effort: None,
        goal: Some(goal.to_string()),
        allowed_hosts: Vec::new(),
        session: None,
        created_by: None,
    };
    Ok(client.create_task(&spec).await?.id)
}

/// An agent cannot write what every later run is told: the memory it
/// proposes waits unapproved until a person runs `lgtm memory approve`.
async fn propose(
    client: &Client,
    repository: Option<&str>,
    task_id: &str,
    args: &Value,
) -> Result<String> {
    let memory = client
        .propose_memory(repository, string(args, "content")?, task_id)
        .await?;
    Ok(proposed_reply(&memory.id))
}

fn proposed_reply(id: &str) -> String {
    format!("proposed {id}; a person approves it with: lgtm memory approve {id}")
}

/// A run can't be paused mid-flight to ask a person, so the request is only
/// recorded; `lgtm allow` answers it before the task's next run.
async fn request_network(client: &Client, task_id: &str, args: &Value) -> Result<String> {
    let host = string(args, "host")?;
    client
        .request_permission(task_id, "network", host, string(args, "reason")?)
        .await?;
    Ok(format!(
        "recorded; a person can allow it with: lgtm allow {task_id} {host}"
    ))
}

/// The web app copies `lgtm://scratchpads/<encoded-repository>/<id>` or
/// `lgtm://scratchpads/<id>`; ids are unique on their own, so the last segment
/// is all that is resolved and a bare id works too.
fn scratchpad_id(link: &str) -> &str {
    link.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(link)
}

fn scratchpad_link(id: &str) -> String {
    format!("lgtm://scratchpads/{id}")
}

async fn scratchpads_list(client: &Client, repository: Option<&str>) -> Result<String> {
    Ok(joined(
        client
            .scratchpads(repository)
            .await?
            .iter()
            .filter(|pad| !pad.archived)
            .map(|pad| format!("{}  {}", scratchpad_link(&pad.id), pad.title)),
    ))
}

/// The same text the web app's "Copy as Markdown" produces, so a pasted
/// document and an opened link read alike.
async fn scratchpad_open(client: &Client, args: &Value) -> Result<String> {
    let pad = client
        .scratchpad(scratchpad_id(string(args, "link")?))
        .await?;
    Ok(format!("# {}\n\n{}", pad.title, pad.content))
}

fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing argument: {key}"))
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}

fn joined(lines: impl Iterator<Item = String>) -> String {
    lines.collect::<Vec<_>>().join("\n")
}

fn tools(env: &Env) -> Value {
    Value::Array(match env {
        Env::Run { .. } => run_tools(),
        Env::Orchestrate { .. } => {
            let mut tools = run_tools();
            tools.extend(goal_tools());
            tools.extend(workspace_tools());
            tools
        }
        Env::Ask => {
            let mut tools = workspace_tools();
            tools.push(tool(
                "todos_list",
                "Open todos across the whole workspace: id, owner, repository, title.",
                json!({}),
                &[],
            ));
            tools.extend(goal_tools().into_iter().filter(|tool| {
                ASK_GOAL_TOOLS.contains(&tool["name"].as_str().unwrap_or_default())
            }));
            tools.extend(scratchpad_tools().into_iter().filter(|tool| {
                ASK_SCRATCHPAD_TOOLS.contains(&tool["name"].as_str().unwrap_or_default())
            }));
            tools
        }
    })
}

/// Shared markdown documents, distinct from the task's own notes above. A
/// link is what a person pastes into a prompt, so opening one is the tool a
/// model reaches for first.
fn scratchpad_tools() -> Vec<Value> {
    let string = |about: &str| json!({ "type": "string", "description": about });
    let link = || string("The scratchpad's link, lgtm://scratchpads/<id>, or its id.");
    vec![
        tool("scratchpads_list", "Shared scratchpads for this repository: link, then title.", json!({}), &[]),
        tool("scratchpad_open", "Read a shared scratchpad by its lgtm://scratchpads/... link. A prompt, todo or message that carries one means: open it and act on it.", json!({ "link": link() }), &["link"]),
        tool("scratchpad_create", "Start a shared scratchpad for this repository. Returns its link.", json!({ "title": string("One line."), "content": string("The document, in markdown.") }), &["title"]),
        tool("scratchpad_update", "Replace a shared scratchpad's title, content, or both.", json!({ "link": link(), "title": string("The new title."), "content": string("The full document, in markdown.") }), &["link"]),
        tool("scratchpad_archive", "Archive a shared scratchpad. A person can restore it.", json!({ "link": link() }), &["link"]),
    ]
}

fn run_tools() -> Vec<Value> {
    let string = |about: &str| json!({ "type": "string", "description": about });
    let mut tools = vec![
        tool("memories_list", "Facts recorded for this repository that every agent run is told.", json!({}), &[]),
        tool("memory_propose", "Propose a fact worth telling every later run. It waits as a pending memory until a person approves it.", json!({ "content": string("The fact, in one sentence.") }), &["content"]),
        tool("todos_list", "Open todos for this repository.", json!({}), &[]),
        tool("todo_create", "Note work that should happen but is not part of this task.", json!({ "title": string("One line."), "description": string("Optional detail.") }), &["title"]),
        tool("scratchpad_read", "This task's own working notes, private to it.", json!({}), &[]),
        tool("scratchpad_write", "Replace this task's own working notes.", json!({ "content": string("The full notes, in markdown.") }), &["content"]),
    ];
    tools.extend(scratchpad_tools());
    tools.push(
        tool("request_network", "Ask a person to allow this task to reach a host its sandbox refused. Recorded for the task's next run, not this one.", json!({ "host": string("The host to allow, e.g. registry.internal."), "reason": string("Why the run needs it.") }), &["host", "reason"]),
    );
    tools
}

/// Reads over the whole workspace, not one goal: what else is running, who
/// started it, and where two tasks are about to collide.
fn workspace_tools() -> Vec<Value> {
    vec![
        tool("goals_list", "Every goal in the workspace: id, status, owner, task count, objective.", json!({}), &[]),
        tool("sessions_list", "Every chat session in the workspace: id, owner, repository, title.", json!({}), &[]),
        tool("tasks_list", "Every task in the workspace: id, status, owner, repository, prompt — and which unmerged tasks changed the same files.", json!({}), &[]),
        tool("activity", "The most recent events across every task: who did what, where.", json!({ "limit": { "type": "integer", "description": "How many lines, default 30." } }), &[]),
    ]
}

fn goal_tools() -> Vec<Value> {
    let string = |about: &str| json!({ "type": "string", "description": about });
    let task_id = || json!({ "task_id": string("Id of a task under this goal.") });
    vec![
        tool("goal_inspect", "The goal's objective, its status, and one line per task under it.", json!({}), &[]),
        tool("task_inspect", "Everything recorded for one task: its prompt, attempts, checks, blocking findings, changed files, recent activity and notes.", task_id(), &["task_id"]),
        tool("task_create", "Add a task the goal needs. It runs in the goal's repository, off the same base branch.", json!({
            "title": string("One line."),
            "prompt": string("Full instructions for a coding agent."),
            "depends_on": { "type": "array", "items": { "type": "string" }, "description": "Ids of tasks under this goal that must finish first." },
            "reason": string("Why the goal needs it."),
        }), &["title", "prompt"]),
        tool("task_message", "Send a follow-up to a task so its agent fixes something itself.", json!({ "task_id": string("Id of a task under this goal."), "text": string("What to tell the agent."), "reason": string("Why.") }), &["task_id", "text"]),
        tool("task_retry", "Requeue a task that crashed or timed out.", json!({ "task_id": string("Id of a task under this goal."), "runner": string("Optional runner to move it to."), "executor": string("Optional executor to swap to: claude or codex."), "reason": string("Why.") }), &["task_id"]),
        tool("task_approve", "Approve and push a task. Refused unless the checks passed and no blocking review finding is left.", json!({ "task_id": string("Id of a task under this goal."), "reason": string("Why.") }), &["task_id"]),
        tool("runner_list", "The connected runners and what they are running.", json!({}), &[]),
        tool("wait", "Stop and leave the goal to a person. The next task or message under the goal clears it.", json!({ "reason": string("What a person has to decide or do.") }), &["reason"]),
    ]
}

fn tool(name: &str, about: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": about,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(goal_id: Option<&str>) -> Env {
        let task_id = "t1".to_string();
        let repository = "https://example.com/r.git".to_string();
        match goal_id {
            Some(goal_id) => Env::Orchestrate {
                task_id,
                repository,
                goal_id: goal_id.to_string(),
            },
            None => Env::Run {
                task_id,
                repository,
            },
        }
    }

    async fn answer_in(request: Value, env: Env) -> Option<Value> {
        let client = Client::new("http://127.0.0.1:1", "tok");
        handle(&request, &client, &env).await
    }

    async fn answer(request: Value, goal_id: Option<&str>) -> Option<Value> {
        answer_in(request, env(goal_id)).await
    }

    async fn names_in(env: Env) -> Vec<String> {
        let reply = answer_in(
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            env,
        )
        .await
        .unwrap();
        reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    async fn tool_names(goal_id: Option<&str>) -> Vec<String> {
        names_in(env(goal_id)).await
    }

    #[test]
    fn the_run_tool_names_and_their_schemas_cannot_drift() {
        let names: Vec<String> = run_tools()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(names, RUN_TOOLS);
    }

    #[test]
    fn proposed_reply_names_the_approve_command() {
        assert_eq!(
            proposed_reply("m1"),
            "proposed m1; a person approves it with: lgtm memory approve m1"
        );
    }

    #[tokio::test]
    async fn initialize_names_the_protocol_and_the_server() {
        let reply = answer(
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL);
        assert_eq!(reply["result"]["serverInfo"]["name"], "lgtm");
    }

    #[tokio::test]
    async fn a_run_without_a_goal_gets_only_the_run_tools() {
        assert_eq!(
            tool_names(None).await,
            [
                "memories_list",
                "memory_propose",
                "todos_list",
                "todo_create",
                "scratchpad_read",
                "scratchpad_write",
                "scratchpads_list",
                "scratchpad_open",
                "scratchpad_create",
                "scratchpad_update",
                "scratchpad_archive",
                "request_network"
            ]
        );
    }

    #[test]
    fn every_link_shape_and_a_bare_id_name_the_same_scratchpad() {
        for link in [
            "lgtm://scratchpads/https%3A%2F%2Fexample.com%2Fr.git/sp1",
            "lgtm://scratchpads/sp1",
            "lgtm://scratchpads/sp1/",
            "sp1",
        ] {
            assert_eq!(scratchpad_id(link), "sp1", "{link}");
        }
    }

    #[tokio::test]
    async fn a_goal_id_unlocks_the_orchestration_tools() {
        let names = tool_names(Some("g1")).await;
        for name in [
            "goal_inspect",
            "task_inspect",
            "task_create",
            "task_message",
            "task_retry",
            "task_approve",
            "runner_list",
            "wait",
        ] {
            assert!(
                names.contains(&name.to_string()),
                "{name} missing: {names:?}"
            );
        }
    }

    #[tokio::test]
    async fn orchestrate_mode_adds_the_workspace_tools() {
        let with_goal = tool_names(Some("g1")).await;
        let plain = tool_names(None).await;
        for name in ["goals_list", "sessions_list", "tasks_list", "activity"] {
            assert!(
                with_goal.contains(&name.to_string()),
                "{name} missing: {with_goal:?}"
            );
            assert!(!plain.contains(&name.to_string()), "{name} leaked to a run");
        }
    }

    #[tokio::test]
    async fn ask_mode_serves_only_workspace_reads() {
        assert_eq!(
            names_in(Env::Ask).await,
            [
                "goals_list",
                "sessions_list",
                "tasks_list",
                "activity",
                "todos_list",
                "task_inspect",
                "runner_list",
                "scratchpads_list",
                "scratchpad_open"
            ]
        );
    }

    #[tokio::test]
    async fn a_write_tool_is_refused_in_ask_mode() {
        for name in ["task_create", "task_approve", "scratchpad_create"] {
            let call = json!({
                "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": { "name": name, "arguments": { "task_id": "t2" } },
            });
            let reply = answer_in(call, Env::Ask).await.unwrap();
            assert_eq!(reply["result"]["isError"], true, "{name}");
            assert!(reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("no such tool"));
        }
    }

    #[test]
    fn repo_short_keeps_the_repository_name() {
        assert_eq!(repo_short("https://github.com/o/lgtm.git"), "lgtm");
        assert_eq!(repo_short("https://github.com/o/lgtm"), "lgtm");
        assert_eq!(repo_short("lgtm"), "lgtm");
    }

    #[test]
    fn age_counts_minutes_then_hours_then_days() {
        let now = 10 * 24 * 60 * 60_000;
        assert_eq!(age(now, now), "0m");
        assert_eq!(age(now, now - 59 * 60_000), "59m");
        assert_eq!(age(now, now - 60 * 60_000), "1h");
        assert_eq!(age(now, now - 47 * 60 * 60_000), "47h");
        assert_eq!(age(now, now - 48 * 60 * 60_000), "2d");
    }

    #[tokio::test]
    async fn an_orchestration_tool_is_refused_without_a_goal() {
        let call = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "goal_inspect", "arguments": {} },
        });
        let reply = answer(call, None).await.unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("no such tool"));
    }

    #[tokio::test]
    async fn an_unknown_method_is_an_error_reply() {
        let reply = answer(
            json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(reply["error"]["code"], -32601);
        assert!(reply.get("result").is_none());
    }

    #[tokio::test]
    async fn a_notification_gets_no_reply() {
        assert!(answer(
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            None
        )
        .await
        .is_none());
    }
}
