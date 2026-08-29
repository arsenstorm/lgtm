//! `lgtm run`: submit a task, then stream and render its events live.

use crate::http::Client;
use crate::render;
use futures_util::StreamExt;
use lgtm_protocol::{Executor, Review, StoredEvent, Task, TaskEvent, TaskKind, TaskSpec};
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;

/// Body of `POST /api/tasks/from-issue`.
#[derive(Serialize)]
struct FromIssueSpec {
    issue: String,
    base_branch: String,
    executor: Executor,
    worker: Option<String>,
}

/// Body of `POST /api/tasks/from-linear`.
#[derive(Serialize)]
struct FromLinearSpec {
    issue: String,
    repository: String,
    base_branch: String,
    executor: Executor,
    worker: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    client: &Client,
    orchestrator: &str,
    token: &str,
    ca: Option<&Path>,
    repository: String,
    base_branch: String,
    prompt: String,
    executor: Executor,
    worker: Option<String>,
    kind: TaskKind,
) -> anyhow::Result<i32> {
    let spec = TaskSpec {
        repository,
        base_branch,
        prompt,
        executor,
        worker,
        issue: None,
        linear: None,
        kind,
        parent: None,
        depends_on: vec![],
    };
    let task: Task = client.post("/api/tasks", Some(&spec)).await?;
    announce_and_stream(orchestrator, token, ca, task).await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_from_issue(
    client: &Client,
    orchestrator: &str,
    token: &str,
    ca: Option<&Path>,
    issue: String,
    base_branch: String,
    executor: Executor,
    worker: Option<String>,
) -> anyhow::Result<i32> {
    let spec = FromIssueSpec {
        issue,
        base_branch,
        executor,
        worker,
    };
    let task: Task = client.post("/api/tasks/from-issue", Some(&spec)).await?;
    announce_and_stream(orchestrator, token, ca, task).await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_from_linear(
    client: &Client,
    orchestrator: &str,
    token: &str,
    ca: Option<&Path>,
    issue: String,
    repository: String,
    base_branch: String,
    executor: Executor,
    worker: Option<String>,
) -> anyhow::Result<i32> {
    let spec = FromLinearSpec {
        issue,
        repository,
        base_branch,
        executor,
        worker,
    };
    let task: Task = client.post("/api/tasks/from-linear", Some(&spec)).await?;
    announce_and_stream(orchestrator, token, ca, task).await
}

async fn announce_and_stream(
    orchestrator: &str,
    token: &str,
    ca: Option<&Path>,
    task: Task,
) -> anyhow::Result<i32> {
    match task.worker.as_deref() {
        Some(worker) => eprintln!("task {} → {worker}", task.id),
        None => eprintln!("task {} queued, waiting for a worker", task.id),
    }
    stream(orchestrator, token, ca, &task.id, 0).await
}

/// Connect to the task's event stream from event index `from` and render
/// events until a terminal one arrives. Shared by `run` (from 0) and `tell`
/// (from the count of events already seen, so a follow-up doesn't replay
/// the whole history).
pub async fn stream(
    orchestrator: &str,
    token: &str,
    ca: Option<&Path>,
    task_id: &str,
    from: usize,
) -> anyhow::Result<i32> {
    let mut request = events_url(orchestrator, task_id, from)?.into_client_request()?;
    request.headers_mut().insert(
        "Authorization",
        HeaderValue::from_str(&format!("Bearer {token}"))?,
    );
    let (ws_stream, _) = match ca {
        Some(ca_path) => {
            let connector = ca_connector(ca_path)?;
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector))
                .await?
        }
        None => tokio_tungstenite::connect_async(request).await?,
    };
    let (_write, mut read) = ws_stream.split();

    let mut stdout = std::io::stdout();
    while let Some(msg) = read.next().await {
        let Message::Text(text) = msg? else {
            continue;
        };
        let stored: StoredEvent = serde_json::from_str(&text)?;
        render::render(&stored.event, &mut stdout)?;
        match stored.event {
            TaskEvent::Completed { result } => match &result.plan {
                Some(plan) => {
                    println!();
                    render::print_plan(plan, &mut stdout)?;
                    return Ok(0);
                }
                None => {
                    println!("\n{} files changed", result.changed_files.len());
                    println!("{}", result.diff);
                    render::print_validation(&result.validation, &mut stdout)?;
                    if let Some(review) = &result.review {
                        render::print_review(review, &mut stdout)?;
                    }
                    render::print_cost(result.cost_usd, &mut stdout)?;
                    let blocking = result.review.as_ref().is_some_and(Review::has_blocking);
                    return Ok(if result.validation_failed() || blocking {
                        3
                    } else {
                        0
                    });
                }
            },
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

/// Rustls connector that trusts the usual webpki roots plus `ca_path`, for
/// orchestrators serving a self-signed cert (`--ca` / `LGTM_CA`).
///
/// Picks the `ring` crypto provider explicitly rather than going through
/// `rustls::ClientConfig::builder()`'s process-default lookup: this binary
/// links both `ring` and `aws-lc-rs` (via reqwest), which makes that lookup
/// ambiguous and panics unless something already installed a default.
fn ca_connector(ca_path: &Path) -> anyhow::Result<Connector> {
    let pem = std::fs::read(ca_path)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", ca_path.display()))?;
    let ca_certs = rustls_pemfile::certs(&mut pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", ca_path.display()))?;
    if ca_certs.is_empty() {
        anyhow::bail!("no certificates found in {}", ca_path.display());
    }

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for cert in ca_certs {
        roots.add(cert)?;
    }

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| anyhow::anyhow!("building TLS config: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(config)))
}

/// `http(s)://host[:port]` -> `ws(s)://host[:port]/api/tasks/<id>/events`,
/// with `?from=<from>` appended unless `from` is 0 (the full history).
fn events_url(orchestrator: &str, task_id: &str, from: usize) -> anyhow::Result<String> {
    let (scheme, rest) = if let Some(rest) = orchestrator.strip_prefix("https://") {
        ("wss://", rest)
    } else if let Some(rest) = orchestrator.strip_prefix("http://") {
        ("ws://", rest)
    } else {
        anyhow::bail!("orchestrator URL must start with http:// or https://");
    };
    let rest = rest.trim_end_matches('/');
    if from == 0 {
        Ok(format!("{scheme}{rest}/api/tasks/{task_id}/events"))
    } else {
        Ok(format!(
            "{scheme}{rest}/api/tasks/{task_id}/events?from={from}"
        ))
    }
}
