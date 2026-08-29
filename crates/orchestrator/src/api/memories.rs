//! `/api/memories`: the facts every agent run in a repository is told.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use lgtm_protocol::Memory;
use serde::Deserialize;

use super::ApiError;
use crate::state::App;

/// Query of `GET /api/memories`.
#[derive(Deserialize)]
pub(super) struct MemoryFilter {
    /// Git URL. Absent lists every memory, whatever its repository.
    #[serde(default)]
    repository: Option<String>,
}

/// Body of `POST /api/memories`.
#[derive(Deserialize)]
pub(super) struct MemoryRequest {
    #[serde(default)]
    repository: Option<String>,
    content: String,
}

pub(super) async fn list_memories(
    State(app): State<Arc<App>>,
    Query(filter): Query<MemoryFilter>,
) -> Json<Vec<Memory>> {
    let state = app.state.lock().unwrap();
    let mut memories: Vec<Memory> = state
        .memories
        .values()
        .filter(|memory| {
            filter
                .repository
                .as_ref()
                .is_none_or(|repository| memory.applies_to(repository))
        })
        .cloned()
        .collect();
    memories.sort_by_key(|memory| memory.created_at);
    Json(memories)
}

pub(super) async fn create_memory(
    State(app): State<Arc<App>>,
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
    let memory = state.create_memory(body.repository.clone(), content.to_string());
    tracing::info!(memory = %memory.id, "memory recorded");
    app.persist_memory(&memory);
    Ok((StatusCode::CREATED, Json(memory)))
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
