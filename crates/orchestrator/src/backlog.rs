//! Turning a backlog of issues into tasks. Pure: the caller does the fetching,
//! the locking and the storing.

use lgtm_protocol::{
    BatchSummary, Executor, IssueRef, LinearRef, SandboxProfile, Task, TaskKind, TaskSpec,
    TaskStatus,
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

/// What every task made from an issue shares, whichever source it came from.
#[derive(Clone)]
pub struct SpecInput {
    pub base_branch: String,
    pub executor: Executor,
    pub worker: Option<String>,
    pub kind: TaskKind,
    pub batch: Option<String>,
    pub sandbox: Option<SandboxProfile>,
}

impl SpecInput {
    fn spec(self, repository: String, prompt: String) -> TaskSpec {
        TaskSpec {
            repository,
            base_branch: self.base_branch,
            prompt,
            executor: self.executor,
            worker: self.worker,
            issue: None,
            linear: None,
            kind: self.kind,
            parent: None,
            depends_on: Vec::new(),
            batch: self.batch,
            sandbox: self.sandbox,
            goal: None,
        }
    }
}

/// The task a GitHub issue would become, shaped like `POST /tasks/from-issue`.
pub fn github_candidate(
    issue: &lgtm_github::Issue,
    repo: &lgtm_github::Repo,
    input: SpecInput,
) -> Candidate {
    let number = issue.number;
    let prompt = format!(
        "Resolve GitHub issue #{number}: {}\n\n{}",
        issue.title, issue.body
    );
    let repository = format!("https://github.com/{}/{}.git", repo.owner, repo.repo);
    let mut spec = input.spec(repository, prompt);
    spec.issue = Some(IssueRef {
        owner: repo.owner.clone(),
        repo: repo.repo.clone(),
        number,
    });
    Candidate {
        key: format!("#{number}"),
        title: issue.title.clone(),
        url: issue.html_url.clone(),
        spec,
    }
}

/// The task a Linear issue would become, shaped like `POST /tasks/from-linear`.
pub fn linear_candidate(
    issue: &lgtm_linear::Issue,
    repository: &str,
    input: SpecInput,
) -> Candidate {
    let prompt = format!(
        "Resolve Linear issue {}: {}\n\n{}",
        issue.identifier, issue.title, issue.description
    );
    let mut spec = input.spec(repository.to_string(), prompt);
    spec.linear = Some(LinearRef {
        id: issue.id.clone(),
        identifier: issue.identifier.clone(),
        url: issue.url.clone(),
    });
    Candidate {
        key: issue.identifier.clone(),
        title: issue.title.clone(),
        url: issue.url.clone(),
        spec,
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
            // Both are failures a person has to look at; a separate column
            // earns nothing yet.
            TaskStatus::Failed | TaskStatus::TimedOut | TaskStatus::RunnerLost => &mut out.failed,
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
