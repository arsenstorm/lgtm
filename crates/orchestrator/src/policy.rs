//! What the repository's `[policy]` lets the orchestrator do on its own. Pure:
//! the callers do the storing, the sending and the GitHub calls.

use lgtm_protocol::{CiState, Task, TaskKind, TaskResult, TaskStatus};

/// Checks passed and the reviewer, if any, found nothing blocking.
fn clean(result: &TaskResult) -> bool {
    !result.validation_failed()
        && result
            .review
            .as_ref()
            .is_none_or(|review| !review.has_blocking())
}

#[derive(Debug, PartialEq, Eq)]
pub enum AutoAction {
    Approve,
    Merge,
}

/// What policy allows right now for `task`, if anything.
pub fn auto_action(task: &Task) -> Option<AutoAction> {
    let result = task.result.as_ref()?;
    let policy = result.policy?;
    match task.status {
        // A plan is approved by creating its steps, which is a decision
        // about work, not a diff to wave through.
        TaskStatus::AwaitingReview
            if policy.auto_approve && task.spec.kind == TaskKind::Run && clean(result) =>
        {
            Some(AutoAction::Approve)
        }
        TaskStatus::Approved
            if policy.auto_merge
                && task.pull_request.is_some()
                && task.ci.as_ref().map(|ci| ci.state) == Some(CiState::Success) =>
        {
            Some(AutoAction::Merge)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{
        CiStatus, Executor, Finding, Plan, Policy, PullRequest, Review, Severity, TaskResult,
        TaskSpec, ValidationResult,
    };

    fn task(status: TaskStatus, policy: Option<Policy>) -> Task {
        Task {
            id: "0123abcd".into(),
            spec: TaskSpec {
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                base_branch: "main".into(),
                prompt: "do the thing".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: Vec::new(),
                batch: None,
                sandbox: None,
                goal: None,
            },
            status,
            worker: Some("w".into()),
            created_at: 1,
            result: Some(TaskResult {
                branch: "lgtm/0123abcd".into(),
                diff: "diff".into(),
                changed_files: vec!["a.rs".into()],
                validation: Vec::new(),
                plan: None,
                review: None,
                policy,
                cost_usd: 0.0,
            }),
            error: None,
            pull_request: None,
            ci: None,
            executions: Vec::new(),
        }
    }

    fn result(task: &mut Task) -> &mut TaskResult {
        task.result.as_mut().unwrap()
    }

    fn auto_approve() -> Option<Policy> {
        Some(Policy {
            auto_approve: true,
            auto_merge: false,
        })
    }

    fn finding(severity: Severity) -> Finding {
        Finding {
            severity,
            file: "a.rs".into(),
            line: Some(1),
            message: "look at this".into(),
        }
    }

    #[test]
    fn approves_a_clean_run() {
        let task = task(TaskStatus::AwaitingReview, auto_approve());
        assert_eq!(auto_action(&task), Some(AutoAction::Approve));

        let mut warned = task;
        result(&mut warned).review = Some(Review {
            findings: vec![finding(Severity::Warning)],
        });
        assert_eq!(
            auto_action(&warned),
            Some(AutoAction::Approve),
            "a warning is not a reason to stop"
        );
    }

    #[test]
    fn holds_back_anything_a_human_should_see() {
        let mut blocking = task(TaskStatus::AwaitingReview, auto_approve());
        result(&mut blocking).review = Some(Review {
            findings: vec![finding(Severity::Warning), finding(Severity::Blocking)],
        });
        assert_eq!(auto_action(&blocking), None);

        let mut failed = task(TaskStatus::AwaitingReview, auto_approve());
        result(&mut failed).validation = vec![ValidationResult {
            name: "test".into(),
            command: "cargo test".into(),
            ok: false,
            output_tail: "1 failed".into(),
        }];
        assert_eq!(auto_action(&failed), None);

        let mut plan = task(TaskStatus::AwaitingReview, auto_approve());
        plan.spec.kind = TaskKind::Plan;
        result(&mut plan).plan = Some(Plan { steps: Vec::new() });
        assert_eq!(auto_action(&plan), None, "a plan is approved by hand");

        assert_eq!(auto_action(&task(TaskStatus::AwaitingReview, None)), None);
        let mut off = task(TaskStatus::AwaitingReview, auto_approve());
        result(&mut off).policy = Some(Policy::default());
        assert_eq!(auto_action(&off), None);

        let mut no_result = task(TaskStatus::AwaitingReview, auto_approve());
        no_result.result = None;
        assert_eq!(auto_action(&no_result), None);
    }

    /// An approved task with `auto_merge` on, a pull request and CI in `state`.
    fn mergeable(state: Option<CiState>) -> Task {
        let mut task = task(
            TaskStatus::Approved,
            Some(Policy {
                auto_approve: false,
                auto_merge: true,
            }),
        );
        task.pull_request = Some(PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        });
        task.ci = state.map(|state| CiStatus {
            state,
            url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
        });
        task
    }

    #[test]
    fn merges_only_once_ci_is_green() {
        assert_eq!(
            auto_action(&mergeable(Some(CiState::Success))),
            Some(AutoAction::Merge)
        );
        assert_eq!(auto_action(&mergeable(Some(CiState::Pending))), None);
        assert_eq!(auto_action(&mergeable(Some(CiState::Failure))), None);
        assert_eq!(auto_action(&mergeable(None)), None);

        let mut no_pull = mergeable(Some(CiState::Success));
        no_pull.pull_request = None;
        assert_eq!(auto_action(&no_pull), None);

        let mut awaiting = mergeable(Some(CiState::Success));
        awaiting.status = TaskStatus::AwaitingReview;
        assert_eq!(
            auto_action(&awaiting),
            None,
            "auto_merge alone does not approve"
        );
    }
}
