//! GitHub side effects: opening the pull request and following its checks.
//! Every call here runs off the state lock, on its own task.

use std::sync::Arc;
use std::time::Duration;

use lgtm_github::Repo;
use lgtm_protocol::{CiState, TaskId, TaskStatus};

use crate::state::{App, PrPlan};

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
        let opened = github
            .create_pull(&plan.repo, &plan.head, &plan.base, &plan.title, &plan.body)
            .await;
        match opened {
            Ok(pr) => {
                tracing::info!(task = %task_id, pull = pr.number, "pull request opened");
                {
                    let mut state = app.state.lock().unwrap();
                    if let Some(rec) = state.tasks.get_mut(&task_id) {
                        rec.task.pull_request = Some(pr);
                    }
                    app.persist_ids(&state, std::slice::from_ref(&task_id));
                }
                if !plan.sha.is_empty() {
                    poll_ci(app, task_id, plan.repo, plan.sha);
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
                    {
                        let mut state = app.state.lock().unwrap();
                        let Some(rec) = state.tasks.get_mut(&task_id) else {
                            return;
                        };
                        if rec.task.status != TaskStatus::Approved {
                            return;
                        }
                        rec.task.ci = Some(status);
                        app.persist_ids(&state, std::slice::from_ref(&task_id));
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
