//! `lgtm mcp` against a running orchestrator: the framing on stdio and one
//! tool round trip through the HTTP API.

use std::process::Stdio;

use lgtm_protocol::{Executor, Task, TaskKind, TaskSpec, TaskStatus};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const TASK_ID: &str = "0000abcd";
const REPOSITORY: &str = "https://example.com/r.git";

/// A task the orchestrator loads at startup, so the round trip needs no
/// worker to have ever connected. The id must be 8 hex digits or the
/// orchestrator refuses to load the record.
fn seed(dir: &std::path::Path) {
    let task = Task {
        id: TASK_ID.to_string(),
        spec: TaskSpec {
            repository: REPOSITORY.to_string(),
            base_branch: "main".to_string(),
            prompt: "p".to_string(),
            executor: Executor::Claude,
            worker: None,
            issue: None,
            linear: None,
            kind: TaskKind::Run,
            parent: None,
            depends_on: Vec::new(),
            depends_on_condition: Default::default(),
            batch: None,
            sandbox: None,
            requirements: Vec::new(),
            review_executor: None,
            model: None,
            goal: None,
            allowed_hosts: Vec::new(),
        },
        status: TaskStatus::AwaitingReview,
        worker: None,
        created_at: 0,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        executions: Vec::new(),
        scratchpad: String::new(),
    };
    let tasks = dir.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let stored = json!({ "task": task, "events": [] });
    std::fs::write(
        tasks.join(format!("{TASK_ID}.json")),
        serde_json::to_vec(&stored).unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn the_server_answers_initialize_and_round_trips_the_scratchpad() {
    let dir = std::env::temp_dir().join(format!("lgtm-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    seed(&dir);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_lgtm"))
        .arg("mcp")
        .env("LGTM_ORCHESTRATOR", format!("http://{addr}"))
        .env("LGTM_TOKEN", "tok")
        .env("LGTM_TASK_ID", TASK_ID)
        .env("LGTM_REPOSITORY", REPOSITORY)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();

    for request in [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "scratchpad_write", "arguments": { "content": "what I found" }
        }}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
            "name": "scratchpad_read", "arguments": {}
        }}),
        json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {
            "name": "request_network", "arguments": { "host": "registry.internal", "reason": "install a private package" }
        }}),
    ] {
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
    }

    let mut replies = Vec::new();
    while replies.len() < 4 {
        let line = stdout.next_line().await.unwrap().expect("server exited");
        replies.push(serde_json::from_str::<Value>(&line).unwrap());
    }
    assert_eq!(replies[0]["result"]["protocolVersion"], "2024-11-05");
    // The notification is the reply that must not exist: the ids stay in step.
    assert_eq!(replies[1]["id"], 2);
    assert!(replies[1]["result"]["isError"].is_null(), "{}", replies[1]);
    assert_eq!(replies[2]["id"], 3);
    assert_eq!(replies[2]["result"]["content"][0]["text"], "what I found");
    assert_eq!(replies[3]["id"], 4);
    assert_eq!(
        replies[3]["result"]["content"][0]["text"],
        format!("recorded; a person can allow it with: lgtm allow {TASK_ID} registry.internal")
    );

    let _ = std::fs::remove_dir_all(&dir);
}
