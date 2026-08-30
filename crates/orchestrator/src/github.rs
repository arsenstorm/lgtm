//! GitHub side effects: opening the pull request and following its checks.
//! Every call here runs off the state lock, on its own task.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lgtm_github::Repo;
use lgtm_protocol::{CiState, CiStatus, PrReview, Task, TaskEvent, TaskId, TaskStatus};

use crate::policy::Decision;
use crate::state::{App, CmdError, PrPlan};

const CI_POLL: Duration = Duration::from_secs(60);
/// Give up after this many consecutive check failures, so a revoked token or a
/// deleted repository does not poll forever.
const CI_MAX_ERRORS: u32 = 30;

/// Opens the pull request for an approved task, then starts polling its checks.
pub fn open_pull_request(app: Arc<App>, task_id: TaskId, plan: PrPlan) {
    let Some(github) = app.github.clone() else {
        return;
    };
    tokio::spawn(async move {
        let pr = match github.create_pull(&plan.pull).await {
            Ok(pr) => pr,
            Err(err) => {
                tracing::warn!(task = %task_id, %err, "failed to open pull request");
                return record_error(&app, &task_id, format!("pull request: {err:#}"));
            }
        };
        let number = pr.number;
        tracing::info!(task = %task_id, pull = number, "pull request opened");
        {
            let mut state = app.state.lock().unwrap();
            if let Some(rec) = state.tasks.get_mut(&task_id) {
                rec.task.pull_request = Some(pr);
            }
            app.persist_ids(&mut state, std::slice::from_ref(&task_id));
        }
        crate::linear::after_transition(&app, &task_id, TaskStatus::Approved, true);
        if !plan.sha.is_empty() {
            poll_ci(app, task_id, plan.pull.repo, number, plan.sha);
        }
    });
}

fn record_error(app: &App, task_id: &TaskId, error: String) {
    let mut state = app.state.lock().unwrap();
    if let Some(rec) = state.tasks.get_mut(task_id) {
        rec.task.error = Some(error);
    }
    app.persist_ids(&mut state, std::slice::from_ref(task_id));
}

/// Why a merge could not happen: a state the task is in, or GitHub itself.
#[derive(Debug)]
pub enum MergeError {
    Cmd(CmdError),
    Github(anyhow::Error),
}

impl From<CmdError> for MergeError {
    fn from(err: CmdError) -> Self {
        MergeError::Cmd(err)
    }
}

impl fmt::Display for MergeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MergeError::Cmd(CmdError::NotFound) => write!(f, "task not found"),
            MergeError::Cmd(CmdError::Conflict(msg)) => write!(f, "{msg}"),
            MergeError::Github(err) => write!(f, "github: {err:#}"),
        }
    }
}

fn conflict(msg: impl Into<String>) -> MergeError {
    MergeError::Cmd(CmdError::Conflict(msg.into()))
}

/// Merges an approved task's pull request and records it, for the merge route
/// and for the CI poller's auto-merge alike. The GitHub calls run off the lock.
pub async fn merge_task(app: &Arc<App>, id: &str) -> Result<Task, MergeError> {
    let (github, repo, number) = mergeable(app, id)?;
    match github.pull_mergeable(&repo, number).await {
        Ok(Some(true)) => {}
        Ok(Some(false)) => return Err(conflict("pull request is not mergeable")),
        Ok(None) => return Err(conflict("pull request mergeability is unknown, retry")),
        Err(err) => return Err(MergeError::Github(err)),
    }
    github
        .merge_pull(&repo, number)
        .await
        .map_err(MergeError::Github)?;
    let task = {
        let mut state = app.state.lock().unwrap();
        let (task, changed) = state.mark_merged(id)?;
        app.persist_ids(&mut state, &changed);
        task
    };
    crate::linear::after_transition(app, id, TaskStatus::Approved, false);
    Ok(task)
}

/// The client and pull request to merge, once the task's own state allows it.
fn mergeable(app: &Arc<App>, id: &str) -> Result<(lgtm_github::GitHub, Repo, u64), MergeError> {
    let state = app.state.lock().unwrap();
    let task = state
        .tasks
        .get(id)
        .map(|rec| &rec.task)
        .ok_or(MergeError::Cmd(CmdError::NotFound))?;
    if task.status != TaskStatus::Approved {
        return Err(conflict("task is not approved"));
    }
    let pull = task
        .pull_request
        .as_ref()
        .ok_or_else(|| conflict("task has no pull request"))?;
    match task.ci.as_ref().map(|ci| ci.state) {
        Some(CiState::Success) => {}
        Some(CiState::Failure) => return Err(conflict("ci is failing")),
        Some(CiState::Pending) | None => return Err(conflict("ci is pending")),
    }
    let repo = lgtm_github::parse_repo(&task.spec.repository)
        .ok_or_else(|| conflict(format!("unrecognised repository: {}", task.spec.repository)))?;
    let github = app
        .github
        .clone()
        .ok_or_else(|| conflict("GITHUB_TOKEN is not configured"))?;
    Ok((github, repo, pull.number))
}

/// Merges a task the policy says needs no one's say-so, once its checks passed.
async fn auto_merge(app: &Arc<App>, task_id: &TaskId) {
    match merge_task(app, task_id).await {
        // `merge_task` already moved the task to `Merged`, and `AutoMerged`
        // changes no status, so the task it returned is what the webhook wants.
        Ok(task) => {
            tracing::info!(task = %task_id, "auto-merged by policy");
            {
                let mut state = app.state.lock().unwrap();
                let changed = state.apply_event(task_id, TaskEvent::AutoMerged);
                app.persist_ids(&mut state, &changed);
            }
            crate::notify::deliver(app, &task, &TaskEvent::AutoMerged);
        }
        Err(err) => {
            tracing::warn!(task = %task_id, %err, "auto-merge failed");
            record_error(app, task_id, format!("auto-merge: {err}"));
        }
    }
}

/// Follows the checks and human reviews for one pushed sha until the checks
/// settle, the task leaves `Approved`, or GitHub keeps failing.
pub fn poll_ci(app: Arc<App>, task_id: TaskId, repo: Repo, number: u64, sha: String) {
    let Some(github) = app.github.clone() else {
        return;
    };
    tokio::spawn(async move {
        let mut errors = 0u32;
        loop {
            let done = match github.checks(&repo, &sha).await {
                Ok(status) => {
                    errors = 0;
                    // A transient failure here must not read as "review cleared":
                    // `None` here means the poll learned nothing, not that GitHub
                    // reported no review.
                    let review = match github.pull_reviews(&repo, number).await {
                        Ok(review) => Some(review),
                        Err(err) => {
                            tracing::warn!(task = %task_id, %err, "failed to read pull request reviews");
                            None
                        }
                    };
                    settle(&app, &task_id, status, review).await
                }
                Err(err) => {
                    errors += 1;
                    tracing::warn!(task = %task_id, %err, "failed to read ci checks");
                    errors >= CI_MAX_ERRORS
                }
            };
            if done {
                return;
            }
            tokio::time::sleep(CI_POLL).await;
        }
    });
}

/// Records one reading of the checks and reviews and merges if policy says
/// so. `true` when polling can stop: the checks settled or the task moved on.
async fn settle(
    app: &Arc<App>,
    task_id: &TaskId,
    status: CiStatus,
    pr_review: Option<Option<PrReview>>,
) -> bool {
    let settled = status.state != CiState::Pending;
    let Some(merge) = record_ci(app, task_id, status, pr_review) else {
        return true;
    };
    if merge {
        auto_merge(app, task_id).await;
    }
    settled
}

/// Stores this reading of the checks and reviews and what policy made of the
/// checks, logging each only when it changed: both are polled in a loop.
/// `None` when the task is gone or has moved on and polling can stop.
fn record_ci(
    app: &Arc<App>,
    task_id: &TaskId,
    status: CiStatus,
    pr_review: Option<Option<PrReview>>,
) -> Option<bool> {
    let mut state = app.state.lock().unwrap();
    let rec = state.tasks.get_mut(task_id)?;
    if rec.task.status != TaskStatus::Approved {
        return None;
    }
    rec.task.ci = Some(status);
    // `Some(new_review)` only when this poll actually read the reviews and
    // that reading differs from what is stored; a review dismissed back to
    // nothing is not itself an event worth a line in the log.
    let new_review = pr_review
        .filter(|review| *review != rec.task.pr_review)
        .and_then(|review| {
            rec.task.pr_review = review.clone();
            review
        });
    let decision = crate::policy::decide(&rec.task);
    let event = decision.as_ref().map(Decision::event);
    let repeat = event.as_ref() == rec.last_policy_decision("merge");
    app.persist_ids(&mut state, std::slice::from_ref(task_id));
    if let Some(review) = new_review {
        deliver_review(app, &mut state, task_id, review);
    }
    if let Some(event) = event.filter(|_| !repeat) {
        let changed = state.apply_event(task_id, event);
        app.persist_ids(&mut state, &changed);
    }
    Some(decision.is_some_and(|decision| decision.allowed))
}

/// Applies and delivers `PrReviewed`: `attention` only speaks up for
/// `ChangesRequested`, but the log and the Overview tab want a record of an
/// approval too.
fn deliver_review(
    app: &Arc<App>,
    state: &mut crate::state::State,
    task_id: &TaskId,
    review: PrReview,
) {
    let event = TaskEvent::PrReviewed {
        state: review.state,
        url: review.url,
    };
    let changed = state.apply_event(task_id, event.clone());
    app.persist_ids(state, &changed);
    if let Some(task) = state.tasks.get(task_id).map(|rec| rec.task.clone()) {
        crate::notify::deliver(app, &task, &event);
    }
}

/// After a restart, picks up polling for tasks whose pull request is open and
/// whose checks had not settled.
pub fn resume_ci_polls(app: &Arc<App>) {
    if app.github.is_none() {
        return;
    }
    let pending: Vec<(TaskId, Repo, u64, String)> = {
        let state = app.state.lock().unwrap();
        state
            .tasks
            .values()
            .filter(|rec| {
                rec.task.status == TaskStatus::Approved
                    && rec.task.pull_request.is_some()
                    && rec
                        .task
                        .ci
                        .as_ref()
                        .is_none_or(|ci| ci.state == CiState::Pending)
            })
            .filter_map(|rec| {
                let repo = lgtm_github::parse_repo(&rec.task.spec.repository)?;
                let number = rec.task.pull_request.as_ref()?.number;
                Some((rec.task.id.clone(), repo, number, rec.pushed_sha()?))
            })
            .collect()
    };
    tracing::info!(tasks = pending.len(), "resuming ci polling");
    for (id, repo, number, sha) in pending {
        poll_ci(app.clone(), id, repo, number, sha);
    }
}
