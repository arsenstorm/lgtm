//! Authenticated HTTP + WebSocket surface under `/api`.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::SinkExt;
use lgtm_protocol::{
    Batch, BatchSource, BatchSummary, Executor, OrchestratorMessage, StoredEvent, Task, TaskEvent,
    TaskId, TaskKind, TaskSpec, TaskStatus, WorkerStatus,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::backlog::{self, Candidate, SpecInput};
use crate::persist::Stored;
use crate::state::{now_ms, App, CmdError};

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
        .route("/tasks/{id}/events", get(events))
        .route("/tasks/{id}/message", post(message))
        .route("/tasks/{id}/cancel", post(cancel))
        .route("/tasks/{id}/approve", post(approve))
        .route("/tasks/{id}/reject", post(reject))
        .route("/batches", get(list_batches).post(create_batch))
        .route("/batches/{id}", get(get_batch))
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

fn github(app: &App) -> Result<lgtm_github::GitHub, ApiError> {
    app.github
        .clone()
        .ok_or_else(|| conflict("GITHUB_TOKEN is not configured".into()))
}

fn bad_gateway(err: anyhow::Error) -> ApiError {
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

fn linear(app: &App) -> Result<lgtm_linear::Linear, ApiError> {
    app.linear
        .clone()
        .ok_or_else(|| conflict("LINEAR_API_KEY is not configured".into()))
}

fn bad_linear(err: anyhow::Error) -> ApiError {
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

fn twenty() -> u32 {
    20
}

/// Body of `POST /api/batches`.
#[derive(Deserialize)]
struct BatchRequest {
    source: BatchSource,
    /// Git URL the tasks clone from. Optional for GitHub, where the label's
    /// repository is the obvious default; required for Linear.
    #[serde(default)]
    repository: Option<String>,
    base_branch: String,
    executor: Executor,
    #[serde(default)]
    worker: Option<String>,
    /// Import each issue as a plan task instead of a run.
    #[serde(default)]
    plan: bool,
    /// Approve this batch's plans without a person.
    #[serde(default)]
    approve_plans: bool,
    #[serde(default = "twenty")]
    max: u32,
    /// Report what would be imported and create nothing.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Serialize)]
struct IssuePreview {
    key: String,
    title: String,
    url: String,
}

#[derive(Serialize)]
struct BatchResponse {
    batch: Option<Batch>,
    issues: Vec<IssuePreview>,
}

#[derive(Serialize)]
struct BatchDetail {
    batch: Batch,
    summary: BatchSummary,
    tasks: Vec<Task>,
}

/// The issues a source returned, still to be turned into candidates: the batch
/// id they carry is only known once the lock is held.
enum Fetched {
    Github(lgtm_github::Repo, Vec<lgtm_github::Issue>),
    Linear(Vec<lgtm_linear::Issue>),
}

/// Talks to the issue tracker, off the state lock. Returns the repository the
/// tasks will clone from alongside what it found.
async fn fetch_batch(app: &App, body: &BatchRequest) -> Result<(String, Fetched), ApiError> {
    match &body.source {
        BatchSource::GithubLabel { owner, repo, label } => {
            let github = github(app)?;
            let repo = lgtm_github::Repo {
                owner: owner.clone(),
                repo: repo.clone(),
            };
            let issues = github
                .issues_with_label(&repo, label)
                .await
                .map_err(bad_gateway)?;
            let repository = body
                .repository
                .clone()
                .unwrap_or_else(|| format!("https://github.com/{}/{}.git", repo.owner, repo.repo));
            Ok((repository, Fetched::Github(repo, issues)))
        }
        BatchSource::Linear { team, state } => {
            let linear = linear(app)?;
            let repository = body.repository.clone().ok_or_else(|| {
                ApiError(
                    StatusCode::BAD_REQUEST,
                    "repository is required for a linear batch".into(),
                )
            })?;
            let issues = linear
                .issues_in_state(team, state)
                .await
                .map_err(bad_linear)?;
            Ok((repository, Fetched::Linear(issues)))
        }
    }
}

async fn create_batch(
    State(app): State<Arc<App>>,
    body: Result<Json<BatchRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<BatchResponse>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let (repository, fetched) = fetch_batch(&app, &body).await?;

    let mut state = app.state.lock().unwrap();
    let id = state.new_batch_id();
    let input = SpecInput {
        base_branch: body.base_branch.clone(),
        executor: body.executor,
        worker: body.worker.clone(),
        kind: if body.plan {
            TaskKind::Plan
        } else {
            TaskKind::Run
        },
        batch: Some(id.clone()),
    };
    let candidates: Vec<Candidate> = match &fetched {
        Fetched::Github(repo, issues) => issues
            .iter()
            .map(|issue| backlog::github_candidate(issue, repo, input.clone()))
            .collect(),
        Fetched::Linear(issues) => issues
            .iter()
            .map(|issue| backlog::linear_candidate(issue, &repository, input.clone()))
            .collect(),
    };
    // ponytail: copies every task to compare against; an index by issue
    // reference is the upgrade if this ever shows up in a profile.
    let existing: Vec<Task> = state.tasks.values().map(|rec| rec.task.clone()).collect();
    let selected = backlog::select(&existing, candidates, body.max);
    let issues: Vec<IssuePreview> = selected
        .iter()
        .map(|candidate| IssuePreview {
            key: candidate.key.clone(),
            title: candidate.title.clone(),
            url: candidate.url.clone(),
        })
        .collect();
    if body.dry_run {
        return Ok((
            StatusCode::OK,
            Json(BatchResponse {
                batch: None,
                issues,
            }),
        ));
    }

    // Every candidate shares executor and worker, so one refusal would hold
    // for all of them; check once before anything is created.
    if let Some(first) = selected.first() {
        state.check_eligible(&first.spec).map_err(conflict)?;
    }
    let mut task_ids: Vec<TaskId> = Vec::new();
    let mut changed: Vec<TaskId> = Vec::new();
    let mut refused = None;
    for candidate in selected {
        match state.create_task(candidate.spec) {
            Ok((task, ids)) => {
                task_ids.push(task.id);
                changed.extend(ids);
            }
            // Whatever made this one ineligible holds for the rest, so stop
            // here. The tasks already created keep their place in the queue.
            Err(err) => {
                refused = Some(err);
                break;
            }
        }
    }
    if let Some(err) = refused {
        app.persist_ids(&state, &changed);
        return Err(conflict(err));
    }
    let batch = Batch {
        id: id.clone(),
        created_at: now_ms(),
        source: body.source,
        repository,
        task_ids,
        approve_plans: body.approve_plans,
    };
    tracing::info!(batch = %id, tasks = batch.task_ids.len(), "batch imported");
    state.batches.insert(id, batch.clone());
    app.persist_batch(&batch);
    app.persist_ids(&state, &changed);
    Ok((
        StatusCode::CREATED,
        Json(BatchResponse {
            batch: Some(batch),
            issues,
        }),
    ))
}

async fn list_batches(State(app): State<Arc<App>>) -> Json<Vec<Batch>> {
    let state = app.state.lock().unwrap();
    let mut batches: Vec<Batch> = state.batches.values().cloned().collect();
    batches.sort_by_key(|batch| batch.created_at);
    Json(batches)
}

async fn get_batch(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<BatchDetail>, ApiError> {
    let state = app.state.lock().unwrap();
    let batch = state
        .batches
        .get(&id)
        .cloned()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "batch not found".into()))?;
    // The batch's own ids are the plan tasks; the children a plan created
    // carry the same batch, so membership is what the task says it is.
    let mut tasks: Vec<&Task> = state
        .tasks
        .values()
        .map(|rec| &rec.task)
        .filter(|task| task.spec.batch.as_deref() == Some(id.as_str()))
        .collect();
    tasks.sort_by_key(|task| task.created_at);
    let summary = backlog::summary(&tasks, &state);
    Ok(Json(BatchDetail {
        batch,
        summary,
        tasks: tasks.into_iter().cloned().collect(),
    }))
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
    {
        // Approving a plan creates its steps here; there is nothing to push.
        let mut state = app.state.lock().unwrap();
        let is_plan = state
            .tasks
            .get(&id)
            .is_some_and(|rec| rec.task.spec.kind == TaskKind::Plan);
        if is_plan {
            let (task, changed) = state.approve_plan(&id)?;
            app.persist_ids(&state, &changed);
            return Ok(Json(task));
        }
    }
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

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    from: usize,
}

async fn events(
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
        TaskEvent::Completed { .. } | TaskEvent::Failed { .. } | TaskEvent::Cancelled
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
