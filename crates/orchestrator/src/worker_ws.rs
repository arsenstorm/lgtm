//! The worker agent's socket: `Hello` authenticates it, then it streams events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use lgtm_protocol::{
    OrchestratorMessage, TaskEvent, TaskId, TaskStatus, WorkerInfo, WorkerMessage,
};
use tokio::sync::mpsc;

use crate::policy::AutoAction;

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
    let (tx, rx) = mpsc::unbounded_channel();
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

    let writer = tokio::spawn(write_all(sink, rx));
    while let Some(Ok(frame)) = stream.next().await {
        let text = match frame {
            Message::Text(text) => text,
            Message::Close(_) => break,
            _ => continue,
        };
        if handle(&app, &info, conn_id, &text) {
            break;
        }
    }
    writer.abort();
    disconnect(&app, &info.name, conn_id);
}

async fn write_all(
    mut sink: SplitSink<WebSocket, Message>,
    mut rx: mpsc::UnboundedReceiver<OrchestratorMessage>,
) {
    while let Some(msg) = rx.recv().await {
        let Ok(text) = serde_json::to_string(&msg) else {
            continue;
        };
        if sink.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}

/// Applies one frame; `true` when the worker said goodbye and the socket is done.
fn handle(app: &Arc<App>, info: &WorkerInfo, conn_id: u64, text: &str) -> bool {
    match serde_json::from_str::<WorkerMessage>(text) {
        Ok(WorkerMessage::Event { task_id, event }) => apply(app, &task_id, event),
        // Deliberate exit: forget the worker now. `disconnect` finds nothing
        // left and so starts no grace timer.
        Ok(WorkerMessage::Goodbye) => {
            let mut state = app.state.lock().unwrap();
            let changed = state.worker_goodbye(&info.name, conn_id);
            app.persist_ids(&state, &changed);
            return true;
        }
        Ok(WorkerMessage::Hello { .. }) => {}
        Err(err) => tracing::warn!(worker = %info.name, %err, "bad worker frame"),
    }
    false
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

/// Approves a completed task the policy says needs no one to look at it. The
/// `Pushed` the worker sends back is what actually moves it to `Approved`.
fn auto_approve(app: &App, state: &mut crate::state::State, task_id: &str) {
    let Some(task) = state.tasks.get(task_id).map(|rec| rec.task.clone()) else {
        return;
    };
    if crate::policy::auto_action(&task) != Some(AutoAction::Approve) {
        return;
    }
    // Asking the worker first, so a task is not marked auto-approved by an
    // event no one can act on.
    if let Err(err) = state.command(
        task_id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Push { task_id },
    ) {
        tracing::warn!(task = %task_id, ?err, "auto-approve skipped");
        return;
    }
    tracing::info!(task = %task_id, "auto-approved by policy");
    let changed = state.apply_event(task_id, TaskEvent::AutoApproved);
    app.persist_ids(state, &changed);
}

fn apply(app: &Arc<App>, task_id: &str, event: TaskEvent) {
    let pushed = matches!(event, TaskEvent::Pushed { .. });
    let completed = matches!(event, TaskEvent::Completed { .. });
    let (previous, plan) = {
        let mut state = app.state.lock().unwrap();
        let previous = state.tasks.get(task_id).map(|rec| rec.task.status);
        let changed = state.apply_event(task_id, event);
        app.persist_ids(&state, &changed);
        if completed {
            auto_approve(app, &mut state, task_id);
            let changed = state.auto_approve_plan(task_id);
            app.persist_ids(&state, &changed);
        }
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
