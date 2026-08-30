//! `/api/todos`: lightweight notes about work to do, promotable into a task.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use lgtm_protocol::{Executor, Task, Todo};
use serde::Deserialize;

use super::{conflict, ApiError};
use crate::state::App;
use crate::todo::PromoteInto;

/// Query of `GET /api/todos`.
#[derive(Deserialize)]
pub(super) struct TodoFilter {
    /// Git URL. Absent lists every todo, whatever its repository.
    #[serde(default)]
    repository: Option<String>,
}

/// Body of `POST /api/todos`.
#[derive(Deserialize)]
pub(super) struct TodoRequest {
    #[serde(default)]
    repository: Option<String>,
    title: String,
    #[serde(default)]
    description: String,
}

/// Body of `POST /api/todos/:id/promote`.
#[derive(Deserialize)]
pub(super) struct PromoteRequest {
    base_branch: String,
    executor: Executor,
    #[serde(default)]
    worker: Option<String>,
}

pub(super) async fn list_todos(
    State(app): State<Arc<App>>,
    Query(filter): Query<TodoFilter>,
) -> Json<Vec<Todo>> {
    let state = app.state.lock().unwrap();
    let mut todos: Vec<Todo> = state
        .todos
        .values()
        .filter(|todo| {
            filter
                .repository
                .as_ref()
                .is_none_or(|repository| todo.repository.as_deref().is_none_or(|r| r == repository))
        })
        .cloned()
        .collect();
    todos.sort_by_key(|todo| todo.created_at);
    Json(todos)
}

pub(super) async fn create_todo(
    State(app): State<Arc<App>>,
    body: Result<Json<TodoRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Todo>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let title = body.title.trim();
    if title.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "title is required".into(),
        ));
    }
    let mut state = app.state.lock().unwrap();
    let todo = state.create_todo(body.repository, title.to_string(), body.description);
    app.persist_todo(&todo);
    Ok((StatusCode::CREATED, Json(todo)))
}

pub(super) async fn finish_todo(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Todo>, ApiError> {
    let mut state = app.state.lock().unwrap();
    let todo = state
        .finish_todo(&id)
        .ok_or(ApiError(StatusCode::NOT_FOUND, "todo not found".into()))?;
    app.persist_todo(&todo);
    Ok(Json(todo))
}

pub(super) async fn promote_todo(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<PromoteRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let into = PromoteInto {
        base_branch: body.base_branch,
        executor: body.executor,
        worker: body.worker,
    };
    let (task, changed) = state.promote_todo(&id, into).map_err(conflict)?;
    app.persist_ids(&mut state, &changed);
    if let Some(todo) = state.todos.get(&id) {
        app.persist_todo(todo);
    }
    Ok((StatusCode::CREATED, Json(task)))
}

pub(super) async fn delete_todo(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let mut state = app.state.lock().unwrap();
    if !state.remove_todo(&id) {
        return Err(ApiError(StatusCode::NOT_FOUND, "todo not found".into()));
    }
    app.forget_todo(&id);
    Ok(StatusCode::NO_CONTENT)
}
