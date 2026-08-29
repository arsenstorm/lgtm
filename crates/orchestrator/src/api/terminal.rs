//! `/api/tasks/{id}/terminal`: a WebSocket onto the shell the task's runner
//! keeps in its worktree.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::SinkExt;
use lgtm_protocol::OrchestratorMessage;
use tokio::sync::broadcast;

use super::{conflict, ApiError};
use crate::state::{App, State as TaskState};

/// Attaches to the task's shell, starting one if the runner has none. Any
/// status is fine: poking at a failed task's worktree is the point.
pub(super) async fn attach(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let (output, scrollback) = {
        let state = app.state.lock().unwrap();
        to_runner(
            &state,
            &id,
            OrchestratorMessage::TerminalOpen {
                task_id: id.clone(),
            },
        )?;
        let rec = state.tasks.get(&id).ok_or_else(not_found)?;
        (rec.terminal.subscribe(), rec.scrollback())
    };
    Ok(ws.on_upgrade(move |socket| stream(socket, app, id, output, scrollback)))
}

/// Kills the task's shell. Detaching does not: the shell is meant to survive.
pub(super) async fn close(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let state = app.state.lock().unwrap();
    to_runner(
        &state,
        &id,
        OrchestratorMessage::TerminalClose {
            task_id: id.clone(),
        },
    )?;
    Ok(StatusCode::NO_CONTENT)
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "task not found".into())
}

fn to_runner(state: &TaskState, id: &str, msg: OrchestratorMessage) -> Result<(), ApiError> {
    let rec = state.tasks.get(id).ok_or_else(not_found)?;
    let name = rec.task.runner.clone().unwrap_or_default();
    let runner = state
        .runners
        .get(&name)
        .filter(|runner| runner.is_connected())
        .ok_or_else(|| conflict(format!("runner {name} is not connected")))?;
    runner.send(msg);
    Ok(())
}

async fn stream(
    mut socket: WebSocket,
    app: Arc<App>,
    id: String,
    mut output: broadcast::Receiver<Option<String>>,
    scrollback: String,
) {
    if !scrollback.is_empty() && socket.send(Message::Text(scrollback.into())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            received = output.recv() => match received {
                Ok(Some(data)) => if socket.send(Message::Text(data.into())).await.is_err() {
                    return;
                },
                // The shell closed, or this client fell so far behind that
                // the rest would be nonsense anyway.
                _ => break,
            },
            frame = socket.recv() => match frame {
                Some(Ok(Message::Text(text))) => input(&app, &id, text.to_string()),
                Some(Ok(_)) => {}
                // The client detached; the shell keeps running.
                _ => return,
            },
        }
    }
    let _ = socket.close().await;
}

fn input(app: &App, id: &str, data: String) {
    let state = app.state.lock().unwrap();
    let msg = OrchestratorMessage::TerminalInput {
        task_id: id.to_string(),
        data,
    };
    if let Err(err) = to_runner(&state, id, msg) {
        tracing::warn!(task = %id, "terminal input dropped: {}", err.1);
    }
}
