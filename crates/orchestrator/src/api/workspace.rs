//! `/api/activity`, `/api/ask` and `/api/enhance`: the whole workspace at a
//! glance, one question to the shared agent, and one rewrite of a task prompt.
//! One orchestrator is one workspace, so they read across every task rather
//! than down one goal.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{Executor, TaskEvent};
use serde::{Deserialize, Serialize};

use super::{conflict, ApiError, AuthedUser};
use crate::state::App;

/// Query of `GET /api/activity`.
#[derive(Deserialize)]
pub(super) struct ActivityQuery {
    #[serde(default)]
    limit: Option<u32>,
}

/// One recent event, newest first, for "what just happened" across every task.
#[derive(Serialize)]
pub(super) struct ActivityLine {
    at: u64,
    task: String,
    /// The creator's display name (their id when the record is gone), or
    /// `None` for the shared token or automation.
    owner: Option<String>,
    repository: String,
    event: String,
    detail: String,
}

pub(super) async fn activity(
    State(app): State<Arc<App>>,
    Query(query): Query<ActivityQuery>,
) -> Json<Vec<ActivityLine>> {
    let limit = query.limit.unwrap_or(30).min(100) as usize;
    let state = app.state.lock().unwrap();
    // Event logs are append-only and never trimmed, so each task contributes
    // at most its `limit` newest lines: the merged result is identical and
    // the scan stays bounded while the state lock is held. Output is one
    // event per stdout line — a single running agent would flood the feed
    // with it, and it says nothing `Progress` and `Command` don't.
    let mut lines: Vec<ActivityLine> = state
        .tasks
        .values()
        .filter(|rec| state.in_workspace(rec.task.workspace.as_deref()))
        .flat_map(|rec| {
            let owner = rec.task.created_by.as_ref().map(|id| {
                state
                    .users
                    .get(id)
                    .map_or_else(|| id.clone(), |rec| rec.user.name.clone())
            });
            rec.events
                .iter()
                .rev()
                .filter(|stored| !matches!(stored.event, TaskEvent::Output { .. }))
                .take(limit)
                .map(move |stored| ActivityLine {
                    at: stored.at,
                    task: rec.task.id.clone(),
                    owner: owner.clone(),
                    repository: rec.task.spec.repository.clone(),
                    event: tag(&stored.event),
                    detail: detail(&stored.event),
                })
        })
        .collect();
    lines.sort_by_key(|line| std::cmp::Reverse(line.at));
    lines.truncate(limit);
    Json(lines)
}

/// The wire spelling, so a reader sees the same word the event stream uses.
fn tag(event: &TaskEvent) -> String {
    serde_json::to_value(event)
        .ok()
        .and_then(|value| value.get("type")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// One line of context per event; the full event is a task away.
fn detail(event: &TaskEvent) -> String {
    let first_line = |text: &str| text.lines().next().unwrap_or_default().to_string();
    match event {
        TaskEvent::Progress { text } => first_line(text),
        TaskEvent::Command { command } => first_line(command),
        TaskEvent::Failed { error } => first_line(error),
        TaskEvent::Completed { result } => {
            format!("{} files changed", result.changed_files.len())
        }
        TaskEvent::Orchestrated { action, .. } => action.clone(),
        _ => String::new(),
    }
}

/// Body of `POST /api/ask`.
#[derive(Deserialize)]
pub(super) struct AskRequest {
    question: String,
}

#[derive(Serialize)]
pub(super) struct AskResponse {
    answer: String,
}

pub(super) async fn ask(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<AskRequest>, JsonRejection>,
) -> Result<Json<AskResponse>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let question = body.question.trim();
    if question.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "question is required".into(),
        ));
    }
    if app.orchestrate.is_none() {
        return Err(conflict("--orchestrate is not configured".into()));
    }
    let Ok(_permit) = app.asking.try_acquire() else {
        return Err(conflict(
            "too many questions at once; try again shortly".into(),
        ));
    };
    let asked = question.to_string();
    let started = Instant::now();
    let answer = crate::orchestrate::answer_question(app.clone(), asked.clone())
        .await
        .map_err(|note| ApiError(StatusCode::BAD_GATEWAY, note))?;
    tracing::info!(
        user = %user.0.as_deref().unwrap_or("-"),
        question = %asked.lines().next().unwrap_or_default(),
        ms = started.elapsed().as_millis(),
        "ask answered"
    );
    Ok(Json(AskResponse { answer }))
}

/// Body of `POST /api/enhance`.
#[derive(Deserialize)]
pub(super) struct EnhanceRequest {
    prompt: String,
    /// Named so the rewrite can say which repository the work is in; the call
    /// reads no code, so an unknown one costs nothing.
    #[serde(default)]
    repository: Option<String>,
}

#[derive(Serialize)]
pub(super) struct EnhanceResponse {
    prompt: String,
}

const ENHANCE_SYSTEM: &str = "You rewrite a developer's rough note into the prompt for a coding \
     agent that will work alone in one repository with no other context. Say what the goal is, \
     the context the agent needs, the constraints it must respect, and what the finished result \
     looks like. Keep the developer's intent and their language, and invent no requirement they \
     did not ask for. Return ONLY the rewritten prompt: no preamble, no explanation, no code \
     fence.";

/// What a person is shown when nothing can run the rewrite; it goes to the UI
/// as it reads here.
const NO_EXECUTOR: &str = "no runner or local executor available for enhancement";

pub(super) async fn enhance(
    State(app): State<Arc<App>>,
    body: Result<Json<EnhanceRequest>, JsonRejection>,
) -> Result<Json<EnhanceResponse>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    enhance_with(&app, body, || {
        crate::orchestrate::pick(crate::orchestrate::Choice::Auto)
    })
    .await
}

/// `local` is the host's own executor, a parameter so the PATH probe can be
/// injected the way `orchestrate::resolve` takes one.
pub(super) async fn enhance_with(
    app: &Arc<App>,
    body: EnhanceRequest,
    local: impl Fn() -> Option<Executor>,
) -> Result<Json<EnhanceResponse>, ApiError> {
    let prompt = body.prompt.trim();
    if prompt.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "prompt is required".into(),
        ));
    }
    // Read what the choice needs and drop the lock: the model call is seconds
    // long and every other request would queue behind it.
    let executor = {
        let state = app.state.lock().unwrap();
        choose_executor(&state, local)
    }
    .ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, NO_EXECUTOR.into()))?;
    let asked = match body
        .repository
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        Some(repository) => format!("Repository: {repository}\n\nTask:\n{prompt}"),
        None => prompt.to_string(),
    };
    let rewritten = crate::infer::infer(app, executor, ENHANCE_SYSTEM, &asked)
        .await
        .map_err(|err| match err {
            crate::infer::InferError::Unavailable => {
                ApiError(StatusCode::SERVICE_UNAVAILABLE, NO_EXECUTOR.into())
            }
            crate::infer::InferError::Failed(note) => ApiError(StatusCode::BAD_GATEWAY, note),
        })?;
    Ok(Json(EnhanceResponse {
        prompt: rewritten.trim().to_string(),
    }))
}

/// The executor to rewrite on: what a connected runner advertises, else one
/// this host has itself.
pub(super) fn choose_executor(
    state: &crate::state::State,
    local: impl Fn() -> Option<Executor>,
) -> Option<Executor> {
    state
        .runners
        .values()
        .filter(|runner| runner.is_connected())
        .find_map(|runner| runner.info.executors.first().copied())
        .or_else(local)
}
