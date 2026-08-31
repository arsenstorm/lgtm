//! `/api/sessions`: one chat thread per repository, and posting a message
//! into one as a task.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{Executor, SandboxProfile, Session, SessionDetail, Task, TaskKind, TaskSpec};
use serde::Deserialize;

use super::{conflict, ApiError, AuthedUser};
use crate::state::App;

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "session not found".into())
}

#[derive(Deserialize)]
pub(super) struct SessionRequest {
    repository: String,
    base_branch: String,
    #[serde(default)]
    title: String,
}

pub(super) async fn create_session(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<SessionRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Session>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let session = state.create_session(body.repository, body.base_branch, body.title, user.0);
    app.persist_session(&session);
    Ok((StatusCode::CREATED, Json(session)))
}

#[derive(Deserialize)]
pub(super) struct SessionQuery {
    repository: Option<String>,
}

pub(super) async fn list_sessions(
    State(app): State<Arc<App>>,
    Query(query): Query<SessionQuery>,
) -> Json<Vec<Session>> {
    let state = app.state.lock().unwrap();
    let mut sessions: Vec<Session> = state
        .sessions
        .values()
        .filter(|session| {
            query
                .repository
                .as_deref()
                .is_none_or(|repo| session.repository == repo)
                && state.in_workspace(session.workspace.as_deref())
        })
        .cloned()
        .collect();
    sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
    Json(sessions)
}

pub(super) async fn get_session(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<SessionDetail>, ApiError> {
    let state = app.state.lock().unwrap();
    let session = state.sessions.get(&id).cloned().ok_or_else(not_found)?;
    let tasks = state.session_tasks(&id).into_iter().cloned().collect();
    Ok(Json(SessionDetail { session, tasks }))
}

#[derive(Deserialize)]
pub(super) struct MessageBody {
    text: String,
    executor: Executor,
    #[serde(default, alias = "worker")]
    runner: Option<String>,
    #[serde(default)]
    sandbox: Option<SandboxProfile>,
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    review_executor: Option<Executor>,
}

/// The task a chat message becomes: it belongs to the session's repository,
/// not whatever the caller might have passed.
fn message_spec(session: &Session, id: &str, body: MessageBody) -> TaskSpec {
    TaskSpec {
        repository: session.repository.clone(),
        base_branch: session.base_branch.clone(),
        prompt: body.text,
        executor: body.executor,
        runner: body.runner,
        issue: None,
        linear: None,
        kind: TaskKind::Run,
        parent: None,
        depends_on: Vec::new(),
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: body.sandbox,
        requirements: body.requirements,
        goal: None,
        review_executor: body.review_executor,
        model: body.model,
        allowed_hosts: Vec::new(),
        session: Some(id.to_string()),
        created_by: None,
    }
}

pub(super) async fn send_message(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    Path(id): Path<String>,
    body: Result<Json<MessageBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let session = state.sessions.get(&id).cloned().ok_or_else(not_found)?;
    let text = body.text.clone();
    let mut spec = message_spec(&session, &id, body);
    spec.created_by = user.0.clone();
    if app.orchestrate.is_some() {
        let goal = state.create_goal(text.clone(), session.repository.clone(), user.0);
        app.persist_goal(&goal);
        spec.goal = Some(goal.id);
    }
    let goal = spec.goal.clone();
    let (task, changed) = match state.create_task(spec) {
        Ok(created) => created,
        // A goal nothing can work on is worse than no goal at all.
        Err(err) => {
            if let Some(goal) = goal {
                state.goals.remove(&goal);
            }
            return Err(conflict(err));
        }
    };
    app.persist_ids(&mut state, &changed);
    if let Some(session) = state.fill_session_title(&id, &text) {
        app.persist_session(&session);
    }
    Ok((StatusCode::CREATED, Json(task)))
}
