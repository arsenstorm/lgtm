//! Authenticated HTTP + WebSocket surface under `/api`.

mod artefacts;
mod batches;
mod chats;
mod credentials;
mod events;
mod goals;
mod memories;
mod projects;
mod scratchpads;
mod skills;
mod terminal;
mod todos;
mod users;
mod workspace;

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, Request, State};
use axum::http::{header::AUTHORIZATION, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Extension, Json, Router};
use lgtm_protocol::{
    overlaps, plan_versions, Executor, OrchestratorMessage, PlanVersion, ReasoningEffort,
    RunnerStatus, SandboxProfile, Stats, Task, TaskEvent, TaskKind, TaskSpec, TaskStatus,
};
use serde::Deserialize;

use crate::backlog::{self, SpecInput};
use crate::commands::RetryInto;
use crate::persist::Stored;
use crate::state::{now_ms, App, CmdError};
use crate::stats;

#[derive(Debug)]
pub(super) struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

pub(super) fn conflict(msg: String) -> ApiError {
    ApiError(StatusCode::CONFLICT, msg)
}

/// Tags as they are stored, or a 400 naming the limit the request broke.
pub(super) fn tags(tags: Vec<String>) -> Result<Vec<String>, ApiError> {
    lgtm_protocol::normalize_tags(tags).map_err(|err| ApiError(StatusCode::BAD_REQUEST, err))
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
        .route("/runners", get(runners))
        // Kept one release so an old CLI/desktop build still finds it.
        .route("/workers", get(runners))
        .route("/stats", get(stats))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/from-issue", post(create_task_from_issue))
        .route("/tasks/from-linear", post(create_task_from_linear))
        .route("/tasks/{id}", get(get_task).patch(update_task))
        .route("/tasks/{id}/merge", post(merge))
        .route("/tasks/{id}/events", get(events::events))
        .route(
            "/tasks/{id}/terminal",
            get(terminal::attach).delete(terminal::close),
        )
        .route("/tasks/{id}/artefacts", get(artefacts::list))
        .route("/tasks/{id}/artefacts/{name}", get(artefacts::get))
        .route("/tasks/{id}/plans", get(get_task_plans))
        .route("/tasks/{id}/message", post(message))
        .route("/tasks/{id}/retry", post(retry))
        .route("/tasks/{id}/allow", post(allow))
        .route("/tasks/{id}/permissions", post(request_permission))
        .route("/tasks/{id}/scratchpad", post(scratchpad))
        .route("/tasks/{id}/orchestrated", post(orchestrated))
        .route("/tasks/{id}/cancel", post(cancel))
        .route("/tasks/{id}/interrupt", post(interrupt))
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
        .route(
            "/memories/{id}",
            delete(memories::delete_memory).patch(memories::update_memory),
        )
        .route("/memories/{id}/approve", post(memories::approve_memory))
        .route(
            "/skills",
            get(skills::list_skills).post(skills::create_skill),
        )
        .route(
            "/skills/{id}",
            delete(skills::delete_skill).patch(skills::update_skill),
        )
        .route("/skills/{id}/approve", post(skills::approve_skill))
        .route("/goals", get(goals::list_goals).post(goals::create_goal))
        .route("/goals/{id}", get(goals::get_goal))
        .route("/goals/{id}/attention", post(goals::set_attention))
        .route("/todos", get(todos::list_todos).post(todos::create_todo))
        .route(
            "/todos/{id}",
            get(todos::get_todo)
                .delete(todos::delete_todo)
                .patch(todos::update_todo),
        )
        .route("/todos/{id}/comments", post(todos::create_comment))
        .route("/todos/{id}/done", post(todos::finish_todo))
        .route("/todos/{id}/promote", post(todos::promote_todo))
        .route(
            "/scratchpads",
            get(scratchpads::list_scratchpads).post(scratchpads::create_scratchpad),
        )
        .route(
            "/scratchpads/{id}",
            get(scratchpads::get_scratchpad)
                .patch(scratchpads::update_scratchpad)
                .delete(scratchpads::delete_scratchpad),
        )
        .route(
            "/projects",
            get(projects::list_projects).post(projects::create_project),
        )
        .route("/projects/{id}", patch(projects::update_project))
        .route("/goals/{id}/plans", get(goals::get_goal_plans))
        .route("/chats", get(chats::list_chats).post(chats::create_chat))
        .route(
            "/chats/{id}",
            get(chats::get_chat).patch(chats::update_chat),
        )
        .route("/chats/{id}/ask", post(chats::ask_chat))
        .route("/provenance/{sha}", get(provenance))
        .route("/users", get(users::list_users).post(users::create_user))
        .route(
            "/credentials",
            get(credentials::list).post(credentials::create),
        )
        .route("/credentials/{id}", delete(credentials::remove))
        .route(
            "/workspace",
            get(credentials::settings).post(credentials::set_settings),
        )
        .route("/users/{id}/revoke", post(users::revoke_user))
        .route("/activity", get(workspace::activity))
        .route("/ask", post(workspace::ask))
        .route("/enhance", post(workspace::enhance))
        .layer(middleware::from_fn_with_state(app, auth))
}

/// Who an authenticated request came from: a user id, or `None` for the
/// shared token (runners, automation, pre-login installs).
#[derive(Clone)]
pub(super) struct AuthedUser(pub Option<String>);

async fn auth(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    let bearer = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string);
    let user = bearer.and_then(|bearer| {
        if bearer == app.token {
            return Some(AuthedUser(None));
        }
        let state = app.state.lock().unwrap();
        state
            .user_for_token(&bearer)
            .map(|user| AuthedUser(Some(user.id.clone())))
    });
    match user {
        Some(user) => {
            req.extensions_mut().insert(user);
            next.run(req).await
        }
        None => ApiError(StatusCode::UNAUTHORIZED, "unauthorized".into()).into_response(),
    }
}

async fn runners(State(app): State<Arc<App>>) -> Json<Vec<RunnerStatus>> {
    let state = app.state.lock().unwrap();
    let mut out: Vec<RunnerStatus> = state
        .runners
        .values()
        .filter(|conn| conn.is_connected())
        .map(|conn| RunnerStatus {
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
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<TaskSpec>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let Json(spec) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    queue(&app, spec, user)
}

fn queue(
    app: &Arc<App>,
    mut spec: TaskSpec,
    user: AuthedUser,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    // Stamped here, never taken from the body: identity comes from the token.
    spec.created_by = user.0;
    let task = {
        let mut state = app.state.lock().unwrap();
        let (task, changed) = state.create_task(spec).map_err(conflict)?;
        app.persist_ids(&mut state, &changed);
        task
    };
    workspace::spawn_title(app.clone(), task.id.clone(), task.spec.prompt.clone());
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
    #[serde(default, alias = "worker")]
    runner: Option<String>,
    #[serde(default)]
    sandbox: Option<SandboxProfile>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    review_executor: Option<Executor>,
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<ReasoningEffort>,
}

async fn create_task_from_issue(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
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
        runner: body.runner,
        kind: TaskKind::Run,
        batch: None,
        sandbox: body.sandbox,
        requirements: body.requirements,
        review_executor: body.review_executor,
        model: body.model,
        reasoning_effort: body.reasoning_effort,
    };
    queue(
        &app,
        backlog::github_candidate(&issue, &repo, input).spec,
        user,
    )
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
    #[serde(default, alias = "worker")]
    runner: Option<String>,
    #[serde(default)]
    sandbox: Option<SandboxProfile>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    review_executor: Option<Executor>,
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<ReasoningEffort>,
}

async fn create_task_from_linear(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
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
        runner: body.runner,
        kind: TaskKind::Run,
        batch: None,
        sandbox: body.sandbox,
        requirements: body.requirements,
        review_executor: body.review_executor,
        model: body.model,
        reasoning_effort: body.reasoning_effort,
    };
    queue(
        &app,
        backlog::linear_candidate(&issue, &body.repository, input).spec,
        user,
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
    headers: axum::http::HeaderMap,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<MessageBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    goal_scoped(
        &headers,
        &state.tasks.get(&id).ok_or(CmdError::NotFound)?.task,
    )?;
    let (task, changed) = state.message(&id, body.text, user.0)?;
    app.persist_ids(&mut state, &changed);
    Ok(Json(task))
}

#[derive(Deserialize)]
struct RetryBody {
    #[serde(default, alias = "worker")]
    runner: Option<String>,
    #[serde(default)]
    executor: Option<Executor>,
}

async fn retry(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    body: Result<Json<RetryBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    goal_scoped(
        &headers,
        &state.tasks.get(&id).ok_or(CmdError::NotFound)?.task,
    )?;
    let (task, changed) = state.retry(
        &id,
        RetryInto {
            runner: body.runner,
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
struct OrchestratedBody {
    action: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    applied: bool,
    #[serde(default)]
    note: String,
}

/// One step of the orchestration loop, on the event log of the task whose end
/// started it. It records; it changes nothing.
async fn orchestrated(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<OrchestratedBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    if !state.tasks.contains_key(&id) {
        return Err(CmdError::NotFound.into());
    }
    tracing::info!(task = %id, action = %body.action, applied = body.applied, "orchestrator step");
    let changed = state.apply_event(
        &id,
        TaskEvent::Orchestrated {
            action: body.action,
            reason: body.reason,
            applied: body.applied,
            note: body.note,
        },
    );
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

fn last_activity(rec: &crate::state::TaskRecord) -> u64 {
    rec.events
        .last()
        .map_or(rec.task.created_at, |event| event.at)
}

async fn list_tasks(State(app): State<Arc<App>>) -> Json<Vec<Task>> {
    let state = app.state.lock().unwrap();
    // Most recently touched first: a list you scan for what just changed.
    let mut recent: Vec<(u64, Task)> = state
        .tasks
        .values()
        .filter(|rec| state.in_workspace(rec.task.workspace.as_deref()))
        .map(|rec| (last_activity(rec), rec.task.clone()))
        .collect();
    recent.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    Json(recent.into_iter().map(|(_, task)| task).collect())
}

/// Body of `PATCH /api/tasks/:id`: whichever fields are being changed.
#[derive(Deserialize)]
struct UpdateTaskBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
}

async fn update_task(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<UpdateTaskBody>, JsonRejection>,
) -> Result<Json<Task>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let title = body.title.map(|title| title.trim().to_string());
    if title.as_deref() == Some("") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "title cannot be empty".into(),
        ));
    }
    let mut state = app.state.lock().unwrap();
    let task = state
        .update_task(&id, title, body.archived)
        .ok_or(CmdError::NotFound)?;
    app.persist_ids(&mut state, &[id]);
    Ok(Json(task))
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

async fn provenance(
    State(app): State<Arc<App>>,
    Path(sha): Path<String>,
) -> Result<Json<lgtm_protocol::Provenance>, ApiError> {
    let state = app.state.lock().unwrap();
    crate::provenance::find(&state, &sha)
        .map(Json)
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "no commit like that".into(),
        ))
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

async fn interrupt(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Task>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let task = state.interrupt(&id)?;
    app.persist_ids(&mut state, std::slice::from_ref(&id));
    Ok(Json(task))
}

/// Header the orchestration loop's MCP server sets on the calls it makes, so
/// its approve is held to the policy a person may waive.
const ORCHESTRATOR_HEADER: &str = "x-lgtm-orchestrator";

/// Header naming the goal an orchestration pass was woken for. A person's
/// client never sets it.
const GOAL_HEADER: &str = "x-lgtm-goal";

/// When the caller named the goal it acts for, the task must be under it.
fn goal_scoped(headers: &axum::http::HeaderMap, task: &Task) -> Result<(), ApiError> {
    let Some(goal) = headers.get(GOAL_HEADER).and_then(|v| v.to_str().ok()) else {
        return Ok(());
    };
    if task.spec.goal.as_deref() == Some(goal) {
        return Ok(());
    }
    Err(ApiError(
        StatusCode::FORBIDDEN,
        format!("task is under another goal; this pass acts for goal {goal}"),
    ))
}

async fn approve(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Task>, ApiError> {
    let mut state = app.state.lock().unwrap();
    goal_scoped(
        &headers,
        &state.tasks.get(&id).ok_or(CmdError::NotFound)?.task,
    )?;
    if headers.contains_key(ORCHESTRATOR_HEADER) {
        let task = state.tasks.get(&id).ok_or(CmdError::NotFound)?;
        policy_clean(&task.task)?;
    }
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
        .map(|rec| rec.task.clone())
        .and_then(|task| app.push_token(&state, &task));
    let task = state.command(
        &id,
        &[TaskStatus::AwaitingReview],
        "task is not awaiting review",
        |task_id| OrchestratorMessage::Push { task_id, token },
    )?;
    Ok(Json(task))
}

/// What the checks and the review already cleared. A model can ask for an
/// approval the diff has not earned; a person's approve is their own call.
fn policy_clean(task: &Task) -> Result<(), ApiError> {
    let result = task
        .result
        .as_ref()
        .ok_or_else(|| conflict("task has no result".into()))?;
    if result.validation_failed() {
        return Err(conflict("checks failed".into()));
    }
    if result
        .review
        .as_ref()
        .is_some_and(lgtm_protocol::Review::has_blocking)
    {
        return Err(conflict("blocking review findings".into()));
    }
    Ok(())
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

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
