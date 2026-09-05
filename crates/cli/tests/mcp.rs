//! `lgtm mcp` against a running orchestrator: the framing on stdio and one
//! tool round trip through the HTTP API.

use std::process::Stdio;

use lgtm_protocol::{Executor, Task, TaskKind, TaskSpec, TaskStatus};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const TASK_ID: &str = "0000abcd";
const REPOSITORY: &str = "https://example.com/r.git";

/// A task the orchestrator loads at startup, so the round trip needs no
/// runner to have ever connected. The id must be 8 hex digits or the
/// orchestrator refuses to load the record.
fn seed(dir: &std::path::Path) {
    let task = Task {
        id: TASK_ID.to_string(),
        title: None,
        spec: TaskSpec {
            repository: REPOSITORY.to_string(),
            base_branch: "main".to_string(),
            prompt: "p".to_string(),
            executor: Executor::Claude,
            runner: None,
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
            reasoning_effort: None,
            goal: None,
            allowed_hosts: Vec::new(),
            created_by: None,
        },
        status: TaskStatus::AwaitingReview,
        runner: None,
        created_at: 0,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        pr_review: None,
        executions: Vec::new(),
        scratchpad: String::new(),
        files: Vec::new(),
        workspace: None,
        created_by: None,
        archived: false,
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
async fn the_server_answers_initialize_round_trips_notes_and_serves_a_scratchpad_resource() {
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

    // A shared scratchpad made by a tool is readable back as a resource by the
    // link the tool returned: the same link the web app copies.
    let create = json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
        "name": "scratchpad_create", "arguments": { "title": "Runner notes", "content": "No sandbox on Windows." }
    }});
    stdin
        .write_all(format!("{create}\n").as_bytes())
        .await
        .unwrap();
    let line = stdout.next_line().await.unwrap().expect("server exited");
    let created = serde_json::from_str::<Value>(&line).unwrap();
    let link = created["result"]["content"][0]["text"].as_str().unwrap();
    assert!(link.starts_with("lgtm://scratchpads/"), "{created}");

    for request in [
        json!({"jsonrpc": "2.0", "id": 6, "method": "resources/list"}),
        json!({"jsonrpc": "2.0", "id": 7, "method": "resources/read", "params": { "uri": link }}),
    ] {
        stdin
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
    }
    let mut replies = Vec::new();
    while replies.len() < 2 {
        let line = stdout.next_line().await.unwrap().expect("server exited");
        replies.push(serde_json::from_str::<Value>(&line).unwrap());
    }
    let listed = replies[0]["result"]["resources"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .any(|r| r["uri"] == link && r["name"] == "Runner notes"),
        "{}",
        replies[0]
    );
    assert_eq!(
        replies[1]["result"]["contents"][0]["text"],
        "# Runner notes\n\nNo sandbox on Windows."
    );

    let _ = std::fs::remove_dir_all(&dir);
}
