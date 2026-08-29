//! `lgtm mcp`: a stdio MCP server that hands one agent run the task's
//! context — the repository's memories and todos, and the task's scratchpad.
//! The runner registers it with claude and codex for every run.

use anyhow::Result;
use lgtm_client::Client;
use lgtm_protocol::TodoStatus;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL: &str = "2024-11-05";

/// Which run this server answers for. The harness spawns it with no
/// arguments of its own, so the runner passes this in the environment.
pub struct Env {
    task_id: String,
    repository: String,
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
        "tools/list" => Ok(json!({ "tools": tools() })),
        "ping" => Ok(json!({})),
        "tools/call" => Ok(match call(params, client, env).await {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(err) => json!({
                "content": [{ "type": "text", "text": format!("{err:#}") }],
                "isError": true,
            }),
        }),
        _ => Err(json!({ "code": -32601, "message": format!("no such method: {method}") })),
    }
}

async fn call(params: &Value, client: &Client, env: &Env) -> Result<String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let repository = Some(env.repository.as_str());
    match name {
        "memories_list" => Ok(joined(
            client
                .memories(repository)
                .await?
                .iter()
                .map(|m| format!("- {}", m.content)),
        )),
        "memory_propose" => propose(client, repository, &args).await,
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
                    string(&args, "title")?,
                    text(&args, "description"),
                )
                .await?;
            Ok(todo.id)
        }
        "scratchpad_read" => Ok(client.task(&env.task_id).await?.task.scratchpad),
        "scratchpad_write" => {
            client
                .set_scratchpad(&env.task_id, string(&args, "content")?)
                .await?;
            Ok("notes saved".to_string())
        }
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

/// A proposal lands as a todo rather than a memory: an agent should not be
/// able to write what every later run is told, so a person reads it and runs
/// `lgtm memory add`.
async fn propose(client: &Client, repository: Option<&str>, args: &Value) -> Result<String> {
    let title = format!("Proposed memory: {}", string(args, "content")?);
    Ok(client.create_todo(repository, &title, "").await?.id)
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

fn tools() -> Value {
    let string = |about: &str| json!({ "type": "string", "description": about });
    json!([
        tool("memories_list", "Facts recorded for this repository that every agent run is told.", json!({}), &[]),
        tool("memory_propose", "Propose a fact worth telling every later run. It becomes a todo for a person to accept.", json!({ "content": string("The fact, in one sentence.") }), &["content"]),
        tool("todos_list", "Open todos for this repository.", json!({}), &[]),
        tool("todo_create", "Note work that should happen but is not part of this task.", json!({ "title": string("One line."), "description": string("Optional detail.") }), &["title"]),
        tool("scratchpad_read", "The working notes kept for this task.", json!({}), &[]),
        tool("scratchpad_write", "Replace the working notes for this task.", json!({ "content": string("The full notes, in markdown.") }), &["content"]),
    ])
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

    fn env() -> Env {
        Env {
            task_id: "t1".to_string(),
            repository: "https://example.com/r.git".to_string(),
        }
    }

    async fn answer(request: Value) -> Option<Value> {
        let client = Client::new("http://127.0.0.1:1", "tok");
        handle(&request, &client, &env()).await
    }

    #[tokio::test]
    async fn initialize_names_the_protocol_and_the_server() {
        let reply = answer(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}))
            .await
            .unwrap();
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL);
        assert_eq!(reply["result"]["serverInfo"]["name"], "lgtm");
    }

    #[tokio::test]
    async fn tools_list_carries_every_tool_with_a_schema() {
        let reply = answer(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .await
            .unwrap();
        let tools = reply["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "memories_list",
                "memory_propose",
                "todos_list",
                "todo_create",
                "scratchpad_read",
                "scratchpad_write"
            ]
        );
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }

    #[tokio::test]
    async fn an_unknown_method_is_an_error_reply() {
        let reply = answer(json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}))
            .await
            .unwrap();
        assert_eq!(reply["error"]["code"], -32601);
        assert!(reply.get("result").is_none());
    }

    #[tokio::test]
    async fn a_notification_gets_no_reply() {
        assert!(
            answer(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .await
                .is_none()
        );
    }
}
