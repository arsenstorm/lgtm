//! The worker agent's socket: `Hello` authenticates it, then it streams events.

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
/// How long a worker's tasks survive its socket, so a restarting agent or a
/// flaky network does not throw away work that is still running.
const GRACE: Duration = Duration::from_secs(30);
static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

use crate::state::{App, Conn};

pub async fn handler(State(app): State<Arc<App>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| run(app, socket))
}

async fn run(app: Arc<App>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let Some((info, running)) = hello(&app, &mut sink, &mut stream).await else {
        return;
    };
    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::unbounded_channel();
    // Queued before anything the hello schedules, so the ack stays first.
    let _ = tx.send(OrchestratorMessage::HelloAck);
    {
        let mut state = app.state.lock().unwrap();
        let changed = state.worker_hello(
            info.clone(),
            running,
            Conn {
                tx: tx.clone(),
                conn_id,
            },
        );
        app.persist_ids(&state, &changed);
    }

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
) -> Option<(WorkerInfo, Vec<TaskId>)> {
    let frame = tokio::time::timeout(HELLO_TIMEOUT, stream.next())
        .await
        .ok()??
        .ok()?;
    let Message::Text(text) = frame else {
        return None;
    };
    let Ok(WorkerMessage::Hello {
        token,
        info,
        running,
    }) = serde_json::from_str::<WorkerMessage>(&text)
    else {
        return None;
    };
    if token != app.token {
        tracing::warn!(worker = %info.name, "worker presented a bad token");
        let _ = sink.send(Message::Close(None)).await;
        return None;
    }
    Some((info, running))
}

fn apply(app: &Arc<App>, task_id: &str, event: TaskEvent) {
    let pushed = matches!(event, TaskEvent::Pushed { .. });
    let (previous, plan) = {
        let mut state = app.state.lock().unwrap();
        let previous = state.tasks.get(task_id).map(|rec| rec.task.status);
        let changed = state.apply_event(task_id, event);
        app.persist_ids(&state, &changed);
        let plan = pushed
            .then(|| state.pull_request_plan(task_id, app.github.is_some()))
            .flatten();
        (previous, plan)
    };
    if let Some(previous) = previous {
        crate::linear::after_transition(app, task_id, previous, false);
    }
    if let Some(plan) = plan {
        crate::github::open_pull_request(app.clone(), task_id.to_string(), plan);
    }
}

fn disconnect(app: &Arc<App>, name: &str, conn_id: u64) {
    let Some(generation) = ({
        let mut state = app.state.lock().unwrap();
        state.disconnect(name, conn_id)
    }) else {
        return;
    };
    let app = app.clone();
    let name = name.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(GRACE).await;
        let mut state = app.state.lock().unwrap();
        let changed = state.expire_worker(&name, generation);
        app.persist_ids(&state, &changed);
    });
}
