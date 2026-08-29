//! GitHub side effects: opening the pull request and following its checks.
//! Every call here runs off the state lock, on its own task.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use lgtm_github::Repo;
use lgtm_protocol::{CiState, Task, TaskEvent, TaskId, TaskStatus};

use crate::policy::{auto_action, AutoAction};
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
        match github.create_pull(&plan.pull).await {
            Ok(pr) => {
                tracing::info!(task = %task_id, pull = pr.number, "pull request opened");
                {
                    let mut state = app.state.lock().unwrap();
                    if let Some(rec) = state.tasks.get_mut(&task_id) {
                        rec.task.pull_request = Some(pr);
                    }
                    app.persist_ids(&state, std::slice::from_ref(&task_id));
                }
                crate::linear::after_transition(&app, &task_id, TaskStatus::Approved, true);
                if !plan.sha.is_empty() {
                    poll_ci(app, task_id, plan.pull.repo, plan.sha);
                }
            }
            Err(err) => {
                tracing::warn!(task = %task_id, %err, "failed to open pull request");
                let mut state = app.state.lock().unwrap();
                if let Some(rec) = state.tasks.get_mut(&task_id) {
                    rec.task.error = Some(format!("pull request: {err:#}"));
                }
                app.persist_ids(&state, std::slice::from_ref(&task_id));
            }
        }
    });
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
    let (github, repo, number) = {
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
        let repo = lgtm_github::parse_repo(&task.spec.repository).ok_or_else(|| {
            conflict(format!("unrecognised repository: {}", task.spec.repository))
        })?;
        let github = app
            .github
            .clone()
            .ok_or_else(|| conflict("GITHUB_TOKEN is not configured"))?;
        (github, repo, pull.number)
    };
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
        app.persist_ids(&state, &changed);
        task
    };
    crate::linear::after_transition(app, id, TaskStatus::Approved, false);
    Ok(task)
}

/// Merges a task the policy says needs no one's say-so, once its checks passed.
async fn auto_merge(app: &Arc<App>, task_id: &TaskId) {
    match merge_task(app, task_id).await {
        Ok(_) => {
            tracing::info!(task = %task_id, "auto-merged by policy");
            let mut state = app.state.lock().unwrap();
            let changed = state.apply_event(task_id, TaskEvent::AutoMerged);
            app.persist_ids(&state, &changed);
        }
        Err(err) => {
            tracing::warn!(task = %task_id, %err, "auto-merge failed");
            let mut state = app.state.lock().unwrap();
            if let Some(rec) = state.tasks.get_mut(task_id) {
                rec.task.error = Some(format!("auto-merge: {err}"));
            }
            app.persist_ids(&state, std::slice::from_ref(task_id));
        }
    }
}

/// Follows the checks for one pushed sha until they settle, the task leaves
/// `Approved`, or GitHub keeps failing.
pub fn poll_ci(app: Arc<App>, task_id: TaskId, repo: Repo, sha: String) {
    let Some(github) = app.github.clone() else {
        return;
    };
    tokio::spawn(async move {
        let mut errors = 0u32;
        loop {
            match github.checks(&repo, &sha).await {
                Ok(status) => {
                    errors = 0;
                    let settled = status.state != CiState::Pending;
                    let merge = {
                        let mut state = app.state.lock().unwrap();
                        let Some(rec) = state.tasks.get_mut(&task_id) else {
                            return;
                        };
                        if rec.task.status != TaskStatus::Approved {
                            return;
                        }
                        rec.task.ci = Some(status);
                        let merge = auto_action(&rec.task) == Some(AutoAction::Merge);
                        app.persist_ids(&state, std::slice::from_ref(&task_id));
                        merge
                    };
                    if merge {
                        auto_merge(&app, &task_id).await;
                    }
                    if settled {
                        return;
                    }
                }
                Err(err) => {
                    errors += 1;
                    tracing::warn!(task = %task_id, %err, "failed to read ci checks");
                    if errors >= CI_MAX_ERRORS {
                        return;
                    }
                }
            }
            tokio::time::sleep(CI_POLL).await;
        }
    });
}

/// After a restart, picks up polling for tasks whose pull request is open and
/// whose checks had not settled.
pub fn resume_ci_polls(app: &Arc<App>) {
    if app.github.is_none() {
        return;
    }
    let pending: Vec<(TaskId, Repo, String)> = {
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
                Some((rec.task.id.clone(), repo, rec.pushed_sha()?))
            })
            .collect()
    };
    tracing::info!(tasks = pending.len(), "resuming ci polling");
    for (id, repo, sha) in pending {
        poll_ci(app.clone(), id, repo, sha);
    }
}
