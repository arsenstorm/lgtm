//! `/api/scratchpads`: standalone markdown documents people and agents share.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::Scratchpad;
use serde::{Deserialize, Deserializer};

use super::{tags, ApiError, AuthedUser};
use crate::state::App;

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "scratchpad not found".into())
}

/// Query of `GET /api/scratchpads`.
#[derive(Deserialize)]
pub(super) struct ScratchpadFilter {
    /// Git URL. Absent lists every scratchpad, whatever its repository.
    #[serde(default)]
    repository: Option<String>,
}

/// Body of `POST /api/scratchpads`.
#[derive(Deserialize)]
pub(super) struct ScratchpadRequest {
    #[serde(default)]
    repository: Option<String>,
    /// May be empty: a fresh blank document is a legitimate thing to make.
    #[serde(default)]
    content: String,
    #[serde(default)]
    tags: Vec<String>,
}

pub(super) async fn list_scratchpads(
    State(app): State<Arc<App>>,
    Query(filter): Query<ScratchpadFilter>,
) -> Json<Vec<Scratchpad>> {
    let state = app.state.lock().unwrap();
    let mut scratchpads: Vec<Scratchpad> = state
        .scratchpads
        .values()
        .filter(|scratchpad| {
            filter.repository.as_ref().is_none_or(|repository| {
                scratchpad
                    .repository
                    .as_deref()
                    .is_none_or(|r| r == repository)
            }) && state.in_workspace(scratchpad.workspace.as_deref())
        })
        .cloned()
        .collect();
    scratchpads.sort_by_key(|scratchpad| scratchpad.created_at);
    Json(scratchpads)
}

pub(super) async fn create_scratchpad(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<ScratchpadRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Scratchpad>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let tags = tags(body.tags)?;
    let mut state = app.state.lock().unwrap();
    let scratchpad = state.create_scratchpad(body.repository, body.content, tags, user.0);
    app.persist_scratchpad(&scratchpad);
    Ok((StatusCode::CREATED, Json(scratchpad)))
}

pub(super) async fn get_scratchpad(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Scratchpad>, ApiError> {
    let state = app.state.lock().unwrap();
    state
        .scratchpads
        .get(&id)
        .cloned()
        .map(Json)
        .ok_or_else(not_found)
}

/// Body of `PATCH /api/scratchpads/:id`: whichever fields are being changed.
#[derive(Deserialize)]
pub(super) struct ScratchpadPatch {
    /// Absent leaves the repository alone; `null` moves the document back to
    /// every repository.
    #[serde(default, deserialize_with = "nullable")]
    repository: Option<Option<String>>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
}

fn nullable<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    Option::deserialize(deserializer).map(Some)
}

pub(super) async fn update_scratchpad(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<ScratchpadPatch>, JsonRejection>,
) -> Result<Json<Scratchpad>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let tags = body.tags.map(tags).transpose()?;
    let mut state = app.state.lock().unwrap();
    let scratchpad = state
        .update_scratchpad(&id, body.repository, body.content, body.archived, tags)
        .ok_or_else(not_found)?;
    app.persist_scratchpad(&scratchpad);
    Ok(Json(scratchpad))
}

pub(super) async fn delete_scratchpad(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut state = app.state.lock().unwrap();
    if !state.remove_scratchpad(&id) {
        return Err(not_found());
    }
    app.forget_scratchpad(&id);
    Ok(StatusCode::NO_CONTENT)
}
