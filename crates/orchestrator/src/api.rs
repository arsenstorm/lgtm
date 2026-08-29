//! Authenticated HTTP + WebSocket surface under `/api`.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lgtm_protocol::{
    OrchestratorMessage, StoredEvent, Task, TaskEvent, TaskSpec, TaskStatus, WorkerStatus,
};
use tokio::sync::broadcast;

use crate::persist::Stored;
use crate::state::{App, CmdError};

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn conflict(msg: String) -> ApiError {
    ApiError(StatusCode::CONFLICT, msg)
}

impl From<CmdError> for ApiError {
    fn from(err: CmdError) -> Self {
        match err {
            CmdError::NotFound => ApiError(StatusCode::NOT_FOUND, "task not found".into()),
            CmdError::Conflict(msg) => conflict(msg),
        }
    }
}

pub fn router(app: Arc<App>) -> Router<Arc<App>> {
    Router::new()
        .route("/workers", get(workers))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/:id", get(get_task))
        .route("/tasks/:id/events", get(events))
        .route("/tasks/:id/cancel", post(cancel))
        .route("/tasks/:id/approve", post(approve))
        .route("/tasks/:id/reject", post(reject))
        .layer(middleware::from_fn_with_state(app, auth))
}

async fn auth(State(app): State<Arc<App>>, req: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", app.token);
    let ok = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);
    if ok {
        next.run(req).await
    } else {
        ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into()).into_response()
    }
}

async fn workers(State(app): State<Arc<App>>) -> Json<Vec<WorkerStatus>> {
    let state = app.state.lock().unwrap();
    let mut out: Vec<WorkerStatus> = state
        .workers
        .values()
        .map(|conn| WorkerStatus {
            info: conn.info.clone(),
            running: conn.running.iter().cloned().collect(),
        })
        .collect();
    out.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    Json(out)
}

async fn create_task(
    State(app): State<Arc<App>>,
    body: Result<Json<TaskSpec>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let Json(spec) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let task = state.create_task(spec).map_err(conflict)?;
    if let Some(rec) = state.tasks.get(&task.id) {
        let _ = app.persist.send(Stored::from(rec));
    }
    Ok((StatusCode::CREATED, Json(task)))
}

async fn list_tasks(State(app): State<Arc<App>>) -> Json<Vec<Task>> {
    let state = app.state.lock().unwrap();
    let mut tasks: Vec<Task> = state.tasks.values().map(|rec| rec.task.clone()).collect();
    tasks.sort_by_key(|task| task.created_at);
    Json(tasks)
}

async fn get_task(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Stored>, ApiError> {
    let state = app.state.lock().unwrap();
    let rec = state
        .tasks
        .get(&id)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "task not found".into()))?;
    Ok(Json(Stored {
        task: rec.task.clone(),
        events: rec.events.clone(),
    }))
}

async fn cancel(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    command(
        &app,
        &id,
        &[TaskStatus::Queued, TaskStatus::Running],
        "task is not running",
        |task_id| OrchestratorMessage::Cancel { task_id },
    )
}

async fn approve(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    command(
        &app,
        &id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Push { task_id },
    )
}

async fn reject(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    command(
        &app,
        &id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Discard { task_id },
    )
}

fn command(
    app: &App,
    id: &str,
    allowed: &[TaskStatus],
    wrong_status: &str,
    msg: impl FnOnce(String) -> OrchestratorMessage,
) -> Result<Json<Task>, ApiError> {
    let mut state = app.state.lock().unwrap();
    Ok(Json(state.command(id, allowed, wrong_status, msg)?))
}

async fn events(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
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
    Ok(ws.on_upgrade(move |socket| stream(socket, stored, live, terminal)))
}

fn is_final(event: &TaskEvent) -> bool {
    matches!(
        event,
        TaskEvent::Completed { .. } | TaskEvent::Failed { .. } | TaskEvent::Cancelled
    )
}

async fn send(socket: &mut WebSocket, event: &StoredEvent) -> bool {
    let Ok(text) = serde_json::to_string(event) else {
        return false;
    };
    socket.send(Message::Text(text)).await.is_ok()
}

async fn stream(
    mut socket: WebSocket,
    stored: Vec<StoredEvent>,
    mut live: broadcast::Receiver<StoredEvent>,
    terminal: bool,
) {
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
    loop {
        tokio::select! {
            received = live.recv() => {
                let Ok(event) = received else { break };
                if !send(&mut socket, &event).await {
                    return;
                }
                if is_final(&event.event) {
                    break;
                }
            }
            // The client sends nothing; this arm only notices it going away.
            frame = socket.recv() => match frame {
                Some(Ok(_)) => {}
                _ => return,
            },
        }
    }
    let _ = socket.close().await;
}
