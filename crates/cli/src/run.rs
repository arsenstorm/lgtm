//! `lgtm run`: submit a task, then stream and render its events live.

use crate::http::Client;
use crate::render;
use futures_util::StreamExt;
use lgtm_protocol::{Executor, StoredEvent, Task, TaskEvent, TaskSpec};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

#[allow(clippy::too_many_arguments)]
pub async fn run(
    client: &Client,
    orchestrator: &str,
    token: &str,
    repository: String,
    base_branch: String,
    prompt: String,
    executor: Executor,
    worker: Option<String>,
) -> anyhow::Result<i32> {
    let spec = TaskSpec {
        repository,
        base_branch,
        prompt,
        executor,
        worker,
    };
    let task: Task = client.post("/api/tasks", Some(&spec)).await?;
    match task.worker.as_deref() {
        Some(worker) => eprintln!("task {} → {worker}", task.id),
        None => eprintln!("task {} queued, waiting for a worker", task.id),
    }

    let mut request = events_url(orchestrator, &task.id)?.into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
    let (_write, mut read) = ws_stream.split();

    let mut stdout = std::io::stdout();
    while let Some(msg) = read.next().await {
        let Message::Text(text) = msg? else {
            continue;
        };
        let stored: StoredEvent = serde_json::from_str(&text)?;
        render::render(&stored.event, &mut stdout)?;
        match stored.event {
            TaskEvent::Completed { result } => {
                println!("\n{} files changed", result.changed_files.len());
                println!("{}", result.diff);
                return Ok(0);
            }
            TaskEvent::Failed { error } => {
                eprintln!("error: {error}");
                return Ok(1);
            }
            TaskEvent::Cancelled => {
                eprintln!("cancelled");
                return Ok(130);
            }
            _ => {}
        }
    }
    eprintln!("connection closed");
    Ok(1)
}

/// `http(s)://host[:port]` -> `ws(s)://host[:port]/api/tasks/<id>/events`.
fn events_url(orchestrator: &str, task_id: &str) -> anyhow::Result<String> {
    let (scheme, rest) = if let Some(rest) = orchestrator.strip_prefix("https://") {
        ("wss://", rest)
    } else if let Some(rest) = orchestrator.strip_prefix("http://") {
        ("ws://", rest)
    } else {
        anyhow::bail!("orchestrator URL must start with http:// or https://");
    };
    let rest = rest.trim_end_matches('/');
    Ok(format!("{scheme}{rest}/api/tasks/{task_id}/events"))
}
