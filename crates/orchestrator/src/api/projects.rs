//! `/api/projects`: the repository prefixes todo display ids are built from.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use lgtm_protocol::Project;
use serde::Deserialize;

use super::{conflict, ApiError};
use crate::project::PREFIX_MAX;
use crate::state::App;

/// Body of `POST /api/projects`.
#[derive(Deserialize)]
pub(super) struct CreateProjectRequest {
    repository: String,
}

/// Body of `PATCH /api/projects/:id`.
#[derive(Deserialize)]
pub(super) struct PrefixRequest {
    prefix: String,
}

pub(super) async fn list_projects(State(app): State<Arc<App>>) -> Json<Vec<Project>> {
    let state = app.state.lock().unwrap();
    let mut projects: Vec<Project> = state.projects.values().cloned().collect();
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Json(projects)
}

pub(super) async fn create_project(
    State(app): State<Arc<App>>,
    body: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let repository = body.repository.trim();
    if repository.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "repository is required".into(),
        ));
    }
    let mut state = app.state.lock().unwrap();
    let existed = state
        .projects
        .values()
        .any(|project| project.repository.as_deref() == Some(repository));
    let id = state.project_for(Some(repository));
    let project = state.projects[&id].clone();
    app.persist_projects(&mut state);
    let status = if existed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(project)))
}

pub(super) async fn update_project(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<PrefixRequest>, JsonRejection>,
) -> Result<Json<Project>, ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let prefix = body.prefix.trim().to_ascii_uppercase();
    // Digits are allowed past the first character because derivation itself
    // mints them when a name is exhausted ("LGTM2"): whatever the system can
    // produce, a person must be able to type back.
    if prefix.is_empty()
        || prefix.len() > PREFIX_MAX
        || !prefix.starts_with(|c: char| c.is_ascii_alphabetic())
        || !prefix.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            format!("prefix must be 1 to {PREFIX_MAX} letters or digits, starting with a letter"),
        ));
    }
    let mut state = app.state.lock().unwrap();
    // Two projects sharing a prefix would make `L-3` name two todos, which is
    // the one thing a display id must never do.
    if let Some(holder) = state
        .projects
        .values()
        .find(|project| project.prefix == prefix && project.id != id)
    {
        return Err(conflict(format!(
            "prefix {prefix} is taken by {}",
            holder.name
        )));
    }
    let project = state
        .projects
        .get_mut(&id)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "project not found".into()))?;
    project.prefix = prefix;
    let project = project.clone();
    app.persist_project(&project);
    Ok(Json(project))
}
