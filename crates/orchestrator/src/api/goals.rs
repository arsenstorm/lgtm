//! `/api/goals`: stating an outcome, and inspecting the tasks under it.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use lgtm_protocol::{plan_versions, Executor, GoalSummary, PlanVersion, Task, TaskKind, TaskSpec};
use serde::{Deserialize, Serialize};

use super::{conflict, ApiError};
use crate::state::App;

/// Body of `POST /api/goals`.
#[derive(Deserialize)]
pub(super) struct GoalRequest {
    objective: String,
    repository: String,
    base_branch: String,
    executor: Executor,
    #[serde(default, alias = "worker")]
    runner: Option<String>,
    /// Propose a plan first instead of running the objective as one task.
    #[serde(default)]
    plan: bool,
}

#[derive(Serialize)]
pub(super) struct GoalDetail {
    summary: GoalSummary,
    tasks: Vec<Task>,
}

fn not_found() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "goal not found".into())
}

/// The one task a new goal starts with: the objective itself.
fn first_spec(body: GoalRequest, goal: String) -> TaskSpec {
    TaskSpec {
        repository: body.repository,
        base_branch: body.base_branch,
        prompt: body.objective,
        executor: body.executor,
        runner: body.runner,
        issue: None,
        linear: None,
        kind: if body.plan {
            TaskKind::Plan
        } else {
            TaskKind::Run
        },
        parent: None,
        depends_on: Vec::new(),
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: None,
        requirements: Vec::new(),
        goal: Some(goal),
        review_executor: None,
        model: None,
        allowed_hosts: Vec::new(),
    }
}

pub(super) async fn create_goal(
    State(app): State<Arc<App>>,
    body: Result<Json<GoalRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<GoalSummary>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let mut state = app.state.lock().unwrap();
    let goal = state.create_goal(body.objective.clone(), body.repository.clone());
    let spec = first_spec(body, goal.id.clone());
    let changed = match state.create_task(spec) {
        Ok((_, changed)) => changed,
        // A goal nothing can work on is worse than no goal at all.
        Err(err) => {
            state.goals.remove(&goal.id);
            return Err(conflict(err));
        }
    };
    app.persist_ids(&mut state, &changed);
    let summary = state.goal_summary(&goal.id).ok_or_else(not_found)?;
    app.persist_goal(&summary.goal);
    Ok((StatusCode::CREATED, Json(summary)))
}

pub(super) async fn list_goals(State(app): State<Arc<App>>) -> Json<Vec<GoalSummary>> {
    let state = app.state.lock().unwrap();
    let mut goals: Vec<GoalSummary> = state
        .goals
        .keys()
        .filter_map(|id| state.goal_summary(id))
        .collect();
    goals.sort_by_key(|summary| summary.goal.created_at);
    Json(goals)
}

pub(super) async fn get_goal(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<GoalDetail>, ApiError> {
    let state = app.state.lock().unwrap();
    let summary = state.goal_summary(&id).ok_or_else(not_found)?;
    let tasks = state.goal_tasks(&id).into_iter().cloned().collect();
    Ok(Json(GoalDetail { summary, tasks }))
}

/// Every version every plan task under the goal has produced, oldest task
/// first, versions within a task in event order.
pub(super) async fn get_goal_plans(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<PlanVersion>>, ApiError> {
    let state = app.state.lock().unwrap();
    state.goals.get(&id).ok_or_else(not_found)?;
    let versions = state
        .goal_tasks(&id)
        .into_iter()
        .filter(|task| task.spec.kind == TaskKind::Plan)
        .flat_map(|task| plan_versions(task, &state.tasks[&task.id].events))
        .collect();
    Ok(Json(versions))
}
