//! `/api/todos`: lightweight notes about work to do, promotable into a task.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use lgtm_protocol::{Executor, Priority, Task, Todo, TodoPatch};
use serde::Deserialize;

use super::{conflict, ApiError};
use crate::state::App;
use crate::todo::{PromoteInto, UpdateTodoError};

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
    #[serde(default)]
    priority: Priority,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    blockers: Vec<String>,
}

impl From<UpdateTodoError> for ApiError {
    fn from(err: UpdateTodoError) -> Self {
        match err {
            UpdateTodoError::NotFound => ApiError(StatusCode::NOT_FOUND, "todo not found".into()),
            UpdateTodoError::UnknownBlocker(id) => {
                ApiError(StatusCode::BAD_REQUEST, format!("unknown blocker {id}"))
            }
            UpdateTodoError::SelfBlocker => {
                ApiError(StatusCode::BAD_REQUEST, "todo cannot block itself".into())
            }
        }
    }
}

/// Body of `POST /api/todos/:id/promote`.
#[derive(Deserialize)]
pub(super) struct PromoteRequest {
    base_branch: String,
    executor: Executor,
    #[serde(default, alias = "worker")]
    runner: Option<String>,
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
    let mut todo = state.create_todo(body.repository, title.to_string(), body.description);
    todo.priority = body.priority;
    todo.assignee = body.assignee;
    todo.blockers = body.blockers;
    state.todos.insert(todo.id.clone(), todo.clone());
    app.persist_todo(&todo);
    Ok((StatusCode::CREATED, Json(todo)))
}

pub(super) async fn update_todo(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    body: Result<Json<TodoPatch>, JsonRejection>,
) -> Result<Json<Todo>, ApiError> {
    let Json(patch) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let todo = state.update_todo(&id, patch)?;
    app.persist_todo(&todo);
    Ok(Json(todo))
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
        runner: body.runner,
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
