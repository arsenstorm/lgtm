//! `/api/memories`: the facts every agent run in a repository is told.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{Memory, MemorySource, TaskId, Verification};
use serde::Deserialize;

use super::{ApiError, AuthedUser};
use crate::state::App;

/// Query of `GET /api/memories`.
#[derive(Deserialize)]
pub(super) struct MemoryFilter {
    /// Git URL. Absent lists every memory, whatever its repository.
    #[serde(default)]
    repository: Option<String>,
    /// Only proposals still awaiting approval.
    #[serde(default)]
    pending: bool,
}

/// Body of `POST /api/memories`.
#[derive(Deserialize)]
pub(super) struct MemoryRequest {
    #[serde(default)]
    repository: Option<String>,
    content: String,
    #[serde(default)]
    source: MemorySource,
    #[serde(default)]
    proposed_by: Option<TaskId>,
}

pub(super) async fn list_memories(
    State(app): State<Arc<App>>,
    Query(filter): Query<MemoryFilter>,
) -> Json<Vec<Memory>> {
    let state = app.state.lock().unwrap();
    // Repository scoping alone, not `Memory::is_told_to`: a proposal must
    // still show up here for a person to find and approve.
    let mut memories: Vec<Memory> = state
        .memories
        .values()
        .filter(|memory| {
            filter.repository.as_ref().is_none_or(|repository| {
                memory.repository.as_deref().is_none_or(|r| r == repository)
            }) && (!filter.pending || memory.verification == Verification::AgentProposed)
                && state.in_workspace(memory.workspace.as_deref())
        })
        .cloned()
        .collect();
    memories.sort_by_key(|memory| memory.created_at);
    Json(memories)
}

pub(super) async fn create_memory(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<MemoryRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Memory>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let content = body.content.trim();
    if content.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "content is required".into(),
        ));
    }
    let mut state = app.state.lock().unwrap();
    let memory = state.create_memory(
        body.repository.clone(),
        content.to_string(),
        body.source,
        body.proposed_by.clone(),
        user.0,
    );
    tracing::info!(memory = %memory.id, "memory recorded");
    app.persist_memory(&memory);
    Ok((StatusCode::CREATED, Json(memory)))
}

/// Body of `PATCH /api/memories/:id`.
#[derive(Deserialize)]
pub(super) struct MemoryEdit {
    content: String,
}

pub(super) async fn update_memory(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<MemoryEdit>, JsonRejection>,
) -> Result<Json<Memory>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let content = body.content.trim();
    if content.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "content is required".into(),
        ));
    }
    let mut state = app.state.lock().unwrap();
    let memory = state
        .edit_memory(&id, content.to_string())
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "memory not found".into()))?;
    app.persist_memory(&memory);
    Ok(Json(memory))
}

pub(super) async fn approve_memory(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Memory>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let memory = state
        .approve_memory(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "memory not found".into()))?;
    app.persist_memory(&memory);
    Ok(Json(memory))
}

pub(super) async fn delete_memory(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut state = app.state.lock().unwrap();
    if !state.remove_memory(&id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "memory not found".into()));
    }
    app.forget_memory(&id);
    Ok(StatusCode::NO_CONTENT)
}
