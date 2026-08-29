//! Outbound WebSocket connection to the orchestrator, with reconnect.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use lgtm_protocol::{
    OrchestratorMessage, TaskEvent, TaskId, WorkerInfo, WorkerMessage, WORKER_WS_PATH,
};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use crate::runner;

const RETRY: Duration = Duration::from_secs(3);

/// Shared state every task runner needs.
pub struct Ctx {
    pub data_dir: PathBuf,
    pub tx: mpsc::UnboundedSender<WorkerMessage>,
    pub running: Mutex<HashMap<TaskId, oneshot::Sender<()>>>,
    /// Mirror path per task, so a discard after a restart of the task still
    /// knows which bare clone owns the worktree.
    pub mirrors: Mutex<HashMap<TaskId, PathBuf>>,
}

impl Ctx {
    pub fn emit(&self, task_id: &str, event: TaskEvent) {
        let _ = self.tx.send(WorkerMessage::Event {
            task_id: task_id.to_string(),
            event,
        });
    }
}

pub async fn run(
    orchestrator: &str,
    token: &str,
    info: &WorkerInfo,
    ctx: Arc<Ctx>,
    mut rx: mpsc::UnboundedReceiver<WorkerMessage>,
) {
    let url = format!("{orchestrator}{WORKER_WS_PATH}");
    loop {
        match session(&url, token, info, &ctx, &mut rx).await {
            Ok(()) => tracing::warn!("disconnected"),
            Err(err) => tracing::warn!("connection failed: {err:#}"),
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn session(
    url: &str,
    token: &str,
    info: &WorkerInfo,
    ctx: &Arc<Ctx>,
    rx: &mut mpsc::UnboundedReceiver<WorkerMessage>,
) -> Result<()> {
    let (ws, _) = tokio_tungstenite::connect_async(url)
        .await
        .with_context(|| format!("connect {url}"))?;
    let (mut sink, mut stream) = ws.split();

    let running = ctx
        .running
        .lock()
        .expect("running map poisoned")
        .keys()
        .cloned()
        .collect();
    let hello = WorkerMessage::Hello {
        token: token.to_string(),
        info: info.clone(),
        running,
    };
    sink.send(Message::Text(serde_json::to_string(&hello)?))
        .await?;

    match stream.next().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
            Ok(OrchestratorMessage::HelloAck) => {}
            _ => bail!("expected hello_ack, got {text}"),
        },
        other => bail!("expected hello_ack, got {other:?}"),
    }
    tracing::info!("connected to {url}");

    loop {
        tokio::select! {
            outbound = rx.recv() => {
                let Some(msg) = outbound else { return Ok(()) };
                sink.send(Message::Text(serde_json::to_string(&msg)?)).await?;
            }
            inbound = stream.next() => {
                match inbound {
                    Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
                        Ok(msg) => dispatch(msg, ctx),
                        Err(err) => tracing::warn!("bad frame: {err} ({text})"),
                    },
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Ok(_)) => {}
                    Some(Err(err)) => return Err(err.into()),
                }
            }
        }
    }
}

fn dispatch(msg: OrchestratorMessage, ctx: &Arc<Ctx>) {
    match msg {
        OrchestratorMessage::HelloAck => {}
        OrchestratorMessage::Start { task } => {
            let (cancel_tx, cancel_rx) = oneshot::channel();
            ctx.running
                .lock()
                .expect("running map poisoned")
                .insert(task.id.clone(), cancel_tx);
            tokio::spawn(runner::run_task(*task, ctx.clone(), cancel_rx));
        }
        OrchestratorMessage::Cancel { task_id } => {
            let sender = ctx
                .running
                .lock()
                .expect("running map poisoned")
                .remove(&task_id);
            if let Some(sender) = sender {
                let _ = sender.send(());
            }
        }
        OrchestratorMessage::Push { task_id } => {
            tokio::spawn(runner::push_task(task_id, ctx.clone()));
        }
        OrchestratorMessage::Discard { task_id } => {
            tokio::spawn(runner::discard_task(task_id, ctx.clone()));
        }
    }
}
