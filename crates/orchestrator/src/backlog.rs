//! Turning a backlog of issues into tasks. Pure: the caller does the fetching,
//! the locking and the storing.

use lgtm_protocol::{
    BatchSummary, Executor, IssueRef, LinearRef, Task, TaskKind, TaskSpec, TaskStatus,
};

use crate::state::State;

/// One issue that could become a task, with what the caller needs to show it.
pub struct Candidate {
    /// `#12` for GitHub, `ENG-3` for Linear.
    pub key: String,
    pub title: String,
    pub url: String,
    pub spec: TaskSpec,
}

/// Whether two specs came from the same issue. A spec with neither reference
/// matches nothing, so hand-written tasks never hide an issue.
fn same_issue(a: &TaskSpec, b: &TaskSpec) -> bool {
    (a.issue.is_some() && a.issue == b.issue) || (a.linear.is_some() && a.linear == b.linear)
}

/// Drops candidates whose issue already has a task that has not finished, and
/// keeps the first `max` of what is left.
// ponytail: a scan of every task per candidate; both lists are small, and an
// index by issue reference is the upgrade if a backlog ever gets long.
pub fn select(existing: &[Task], candidates: Vec<Candidate>, max: u32) -> Vec<Candidate> {
    candidates
        .into_iter()
        .filter(|candidate| {
            !existing
                .iter()
                .any(|task| !task.status.is_terminal() && same_issue(&task.spec, &candidate.spec))
        })
        .take(max as usize)
        .collect()
}

fn kind(plan: bool) -> TaskKind {
    if plan {
        TaskKind::Plan
    } else {
        TaskKind::Run
    }
}

/// The task a GitHub issue would become, shaped like `POST /tasks/from-issue`.
pub fn github_candidate(
    issue: &lgtm_github::Issue,
    repo: &lgtm_github::Repo,
    base_branch: &str,
    executor: Executor,
    worker: Option<String>,
    plan: bool,
    batch: &str,
) -> Candidate {
    let number = issue.number;
    Candidate {
        key: format!("#{number}"),
        title: issue.title.clone(),
        url: issue.html_url.clone(),
        spec: TaskSpec {
            repository: format!("https://github.com/{}/{}.git", repo.owner, repo.repo),
            base_branch: base_branch.to_string(),
            prompt: format!(
                "Resolve GitHub issue #{number}: {}\n\n{}",
                issue.title, issue.body
            ),
            executor,
            worker,
            issue: Some(IssueRef {
                owner: repo.owner.clone(),
                repo: repo.repo.clone(),
                number,
            }),
            linear: None,
            kind: kind(plan),
            parent: None,
            depends_on: Vec::new(),
            batch: Some(batch.to_string()),
        },
    }
}

/// The task a Linear issue would become, shaped like `POST /tasks/from-linear`.
pub fn linear_candidate(
    issue: &lgtm_linear::Issue,
    repository: &str,
    base_branch: &str,
    executor: Executor,
    worker: Option<String>,
    plan: bool,
    batch: &str,
) -> Candidate {
    Candidate {
        key: issue.identifier.clone(),
        title: issue.title.clone(),
        url: issue.url.clone(),
        spec: TaskSpec {
            repository: repository.to_string(),
            base_branch: base_branch.to_string(),
            prompt: format!(
                "Resolve Linear issue {}: {}\n\n{}",
                issue.identifier, issue.title, issue.description
            ),
            executor,
            worker,
            issue: None,
            linear: Some(LinearRef {
                id: issue.id.clone(),
                identifier: issue.identifier.clone(),
                url: issue.url.clone(),
            }),
            kind: kind(plan),
            parent: None,
            depends_on: Vec::new(),
            batch: Some(batch.to_string()),
        },
    }
}

/// Counts `tasks` by status. A queued task waiting on a dependency counts as
/// blocked instead of queued, since nothing will pick it up yet.
pub fn summary(tasks: &[&Task], state: &State) -> BatchSummary {
    let mut out = BatchSummary::default();
    for task in tasks {
        let counter = match task.status {
            TaskStatus::Queued if task.worker.is_none() && !state.deps_met(&task.spec) => {
                &mut out.blocked
            }
            TaskStatus::Queued => &mut out.queued,
            TaskStatus::Running => &mut out.running,
            TaskStatus::AwaitingReview => &mut out.awaiting_review,
            TaskStatus::Approved => &mut out.approved,
            TaskStatus::Merged => &mut out.merged,
            TaskStatus::Failed => &mut out.failed,
            TaskStatus::Cancelled => &mut out.cancelled,
            TaskStatus::Rejected => &mut out.rejected,
        };
        *counter += 1;
    }
    out
}

#[cfg(test)]
#[path = "backlog_tests.rs"]
mod tests;
