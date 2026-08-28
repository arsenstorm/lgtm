//! The worker agent's socket: `Hello` authenticates it, then it streams events.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use lgtm_protocol::{OrchestratorMessage, TaskEvent, TaskId, WorkerInfo, WorkerMessage};
use tokio::sync::mpsc;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

use crate::persist;
use crate::state::{App, WorkerConn};

pub async fn handler(State(app): State<Arc<App>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| run(app, socket))
}

async fn run(app: Arc<App>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let Some(info) = hello(&app, &mut sink, &mut stream).await else {
        return;
    };
    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::unbounded_channel();
    {
        // Replaces any previous connection under this name; the old writer task
        // ends when its receiver drops with the old WorkerConn.
        let mut state = app.state.lock().unwrap();
        state.workers.insert(
            info.name.clone(),
            WorkerConn {
                info: info.clone(),
                tx: tx.clone(),
                running: HashSet::new(),
                conn_id,
            },
        );
    }
    let _ = tx.send(OrchestratorMessage::HelloAck);

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&msg) else {
                continue;
            };
            if sink.send(Message::Text(text)).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(frame)) = stream.next().await {
        match frame {
            Message::Text(text) => match serde_json::from_str::<WorkerMessage>(&text) {
                Ok(WorkerMessage::Event { task_id, event }) => apply(&app, &task_id, event),
                Ok(WorkerMessage::Hello { .. }) => {}
                Err(err) => tracing::warn!(worker = %info.name, %err, "bad worker frame"),
            },
            Message::Close(_) => break,
            _ => {}
        }
    }
    writer.abort();
    disconnect(&app, &info.name, conn_id);
}

async fn hello(
    app: &App,
    sink: &mut SplitSink<WebSocket, Message>,
    stream: &mut SplitStream<WebSocket>,
) -> Option<WorkerInfo> {
    let frame = tokio::time::timeout(HELLO_TIMEOUT, stream.next())
        .await
        .ok()??
        .ok()?;
    let Message::Text(text) = frame else {
        return None;
    };
    let Ok(WorkerMessage::Hello { token, info }) = serde_json::from_str::<WorkerMessage>(&text)
    else {
        return None;
    };
    if token != app.token {
        tracing::warn!(worker = %info.name, "worker presented a bad token");
        let _ = sink.send(Message::Close(None)).await;
        return None;
    }
    tracing::info!(worker = %info.name, "worker connected");
    Some(info)
}

fn apply(app: &App, task_id: &str, event: TaskEvent) {
    let mut state = app.state.lock().unwrap();
    if let Some(rec) = state.apply_event(task_id, event) {
        persist::save(&app.tasks_dir, rec);
    }
}

fn disconnect(app: &App, name: &str, conn_id: u64) {
    let running: Vec<TaskId> = {
        let mut state = app.state.lock().unwrap();
        match state.workers.get(name) {
            // A newer socket already took this name over; leave it alone.
            Some(conn) if conn.conn_id != conn_id => return,
            Some(conn) => {
                let running = conn.running.iter().cloned().collect();
                state.workers.remove(name);
                running
            }
            None => return,
        }
    };
    tracing::info!(worker = %name, tasks = running.len(), "worker disconnected");
    for task_id in running {
        let unfinished = {
            let state = app.state.lock().unwrap();
            state
                .tasks
                .get(&task_id)
                .is_some_and(|rec| !rec.task.status.is_terminal())
        };
        if unfinished {
            apply(
                app,
                &task_id,
                TaskEvent::Failed {
                    error: "worker disconnected".into(),
                },
            );
        }
    }
}
