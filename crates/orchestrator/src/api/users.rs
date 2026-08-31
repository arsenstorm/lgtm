//! `/api/users`: the people who can call this API with tokens of their own.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use lgtm_protocol::{CreatedUser, User};
use serde::Deserialize;

use super::{ApiError, AuthedUser};
use crate::state::App;

/// Minting and revoking need the shared token: a per-user token that could
/// mint spares or revoke others would make revocation meaningless.
fn admin_only(user: &AuthedUser) -> Result<(), ApiError> {
    if user.0.is_some() {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "needs the shared orchestrator token".into(),
        ));
    }
    Ok(())
}

/// Body of `POST /api/users`.
#[derive(Deserialize)]
pub(super) struct UserRequest {
    name: String,
}

pub(super) async fn list_users(State(app): State<Arc<App>>) -> Json<Vec<User>> {
    let state = app.state.lock().unwrap();
    let mut users: Vec<User> = state.users.values().map(|rec| rec.user.clone()).collect();
    users.sort_by_key(|user| user.created_at);
    Json(users)
}

pub(super) async fn create_user(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    body: Result<Json<UserRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<CreatedUser>), ApiError> {
    admin_only(&user)?;
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError(StatusCode::BAD_REQUEST, "name is required".into()));
    }
    let mut state = app.state.lock().unwrap();
    let (user, token) = state.create_user(name);
    tracing::info!(user = %user.id, name = %user.name, "user created");
    app.persist_users(&state);
    Ok((StatusCode::CREATED, Json(CreatedUser { user, token })))
}

pub(super) async fn revoke_user(
    State(app): State<Arc<App>>,
    Extension(user): Extension<AuthedUser>,
    Path(id): Path<String>,
) -> Result<Json<User>, ApiError> {
    admin_only(&user)?;
    let mut state = app.state.lock().unwrap();
    let user = state
        .revoke_user(&id)
        .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "user not found".into()))?;
    tracing::info!(user = %user.id, "user revoked");
    app.persist_users(&state);
    Ok(Json(user))
}
