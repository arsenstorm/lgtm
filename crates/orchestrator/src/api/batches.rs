//! `/api/batches`: importing a backlog and inspecting the result.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use lgtm_protocol::{Batch, BatchSource, BatchSummary, Executor, Task, TaskId, TaskKind};
use serde::{Deserialize, Serialize};

use super::{bad_gateway, bad_linear, conflict, github, linear, ApiError};
use crate::backlog::{self, Candidate, SpecInput};
use crate::state::{now_ms, App, State as TaskState};

fn twenty() -> u32 {
    20
}

/// Body of `POST /api/batches`.
#[derive(Deserialize)]
pub(super) struct BatchRequest {
    source: BatchSource,
    /// Git URL the tasks clone from. Optional for GitHub, where the label's
    /// repository is the obvious default; required for Linear.
    #[serde(default)]
    repository: Option<String>,
    base_branch: String,
    executor: Executor,
    #[serde(default)]
    worker: Option<String>,
    /// Import each issue as a plan task instead of a run.
    #[serde(default)]
    plan: bool,
    /// Approve this batch's plans without a person.
    #[serde(default)]
    approve_plans: bool,
    #[serde(default = "twenty")]
    max: u32,
    /// Report what would be imported and create nothing.
    #[serde(default)]
    dry_run: bool,
}

#[derive(Serialize)]
pub(super) struct IssuePreview {
    key: String,
    title: String,
    url: String,
}

#[derive(Serialize)]
pub(super) struct BatchResponse {
    batch: Option<Batch>,
    issues: Vec<IssuePreview>,
}

#[derive(Serialize)]
pub(super) struct BatchDetail {
    batch: Batch,
    summary: BatchSummary,
    tasks: Vec<Task>,
}

/// The issues a source returned, still to be turned into candidates: the batch
/// id they carry is only known once the lock is held.
enum Fetched {
    Github(lgtm_github::Repo, Vec<lgtm_github::Issue>),
    Linear(Vec<lgtm_linear::Issue>),
}

/// Talks to the issue tracker, off the state lock. Returns the repository the
/// tasks will clone from alongside what it found.
async fn fetch_batch(app: &App, body: &BatchRequest) -> Result<(String, Fetched), ApiError> {
    match &body.source {
        BatchSource::GithubLabel { owner, repo, label } => {
            let github = github(app)?;
            let repo = lgtm_github::Repo {
                owner: owner.clone(),
                repo: repo.clone(),
            };
            let issues = github
                .issues_with_label(&repo, label)
                .await
                .map_err(bad_gateway)?;
            let repository = body
                .repository
                .clone()
                .unwrap_or_else(|| format!("https://github.com/{}/{}.git", repo.owner, repo.repo));
            Ok((repository, Fetched::Github(repo, issues)))
        }
        BatchSource::Linear { team, state } => {
            let linear = linear(app)?;
            let repository = body.repository.clone().ok_or_else(|| {
                ApiError(
                    StatusCode::BAD_REQUEST,
                    "repository is required for a linear batch".into(),
                )
            })?;
            let issues = linear
                .issues_in_state(team, state)
                .await
                .map_err(bad_linear)?;
            Ok((repository, Fetched::Linear(issues)))
        }
    }
}

pub(super) async fn create_batch(
    State(app): State<Arc<App>>,
    body: Result<Json<BatchRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<BatchResponse>), ApiError> {
    let Json(body) = body.map_err(|err| ApiError(StatusCode::BAD_REQUEST, err.body_text()))?;
    let (repository, fetched) = fetch_batch(&app, &body).await?;

    let mut state = app.state.lock().unwrap();
    let id = state.new_batch_id();
    let candidates = candidates(&fetched, &repository, &body, &id);
    let selected = select_new(&state, candidates, body.max);
    let issues = previews(&selected);
    if body.dry_run {
        let response = BatchResponse {
            batch: None,
            issues,
        };
        return Ok((StatusCode::OK, Json(response)));
    }

    let Created {
        task_ids,
        changed,
        refused,
    } = create_tasks(&mut state, selected);
    if let Some(err) = refused {
        app.persist_ids(&state, &changed);
        return Err(conflict(err));
    }
    let batch = Batch {
        id,
        created_at: now_ms(),
        source: body.source,
        repository,
        task_ids,
        approve_plans: body.approve_plans,
    };
    store(&app, &mut state, &batch, &changed);
    let response = BatchResponse {
        batch: Some(batch),
        issues,
    };
    Ok((StatusCode::CREATED, Json(response)))
}

fn store(app: &App, state: &mut TaskState, batch: &Batch, changed: &[TaskId]) {
    tracing::info!(batch = %batch.id, tasks = batch.task_ids.len(), "batch imported");
    state.batches.insert(batch.id.clone(), batch.clone());
    app.persist_batch(batch);
    app.persist_ids(state, changed);
}

/// Drops the candidates whose issue already has a live task.
// ponytail: copies every task to compare against; an index by issue
// reference is the upgrade if this ever shows up in a profile.
fn select_new(state: &TaskState, candidates: Vec<Candidate>, max: u32) -> Vec<Candidate> {
    let existing: Vec<Task> = state.tasks.values().map(|rec| rec.task.clone()).collect();
    backlog::select(&existing, candidates, max)
}

fn candidates(
    fetched: &Fetched,
    repository: &str,
    body: &BatchRequest,
    id: &str,
) -> Vec<Candidate> {
    let input = SpecInput {
        base_branch: body.base_branch.clone(),
        executor: body.executor,
        worker: body.worker.clone(),
        kind: if body.plan {
            TaskKind::Plan
        } else {
            TaskKind::Run
        },
        batch: Some(id.to_string()),
    };
    match fetched {
        Fetched::Github(repo, issues) => issues
            .iter()
            .map(|issue| backlog::github_candidate(issue, repo, input.clone()))
            .collect(),
        Fetched::Linear(issues) => issues
            .iter()
            .map(|issue| backlog::linear_candidate(issue, repository, input.clone()))
            .collect(),
    }
}

fn previews(selected: &[Candidate]) -> Vec<IssuePreview> {
    selected
        .iter()
        .map(|candidate| IssuePreview {
            key: candidate.key.clone(),
            title: candidate.title.clone(),
            url: candidate.url.clone(),
        })
        .collect()
}

/// What queueing a batch's candidates left behind: the tasks made, the ids to
/// persist either way, and the refusal that stopped it, if one did.
#[derive(Default)]
struct Created {
    task_ids: Vec<TaskId>,
    changed: Vec<TaskId>,
    refused: Option<String>,
}

fn create_tasks(state: &mut TaskState, selected: Vec<Candidate>) -> Created {
    let mut created = Created::default();
    // Every candidate shares executor and worker, so one refusal would hold
    // for all of them; check once before anything is created.
    if let Some(first) = selected.first() {
        if let Err(err) = state.check_eligible(&first.spec) {
            created.refused = Some(err);
            return created;
        }
    }
    for candidate in selected {
        match state.create_task(candidate.spec) {
            Ok((task, ids)) => {
                created.task_ids.push(task.id);
                created.changed.extend(ids);
            }
            // Whatever made this one ineligible holds for the rest, so stop
            // here. The tasks already created keep their place in the queue.
            Err(err) => {
                created.refused = Some(err);
                break;
            }
        }
    }
    created
}

pub(super) async fn list_batches(State(app): State<Arc<App>>) -> Json<Vec<Batch>> {
    let state = app.state.lock().unwrap();
    let mut batches: Vec<Batch> = state.batches.values().cloned().collect();
    batches.sort_by_key(|batch| batch.created_at);
    Json(batches)
}

pub(super) async fn get_batch(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
) -> Result<Json<BatchDetail>, ApiError> {
    let state = app.state.lock().unwrap();
    let batch = state
        .batches
        .get(&id)
        .cloned()
        .ok_or(ApiError(StatusCode::NOT_FOUND, "batch not found".into()))?;
    // The batch's own ids are the plan tasks; the children a plan created
    // carry the same batch, so membership is what the task says it is.
    let mut tasks: Vec<&Task> = state
        .tasks
        .values()
        .map(|rec| &rec.task)
        .filter(|task| task.spec.batch.as_deref() == Some(id.as_str()))
        .collect();
    tasks.sort_by_key(|task| task.created_at);
    let summary = backlog::summary(&tasks, &state);
    Ok(Json(BatchDetail {
        batch,
        summary,
        tasks: tasks.into_iter().cloned().collect(),
    }))
}
