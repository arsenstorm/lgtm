//! `lgtm mcp`: a stdio MCP server that hands one agent run the task's
//! context — the repository's memories and todos, and the task's scratchpad.
//! The runner registers it with claude and codex for every run.
//!
//! With `LGTM_GOAL_ID` set it is the orchestration loop's server instead, and
//! carries the tools that inspect and act on a whole goal. Every one of those
//! goes through the endpoints a person uses, so LGTM validates them the same
//! way, and every call lands on the ended task's event log.

use anyhow::Result;
use lgtm_client::{Client, Orchestrated, Retry};
use lgtm_protocol::{Task, TaskKind, TaskSpec, TodoStatus};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL: &str = "2024-11-05";
/// How much of a task's chatter `task_inspect` carries.
const RECENT_EVENTS: usize = 20;

/// Which run this server answers for. The harness spawns it with no
/// arguments of its own, so the runner passes this in the environment.
pub struct Env {
    task_id: String,
    repository: String,
    /// Set only for the orchestration loop, which unlocks the goal tools.
    goal_id: Option<String>,
}

pub async fn serve(client: &Client) -> Result<i32> {
    let env = require_env();
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
        (Some(task_id), Some(repository)) => Env {
            task_id,
            repository,
            goal_id: var("LGTM_GOAL_ID"),
        },
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
        "tools/list" => Ok(json!({ "tools": tools(env.goal_id.is_some()) })),
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
    if env.goal_id.is_some() {
        let outcome = match &done {
            Ok(text) => text.clone(),
            Err(err) => format!("{err:#}"),
        };
        let _ = client
            .orchestrated(
                &env.task_id,
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
    let repository = Some(env.repository.as_str());
    match name {
        "memories_list" => Ok(joined(
            client
                .memories(repository, false)
                .await?
                .iter()
                .map(|m| format!("- {}", m.content)),
        )),
        "memory_propose" => propose(client, repository, env, args).await,
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
                )
                .await?;
            Ok(todo.id)
        }
        "scratchpad_read" => Ok(client.task(&env.task_id).await?.task.scratchpad),
        "scratchpad_write" => {
            client
                .set_scratchpad(&env.task_id, string(args, "content")?)
                .await?;
            Ok("notes saved".to_string())
        }
        "request_network" => request_network(client, env, args).await,
        _ => orchestration_call(name, args, client, env).await,
    }
}

/// Only reachable with `LGTM_GOAL_ID`, so a plain agent run cannot drive the
/// goal it is one task of.
async fn orchestration_call(
    name: &str,
    args: &Value,
    client: &Client,
    env: &Env,
) -> Result<String> {
    let goal = env
        .goal_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no such tool: {name}"))?;
    match name {
        "goal_inspect" => goal_inspect(client, goal).await,
        "task_inspect" => task_inspect(client, string(args, "task_id")?).await,
        "task_create" => task_create(client, goal, args).await,
        "task_message" => {
            client
                .tell(string(args, "task_id")?, string(args, "text")?)
                .await?;
            Ok("sent".to_string())
        }
        "task_retry" => {
            let into = Retry {
                runner: args.get("runner").and_then(Value::as_str).map(String::from),
                executor: serde_json::from_value(args.get("executor").cloned().unwrap_or_default())
                    .ok(),
            };
            client.retry(string(args, "task_id")?, &into).await?;
            Ok("requeued".to_string())
        }
        "task_approve" => {
            client
                .approve_as_orchestrator(string(args, "task_id")?)
                .await?;
            Ok("approved".to_string())
        }
        "runner_list" => Ok(joined(client.runners().await?.iter().map(|runner| {
            let executors: Vec<&str> = runner.info.executors.iter().map(|e| e.binary()).collect();
            format!(
                "{} {} running {}/{}",
                runner.info.name,
                executors.join(","),
                runner.running.len(),
                runner.info.slots,
            )
        }))),
        "wait" => {
            client
                .set_attention(goal, Some(string(args, "reason")?))
                .await?;
            Ok("recorded".to_string())
        }
        _ => anyhow::bail!("no such tool: {name}"),
    }
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
        goal: Some(goal.to_string()),
        allowed_hosts: Vec::new(),
        session: None,
    };
    Ok(client.create_task(&spec).await?.id)
}

/// An agent cannot write what every later run is told: the memory it
/// proposes waits unapproved until a person runs `lgtm memory approve`.
async fn propose(
    client: &Client,
    repository: Option<&str>,
    env: &Env,
    args: &Value,
) -> Result<String> {
    let memory = client
        .propose_memory(repository, string(args, "content")?, &env.task_id)
        .await?;
    Ok(proposed_reply(&memory.id))
}

fn proposed_reply(id: &str) -> String {
    format!("proposed {id}; a person approves it with: lgtm memory approve {id}")
}

/// A run can't be paused mid-flight to ask a person, so the request is only
/// recorded; `lgtm allow` answers it before the task's next run.
async fn request_network(client: &Client, env: &Env, args: &Value) -> Result<String> {
    let host = string(args, "host")?;
    client
        .request_permission(&env.task_id, "network", host, string(args, "reason")?)
        .await?;
    Ok(format!(
        "recorded; a person can allow it with: lgtm allow {} {host}",
        env.task_id
    ))
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

fn tools(goal: bool) -> Value {
    let string = |about: &str| json!({ "type": "string", "description": about });
    let mut tools = vec![
        tool("memories_list", "Facts recorded for this repository that every agent run is told.", json!({}), &[]),
        tool("memory_propose", "Propose a fact worth telling every later run. It waits as a pending memory until a person approves it.", json!({ "content": string("The fact, in one sentence.") }), &["content"]),
        tool("todos_list", "Open todos for this repository.", json!({}), &[]),
        tool("todo_create", "Note work that should happen but is not part of this task.", json!({ "title": string("One line."), "description": string("Optional detail.") }), &["title"]),
        tool("scratchpad_read", "The working notes kept for this task.", json!({}), &[]),
        tool("scratchpad_write", "Replace the working notes for this task.", json!({ "content": string("The full notes, in markdown.") }), &["content"]),
        tool("request_network", "Ask a person to allow this task to reach a host its sandbox refused. Recorded for the task's next run, not this one.", json!({ "host": string("The host to allow, e.g. registry.internal."), "reason": string("Why the run needs it.") }), &["host", "reason"]),
    ];
    if goal {
        tools.extend(goal_tools());
    }
    Value::Array(tools)
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
        Env {
            task_id: "t1".to_string(),
            repository: "https://example.com/r.git".to_string(),
            goal_id: goal_id.map(str::to_string),
        }
    }

    async fn answer(request: Value, goal_id: Option<&str>) -> Option<Value> {
        let client = Client::new("http://127.0.0.1:1", "tok");
        handle(&request, &client, &env(goal_id)).await
    }

    async fn tool_names(goal_id: Option<&str>) -> Vec<String> {
        let reply = answer(
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            goal_id,
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
                "request_network"
            ]
        );
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
