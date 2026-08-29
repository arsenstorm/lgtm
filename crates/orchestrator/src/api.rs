//! Authenticated HTTP + WebSocket surface under `/api`.

mod batches;
mod events;

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use lgtm_protocol::{
    Executor, OrchestratorMessage, Task, TaskKind, TaskSpec, TaskStatus, WorkerStatus,
};
use serde::Deserialize;

use crate::backlog::{self, SpecInput};
use crate::persist::Stored;
use crate::state::{App, CmdError};

pub(super) struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

pub(super) fn conflict(msg: String) -> ApiError {
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

impl From<crate::github::MergeError> for ApiError {
    fn from(err: crate::github::MergeError) -> Self {
        match err {
            crate::github::MergeError::Cmd(err) => err.into(),
            crate::github::MergeError::Github(err) => bad_gateway(err),
        }
    }
}

pub fn router(app: Arc<App>) -> Router<Arc<App>> {
    Router::new()
        .route("/workers", get(workers))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/from-issue", post(create_task_from_issue))
        .route("/tasks/from-linear", post(create_task_from_linear))
        .route("/tasks/{id}", get(get_task))
        .route("/tasks/{id}/merge", post(merge))
        .route("/tasks/{id}/events", get(events::events))
        .route("/tasks/{id}/message", post(message))
        .route("/tasks/{id}/cancel", post(cancel))
        .route("/tasks/{id}/approve", post(approve))
        .route("/tasks/{id}/reject", post(reject))
        .route(
            "/batches",
            get(batches::list_batches).post(batches::create_batch),
        )
        .route("/batches/{id}", get(batches::get_batch))
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
        .filter(|conn| conn.is_connected())
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
    queue(&app, spec)
}

fn queue(app: &App, spec: TaskSpec) -> Result<(StatusCode, Json<Task>), ApiError> {
    let mut state = app.state.lock().unwrap();
    let (task, changed) = state.create_task(spec).map_err(conflict)?;
    app.persist_ids(&state, &changed);
    Ok((StatusCode::CREATED, Json(task)))
}

pub(super) fn github(app: &App) -> Result<lgtm_github::GitHub, ApiError> {
    app.github
        .clone()
        .ok_or_else(|| conflict("GITHUB_TOKEN is not configured".into()))
}

pub(super) fn bad_gateway(err: anyhow::Error) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, format!("github: {err:#}"))
}

#[derive(Deserialize)]
struct FromIssueBody {
    issue: String,
    base_branch: String,
    executor: Executor,
    #[serde(default)]
    worker: Option<String>,
}

async fn create_task_from_issue(
    State(app): State<Arc<App>>,
    body: Result<Json<FromIssueBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let github = github(&app)?;
    let (repo, number) = lgtm_github::parse_issue(&body.issue).ok_or_else(|| {
        ApiError(
            StatusCode::BAD_REQUEST,
            format!("unrecognised issue: {}", body.issue),
        )
    })?;
    let issue = github.issue(&repo, number).await.map_err(bad_gateway)?;
    let input = SpecInput {
        base_branch: body.base_branch,
        executor: body.executor,
        worker: body.worker,
        kind: TaskKind::Run,
        batch: None,
    };
    queue(&app, backlog::github_candidate(&issue, &repo, input).spec)
}

pub(super) fn linear(app: &App) -> Result<lgtm_linear::Linear, ApiError> {
    app.linear
        .clone()
        .ok_or_else(|| conflict("LINEAR_API_KEY is not configured".into()))
}

pub(super) fn bad_linear(err: anyhow::Error) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, format!("linear: {err:#}"))
}

#[derive(Deserialize)]
struct FromLinearBody {
    issue: String,
    /// Linear knows nothing about repositories, so the caller names one.
    repository: String,
    base_branch: String,
    executor: Executor,
    #[serde(default)]
    worker: Option<String>,
}

async fn create_task_from_linear(
    State(app): State<Arc<App>>,
    body: Result<Json<FromLinearBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let linear = linear(&app)?;
    let identifier = lgtm_linear::parse_issue(&body.issue).ok_or_else(|| {
        ApiError(
            StatusCode::BAD_REQUEST,
            format!("unrecognised linear issue: {}", body.issue),
        )
    })?;
    let issue = linear.issue(&identifier).await.map_err(bad_linear)?;
    let input = SpecInput {
        base_branch: body.base_branch,
        executor: body.executor,
        worker: body.worker,
        kind: TaskKind::Run,
        batch: None,
    };
    queue(
        &app,
        backlog::linear_candidate(&issue, &body.repository, input).spec,
    )
}

async fn merge(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    Ok(Json(crate::github::merge_task(&app, &id).await?))
}

#[derive(Deserialize)]
struct MessageBody {
    text: String,
}

async fn message(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<MessageBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let (task, changed) = state.message(&id, body.text)?;
    app.persist_ids(&state, &changed);
    Ok(Json(task))
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
    let mut state = app.state.lock().unwrap();
    let task = state.cancel(&id)?;
    app.persist_ids(&state, std::slice::from_ref(&id));
    Ok(Json(task))
}

async fn approve(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let is_plan = state
        .tasks
        .get(&id)
        .is_some_and(|rec| rec.task.spec.kind == TaskKind::Plan);
    // Approving a plan creates its steps here; there is nothing to push.
    if is_plan {
        let (task, changed) = state.approve_plan(&id)?;
        app.persist_ids(&state, &changed);
        return Ok(Json(task));
    }
    let task = state.command(
        &id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Push { task_id },
    )?;
    Ok(Json(task))
}

async fn reject(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let task = state.command(
        &id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Discard { task_id },
    )?;
    Ok(Json(task))
}
