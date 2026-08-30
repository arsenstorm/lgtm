//! Authenticated HTTP + WebSocket surface under `/api`.

mod batches;
mod events;
mod goals;
mod memories;
mod terminal;
mod todos;

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use lgtm_protocol::{
    overlaps, plan_versions, Executor, OrchestratorMessage, PlanVersion, SandboxProfile, Stats,
    Task, TaskEvent, TaskKind, TaskSpec, TaskStatus, WorkerStatus,
};
use serde::Deserialize;

use crate::backlog::{self, SpecInput};
use crate::commands::RetryInto;
use crate::persist::Stored;
use crate::state::{now_ms, App, CmdError};
use crate::stats;

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
        .route("/stats", get(stats))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/from-issue", post(create_task_from_issue))
        .route("/tasks/from-linear", post(create_task_from_linear))
        .route("/tasks/{id}", get(get_task))
        .route("/tasks/{id}/merge", post(merge))
        .route("/tasks/{id}/events", get(events::events))
        .route(
            "/tasks/{id}/terminal",
            get(terminal::attach).delete(terminal::close),
        )
        .route("/tasks/{id}/plans", get(get_task_plans))
        .route("/tasks/{id}/message", post(message))
        .route("/tasks/{id}/retry", post(retry))
        .route("/tasks/{id}/allow", post(allow))
        .route("/tasks/{id}/permissions", post(request_permission))
        .route("/tasks/{id}/scratchpad", post(scratchpad))
        .route("/tasks/{id}/cancel", post(cancel))
        .route("/tasks/{id}/approve", post(approve))
        .route("/tasks/{id}/reject", post(reject))
        .route(
            "/batches",
            get(batches::list_batches).post(batches::create_batch),
        )
        .route("/batches/{id}", get(batches::get_batch))
        .route(
            "/memories",
            get(memories::list_memories).post(memories::create_memory),
        )
        .route("/memories/{id}", delete(memories::delete_memory))
        .route("/goals", get(goals::list_goals).post(goals::create_goal))
        .route("/goals/{id}", get(goals::get_goal))
        .route("/todos", get(todos::list_todos).post(todos::create_todo))
        .route("/todos/{id}", delete(todos::delete_todo))
        .route("/todos/{id}/done", post(todos::finish_todo))
        .route("/todos/{id}/promote", post(todos::promote_todo))
        .route("/goals/{id}/plans", get(goals::get_goal_plans))
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

/// A week is the default window: long enough to trust, short enough to load
/// without paging.
const DEFAULT_STATS_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Deserialize)]
struct StatsQuery {
    #[serde(default)]
    since: Option<u64>,
}

async fn stats(State(app): State<Arc<App>>, Query(query): Query<StatsQuery>) -> Json<Stats> {
    let state = app.state.lock().unwrap();
    let since = query
        .since
        .unwrap_or_else(|| now_ms().saturating_sub(DEFAULT_STATS_WINDOW_MS));
    let records: Vec<(&Task, &[lgtm_protocol::StoredEvent])> = state
        .tasks
        .values()
        .map(|rec| (&rec.task, rec.events.as_slice()))
        .collect();
    Json(stats::compute(&records, since))
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
    app.persist_ids(&mut state, &changed);
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
    #[serde(default)]
    sandbox: Option<SandboxProfile>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    review_executor: Option<Executor>,
    model: Option<String>,
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
        sandbox: body.sandbox,
        requirements: body.requirements,
        review_executor: body.review_executor,
        model: body.model,
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
    #[serde(default)]
    sandbox: Option<SandboxProfile>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    review_executor: Option<Executor>,
    model: Option<String>,
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
        sandbox: body.sandbox,
        requirements: body.requirements,
        review_executor: body.review_executor,
        model: body.model,
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
    app.persist_ids(&mut state, &changed);
    Ok(Json(task))
}

#[derive(Deserialize)]
struct RetryBody {
    #[serde(default)]
    worker: Option<String>,
    #[serde(default)]
    executor: Option<Executor>,
}

async fn retry(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<RetryBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let (task, changed) = state.retry(
        &id,
        RetryInto {
            worker: body.worker,
            executor: body.executor,
        },
    )?;
    app.persist_ids(&mut state, &changed);
    Ok(Json(task))
}

#[derive(Deserialize)]
struct AllowBody {
    host: String,
}

/// A person granting `host` for this task's next run.
async fn allow(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<AllowBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let host = valid_host(&body.host)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, "invalid host".into()))?;
    let mut state = app.state.lock().unwrap();
    let (task, changed) = state.allow_host(&id, host)?;
    app.persist_ids(&mut state, &changed);
    Ok(Json(task))
}

/// A host is non-empty, has no whitespace, and names no scheme: the proxy
/// matches it against the bare host it sees on the connection, nothing else.
fn valid_host(host: &str) -> Option<String> {
    let host = host.trim();
    let ok = !host.is_empty() && !host.contains(char::is_whitespace) && !host.contains("://");
    ok.then(|| host.to_string())
}

#[derive(Deserialize)]
struct PermissionBody {
    kind: String,
    target: String,
    reason: String,
}

/// What the MCP tool calls when the agent hits a sandbox refusal it wants a
/// person to lift. Recorded for `pending_requests`; nothing else acts on it.
async fn request_permission(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<PermissionBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    if !state.tasks.contains_key(&id) {
        return Err(CmdError::NotFound.into());
    }
    let event = TaskEvent::PermissionRequested {
        kind: body.kind,
        target: body.target,
        reason: body.reason,
    };
    let changed = state.apply_event(&id, event);
    app.persist_ids(&mut state, &changed);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ScratchpadBody {
    content: String,
}

/// A person rewriting the notes the agent kept; reading them is `GET /tasks/:id`.
async fn scratchpad(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<ScratchpadBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    if !state.tasks.contains_key(&id) {
        return Err(CmdError::NotFound.into());
    }
    let event = TaskEvent::Scratchpad {
        content: body.content,
    };
    let changed = state.apply_event(&id, event);
    let task = state.tasks.get(&id).ok_or(CmdError::NotFound)?.task.clone();
    app.persist_ids(&mut state, &changed);
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
    let others: Vec<&Task> = state.tasks.values().map(|rec| &rec.task).collect();
    Ok(Json(Stored {
        overlaps: overlaps(&rec.task, &others),
        ..Stored::from(rec)
    }))
}

async fn get_task_plans(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PlanVersion>>, ApiError> {
    let state = app.state.lock().unwrap();
    let rec = state
        .tasks
        .get(&id)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "task not found".into()))?;
    Ok(Json(plan_versions(&rec.task, &rec.events)))
}

async fn cancel(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let task = state.cancel(&id)?;
    app.persist_ids(&mut state, std::slice::from_ref(&id));
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
        app.persist_ids(&mut state, &changed);
        return Ok(Json(task));
    }
    let token = state
        .tasks
        .get(&id)
        .and_then(|rec| app.push_token(&rec.task));
    let task = state.command(
        &id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Push { task_id, token },
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
