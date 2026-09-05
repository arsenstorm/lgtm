//! `/api/skills`: the procedures every agent run in a repository is handed.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{MemorySource, Skill, SkillFile, SkillPatch, TaskId, Verification};
use serde::Deserialize;

use super::{ApiError, AuthedUser};
use crate::state::App;

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "skill not found".into())
}

/// Query of `GET /api/skills`.
#[derive(Deserialize)]
pub(super) struct SkillFilter {
    /// Git URL. Absent lists every skill, whatever its repository.
    #[serde(default)]
    repository: Option<String>,
    /// Only proposals still awaiting approval.
    #[serde(default)]
    pending: bool,
}

/// Body of `POST /api/skills`.
#[derive(Deserialize)]
pub(super) struct SkillRequest {
    #[serde(default)]
    repository: Option<String>,
    content: String,
    #[serde(default)]
    files: Vec<SkillFile>,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    source: MemorySource,
    #[serde(default)]
    proposed_by: Option<TaskId>,
}

pub(super) async fn list_skills(
    State(app): State<Arc<App>>,
    Query(filter): Query<SkillFilter>,
) -> Json<Vec<Skill>> {
    let state = app.state.lock().unwrap();
    // Repository scoping alone, not `Skill::is_told_to`: a proposal must
    // still show up here for a person to find and approve.
    let mut skills: Vec<Skill> = state
        .skills
        .values()
        .filter(|skill| {
            filter.repository.as_ref().is_none_or(|repository| {
                skill.repository.as_deref().is_none_or(|r| r == repository)
            }) && (!filter.pending || skill.verification == Verification::AgentProposed)
                && state.in_workspace(skill.workspace.as_deref())
        })
        .cloned()
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name).then(a.created_at.cmp(&b.created_at)));
    Json(skills)
}

pub(super) async fn create_skill(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<SkillRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Skill>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    if body.content.trim().is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "content is required".into(),
        ));
    }
    let mut state = app.state.lock().unwrap();
    // Passed untrimmed: a SKILL.md's whitespace is its own.
    let skill = state
        .create_skill(
            body.repository.clone(),
            body.content,
            body.files,
            body.origin,
            body.source,
            body.proposed_by.clone(),
            user.0,
        )
        .map_err(|reason| ApiError(StatusCode::BAD_REQUEST, reason))?;
    tracing::info!(skill = %skill.id, name = %skill.name, "skill recorded");
    app.persist_skill(&skill);
    Ok((StatusCode::CREATED, Json(skill)))
}

pub(super) async fn get_skill(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Skill>, ApiError> {
    let state = app.state.lock().unwrap();
    state
        .skills
        .get(&id)
        .filter(|skill| state.in_workspace(skill.workspace.as_deref()))
        .cloned()
        .map(Json)
        .ok_or_else(not_found)
}

pub(super) async fn update_skill(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<SkillPatch>, JsonRejection>,
) -> Result<Json<Skill>, ApiError> {
    let Json(patch) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let skill = state
        .edit_skill(&id, patch)
        .map_err(|reason| ApiError(StatusCode::BAD_REQUEST, reason))?
        .ok_or_else(not_found)?;
    app.persist_skill(&skill);
    Ok(Json(skill))
}

pub(super) async fn approve_skill(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Skill>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let skill = state.approve_skill(&id).ok_or_else(not_found)?;
    app.persist_skill(&skill);
    Ok(Json(skill))
}

pub(super) async fn delete_skill(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut state = app.state.lock().unwrap();
    if !state.remove_skill(&id) {
        return Err(not_found());
    }
    app.forget_skill(&id);
    Ok(StatusCode::NO_CONTENT)
}
