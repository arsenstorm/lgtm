//! `/api/tasks/{id}/events`: the stored history, then live events until the
//! task ends.

use std::ops::ControlFlow;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::SinkExt;
use lgtm_protocol::{StoredEvent, TaskEvent};
use serde::Deserialize;
use tokio::sync::broadcast;

use super::ApiError;
use crate::state::App;

#[derive(Deserialize)]
pub(super) struct EventsQuery {
    #[serde(default)]
    from: usize,
}

pub(super) async fn events(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let (stored, live, terminal) = {
        let state = app.state.lock().unwrap();
        let rec = state
            .tasks
            .get(&id)
            .ok_or(ApiError(StatusCode::NOT_FOUND, "task not found".into()))?;
        (
            rec.events.clone(),
            rec.live.subscribe(),
            rec.task.status.is_terminal(),
        )
    };
    let from = query.from.min(stored.len());
    let stored = stored[from..].to_vec();
    Ok(ws.on_upgrade(move |socket| stream(socket, stored, live, terminal)))
}

fn is_final(event: &TaskEvent) -> bool {
    matches!(
        event,
        TaskEvent::Completed { .. }
            | TaskEvent::Conflicted { .. }
            | TaskEvent::Failed { .. }
            | TaskEvent::TimedOut { .. }
            | TaskEvent::RunnerLost
            | TaskEvent::Cancelled
    )
}

async fn send(socket: &mut WebSocket, event: &StoredEvent) -> bool {
    let Ok(text) = serde_json::to_string(event) else {
        return false;
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn stream(
    mut socket: WebSocket,
    stored: Vec<StoredEvent>,
    mut live: broadcast::Receiver<StoredEvent>,
    terminal: bool,
) {
    // A task failed by a restart has no final event on record, so the status
    // has to close the socket too.
    let mut done = terminal;
    for event in &stored {
        if !send(&mut socket, event).await {
            return;
        }
        done |= is_final(&event.event);
    }
    if done {
        let _ = socket.close().await;
        return;
    }
    let close = loop {
        tokio::select! {
            received = live.recv() => match forward(&mut socket, received).await {
                ControlFlow::Continue(()) => {}
                ControlFlow::Break(close) => break close,
            },
            // The client sends nothing; this arm only notices it going away.
            frame = socket.recv() => if !matches!(frame, Some(Ok(_))) {
                break false;
            },
        }
    };
    if close {
        let _ = socket.close().await;
    }
}

/// Sends one live event; `Break(true)` closes the socket, `Break(false)`
/// drops it because the client already went away.
async fn forward(
    socket: &mut WebSocket,
    received: Result<StoredEvent, broadcast::error::RecvError>,
) -> ControlFlow<bool> {
    let Ok(event) = received else {
        return ControlFlow::Break(true);
    };
    if !send(socket, &event).await {
        return ControlFlow::Break(false);
    }
    if is_final(&event.event) {
        return ControlFlow::Break(true);
    }
    ControlFlow::Continue(())
}
