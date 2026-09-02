//! `/api/credentials`: who a workspace's commits are attributed to, and what
//! pushes them. A credential goes in here and never comes back out — the API
//! only ever returns the name attached to it.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{CredentialKind, CredentialSummary, Identity, WorkspaceSettings};
use serde::Deserialize;

use super::{ApiError, AuthedUser};
use crate::credentials::{CredentialRecord, WorkspaceRecord};
use crate::state::App;

/// Body of `POST /api/credentials`.
#[derive(Deserialize)]
pub(super) struct CredentialRequest {
    #[serde(default)]
    workspace: Option<String>,
    kind: CredentialKind,
    #[serde(default)]
    owner: Option<String>,
    name: String,
    email: String,
    /// A credential for pushing over https.
    #[serde(default)]
    token: Option<String>,
    /// Path to an SSH key on the runner, for signing and for pushing.
    #[serde(default)]
    ssh_key: Option<String>,
}

/// A per-user token may register a credential for its own holder and nobody
/// else; anything wider needs the shared token. Otherwise one teammate could
/// register a credential that pushes as another.
fn may_own(user: &AuthedUser, owner: Option<&str>) -> Result<(), ApiError> {
    match (&user.0, owner) {
        // The shared token is the workspace admin: it may set any owner.
        (None, _) => Ok(()),
        (Some(id), Some(owner)) if id == owner => Ok(()),
        (Some(_), Some(_)) => Err(ApiError(
            StatusCode::FORBIDDEN,
            "a per-user token may only register a credential it owns".into(),
        )),
        (Some(_), None) => Err(ApiError(
            StatusCode::FORBIDDEN,
            "an unowned credential is shared by the workspace and needs the shared token".into(),
        )),
    }
}

pub(super) async fn list(State(app): State<Arc<App>>) -> Json<Vec<CredentialSummary>> {
    let state = app.state.lock().unwrap();
    Json(
        state
            .credentials
            .credentials
            .iter()
            .map(CredentialRecord::summary)
            .collect(),
    )
}

pub(super) async fn create(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<CredentialRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CredentialSummary>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    // A credential that carries neither cannot push or sign, and would only
    // fail later on a runner.
    if body.token.is_none() && body.ssh_key.is_none() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "a credential needs a token or an ssh key".into(),
        ));
    }
    // A human credential is always owned: an unowned one would say anyone may
    // push as this person.
    let owner = match (body.kind, body.owner.clone()) {
        (CredentialKind::Human, None) => user.0.clone().ok_or_else(|| {
            ApiError(
                StatusCode::BAD_REQUEST,
                "a human credential needs an owner".into(),
            )
        })?,
        (CredentialKind::Human, Some(owner)) => owner,
        (CredentialKind::Agent, owner) => {
            may_own(&user, owner.as_deref())?;
            return Ok(insert(&app, &body, owner));
        }
    };
    may_own(&user, Some(&owner))?;
    Ok(insert(&app, &body, Some(owner)))
}

fn insert(
    app: &Arc<App>,
    body: &CredentialRequest,
    owner: Option<String>,
) -> (StatusCode, Json<CredentialSummary>) {
    let mut state = app.state.lock().unwrap();
    let id = crate::state::random_id();
    let record = CredentialRecord {
        id: id.clone(),
        workspace: body.workspace.clone(),
        kind: body.kind,
        owner,
        identity: Identity {
            name: body.name.clone(),
            email: body.email.clone(),
        },
        token: body.token.clone(),
        ssh_key: body.ssh_key.clone(),
    };
    let summary = record.summary();
    // One credential per (workspace, kind, owner): registering again replaces
    // the old one rather than leaving two that resolution picks between.
    state.credentials.credentials.retain(|it| {
        !(it.kind == record.kind && it.workspace == record.workspace && it.owner == record.owner)
    });
    state.credentials.credentials.push(record);
    tracing::info!(credential = %id, "credential registered");
    app.persist_credentials(&state);
    (StatusCode::CREATED, Json(summary))
}

pub(super) async fn remove(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut state = app.state.lock().unwrap();
    let held = state
        .credentials
        .credentials
        .iter()
        .find(|it| it.id == id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "credential not found".into()))?;
    may_own(&user, held.owner.as_deref())?;
    state.credentials.credentials.retain(|it| it.id != id);
    tracing::info!(credential = %id, "credential removed");
    app.persist_credentials(&state);
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn settings(State(app): State<Arc<App>>) -> Json<WorkspaceSettings> {
    let state = app.state.lock().unwrap();
    let workspace = state.workspace.clone();
    Json(state.credentials.public_settings(workspace.as_deref()))
}

pub(super) async fn set_settings(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<WorkspaceSettings>, JsonRejection>,
) -> Result<Json<WorkspaceSettings>, ApiError> {
    // Which mode a workspace pushes in is a workspace-wide decision.
    if user.0.is_some() {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "needs the shared orchestrator token".into(),
        ));
    }
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let workspace = state.workspace.clone();
    state
        .credentials
        .workspaces
        .retain(|it| it.workspace != workspace);
    state.credentials.workspaces.push(WorkspaceRecord {
        workspace,
        mode: body.mode,
        credit_agent: body.credit_agent,
    });
    tracing::info!(mode = ?body.mode, credit_agent = body.credit_agent, "workspace authorship set");
    app.persist_credentials(&state);
    Ok(Json(body))
}
