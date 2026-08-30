//! The runner agent's socket: `Hello` authenticates it, then it streams events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use lgtm_protocol::{
    OrchestratorMessage, RunnerInfo, RunnerMessage, TaskEvent, TaskId, TaskStatus, PROTOCOL_VERSION,
};
use tokio::sync::mpsc;

const HELLO_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a runner's tasks survive its socket, so a restarting agent or a
/// flaky network does not throw away work that is still running.
const GRACE: Duration = Duration::from_secs(30);
/// A ping every `PING`, and a socket that has said nothing for `READ_TIMEOUT`
/// is gone: three missed pings, so one slow moment does not drop a runner.
const PING: Duration = Duration::from_secs(15);
const READ_TIMEOUT: Duration = Duration::from_secs(45);
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
        let changed = state.runner_hello(
            info.clone(),
            running,
            Conn {
                tx: tx.clone(),
                conn_id,
            },
        );
        app.persist_ids(&mut state, &changed);
    }

    let writer = tokio::spawn(write_all(sink, rx));
    loop {
        let Ok(frame) = tokio::time::timeout(READ_TIMEOUT, stream.next()).await else {
            tracing::warn!(runner = %info.name, "no frame in {READ_TIMEOUT:?}, dropping socket");
            break;
        };
        let text = match frame {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(_))) | None => break,
            Some(Ok(_)) => continue,
            Some(Err(_)) => break,
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
    // From now plus one period: an immediate first tick would race the
    // hello ack for the first frame on the socket.
    let start = tokio::time::Instant::now() + PING;
    let mut ping = tokio::time::interval_at(start, PING);
    loop {
        let frame = tokio::select! {
            msg = rx.recv() => match msg {
                Some(msg) => match serde_json::to_string(&msg) {
                    Ok(text) => Message::Text(text.into()),
                    Err(_) => continue,
                },
                None => break,
            },
            _ = ping.tick() => Message::Ping(Vec::new().into()),
        };
        if sink.send(frame).await.is_err() {
            break;
        }
    }
}

/// Applies one frame; `true` when the runner said goodbye and the socket is done.
fn handle(app: &Arc<App>, info: &RunnerInfo, conn_id: u64, text: &str) -> bool {
    match serde_json::from_str::<RunnerMessage>(text) {
        Ok(RunnerMessage::Event { task_id, event }) => apply(app, &task_id, event),
        // Deliberate exit: forget the runner now. `disconnect` finds nothing
        // left and so starts no grace timer.
        Ok(RunnerMessage::Goodbye) => {
            let mut state = app.state.lock().unwrap();
            let changed = state.runner_goodbye(&info.name, conn_id);
            app.persist_ids(&mut state, &changed);
            return true;
        }
        Ok(RunnerMessage::Terminal { task_id, data }) => terminal(app, &task_id, Some(data)),
        Ok(RunnerMessage::TerminalClosed { task_id }) => terminal(app, &task_id, None),
        Ok(RunnerMessage::Hello { .. }) => {}
        Err(err) => tracing::warn!(runner = %info.name, %err, "bad runner frame"),
    }
    false
}

/// Output from a task's shell, or `None` for the shell closing. Never stored:
/// terminal traffic is not task history.
fn terminal(app: &Arc<App>, task_id: &str, data: Option<String>) {
    let mut state = app.state.lock().unwrap();
    let Some(rec) = state.tasks.get_mut(task_id) else {
        return;
    };
    match data {
        Some(data) => rec.push_terminal(data),
        None => {
            let _ = rec.terminal.send(None);
        }
    }
}

async fn hello(
    app: &App,
    sink: &mut SplitSink<WebSocket, Message>,
    stream: &mut SplitStream<WebSocket>,
) -> Option<(RunnerInfo, Vec<TaskId>)> {
    let frame = tokio::time::timeout(HELLO_TIMEOUT, stream.next())
        .await
        .ok()??
        .ok()?;
    let Message::Text(text) = frame else {
        return None;
    };
    let Ok(RunnerMessage::Hello {
        token,
        info,
        running,
        version,
    }) = serde_json::from_str::<RunnerMessage>(&text)
    else {
        return None;
    };
    authorize(app, sink, &info, &token, version).await?;
    Some((info, running))
}

/// `None` when the runner was refused, having already been told why and closed.
async fn authorize(
    app: &App,
    sink: &mut SplitSink<WebSocket, Message>,
    info: &RunnerInfo,
    token: &str,
    version: u32,
) -> Option<()> {
    if token != app.token {
        tracing::warn!(runner = %info.name, "runner presented a bad token");
        let _ = sink.send(Message::Close(None)).await;
        return None;
    }
    if version != PROTOCOL_VERSION {
        tracing::warn!(runner = %info.name, %version, "runner speaks a different protocol version");
        let msg = OrchestratorMessage::Rejected {
            reason: format!(
                "protocol version {version}, this orchestrator speaks {PROTOCOL_VERSION}"
            ),
        };
        if let Ok(text) = serde_json::to_string(&msg) {
            let _ = sink.send(Message::Text(text.into())).await;
        }
        let _ = sink.send(Message::Close(None)).await;
        return None;
    }
    Some(())
}

/// Approves a completed task the policy says needs no one to look at it. The
/// `Pushed` the runner sends back is what actually moves it to `Approved`.
fn auto_approve(app: &App, state: &mut crate::state::State, task_id: &str) {
    let Some(task) = state.tasks.get(task_id).map(|rec| rec.task.clone()) else {
        return;
    };
    let Some(decision) = crate::policy::decide(&task) else {
        return;
    };
    let changed = state.apply_event(task_id, decision.event());
    app.persist_ids(state, &changed);
    if !decision.allowed {
        tracing::info!(task = %task_id, reasons = ?decision.reasons, "policy refused auto-approve");
        return;
    }
    // Asking the runner first, so a task is not marked auto-approved by an
    // event no one can act on.
    let token = app.push_token(&task);
    if let Err(err) = state.command(
        task_id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Push { task_id, token },
    ) {
        tracing::warn!(task = %task_id, ?err, "auto-approve skipped");
        return;
    }
    tracing::info!(task = %task_id, "auto-approved by policy");
    let changed = state.apply_event(task_id, TaskEvent::AutoApproved);
    app.persist_ids(state, &changed);
}

/// The webhook describes the task as the event left it, so it reads the task
/// back rather than reusing a copy from before `apply_event`.
fn deliver(app: &Arc<App>, task_id: &str, event: &TaskEvent) {
    let task = {
        let state = app.state.lock().unwrap();
        state.tasks.get(task_id).map(|rec| rec.task.clone())
    };
    if let Some(task) = task {
        crate::notify::deliver(app, &task, event);
    }
}

fn apply(app: &Arc<App>, task_id: &str, event: TaskEvent) {
    let pushed = matches!(event, TaskEvent::Pushed { .. });
    let completed = matches!(event, TaskEvent::Completed { .. });
    // Only cloned when there is somewhere to send it: an event carries the
    // whole diff.
    let for_webhook = app.webhook.is_some().then(|| event.clone());
    let ended = matches!(
        event,
        TaskEvent::Completed { .. }
            | TaskEvent::Failed { .. }
            | TaskEvent::TimedOut { .. }
            | TaskEvent::RunnerLost
    );
    let (previous, plan) = {
        let mut state = app.state.lock().unwrap();
        let previous = state.tasks.get(task_id).map(|rec| rec.task.status);
        let changed = state.apply_event(task_id, event);
        app.persist_ids(&mut state, &changed);
        if completed {
            if let Some(rec) = state.tasks.get(task_id) {
                app.warm_push_token(&rec.task);
            }
            auto_approve(app, &mut state, task_id);
            let changed = state.auto_approve_plan(task_id);
            app.persist_ids(&mut state, &changed);
        }
        let plan = pushed
            .then(|| state.pull_request_plan(task_id, app.github.is_some()))
            .flatten();
        (previous, plan)
    };
    if let Some(event) = for_webhook {
        deliver(app, task_id, &event);
    }
    if let Some(previous) = previous {
        crate::linear::after_transition(app, task_id, previous, false);
    }
    if let Some(plan) = plan {
        crate::github::open_pull_request(app.clone(), task_id.to_string(), plan);
    }
    // A task with no goal, or one policy has already moved on, is refused
    // when the decision is applied; nothing needs to be checked twice here.
    if ended && app.orchestrate.is_some() {
        tokio::spawn(crate::orchestrate::run(app.clone(), task_id.to_string()));
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
        if let Some(changed) = state.expire_runner(&name, generation) {
            app.persist_ids(&mut state, &changed);
            crate::notify::deliver_runner(&app, &name, "disconnected");
        }
    });
}
