use super::*;
use lgtm_protocol::{
    CiStatus, Executor, Finding, Plan, PullRequest, Review, TaskResult, TaskSpec, ValidationResult,
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
            requirements: Vec::new(),
            goal: None,
            review_executor: None,
            model: None,
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
        scratchpad: String::new(),
    }
}

fn result(task: &mut Task) -> &mut TaskResult {
    task.result.as_mut().unwrap()
}

fn auto_approve() -> Option<Policy> {
    Some(Policy {
        auto_approve: true,
        ..Policy::default()
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

/// The reasons of the decision `decide` reaches, or `None`.
fn reasons(task: &Task) -> Option<(bool, Vec<String>)> {
    decide(task).map(|d| (d.allowed, d.reasons))
}

fn allowed(task: &Task) -> bool {
    decide(task).is_some_and(|d| d.allowed)
}

#[test]
fn approves_a_clean_run() {
    let task = task(TaskStatus::AwaitingReview, auto_approve());
    assert_eq!(
        reasons(&task),
        Some((
            true,
            vec!["checks passed".into(), "no blocking findings".into()]
        ))
    );
    assert_eq!(decide(&task).unwrap().action, AutoAction::Approve);

    let mut warned = task;
    result(&mut warned).review = Some(Review {
        findings: vec![finding(Severity::Warning)],
        executor: None,
    });
    assert!(allowed(&warned), "a warning is not a reason to stop");
}

#[test]
fn a_diff_limit_is_reported_even_when_it_passes() {
    let mut task = task(TaskStatus::AwaitingReview, auto_approve());
    result(&mut task).policy.as_mut().unwrap().max_diff_lines = Some(10);
    result(&mut task).diff = "--- a\n+++ b\n+one\n-two\n context\n".into();
    assert_eq!(
        reasons(&task),
        Some((
            true,
            vec![
                "checks passed".into(),
                "no blocking findings".into(),
                "2 lines".into()
            ]
        ))
    );
}

#[test]
fn every_reason_to_refuse_is_recorded() {
    let mut task = task(TaskStatus::AwaitingReview, auto_approve());
    let policy = result(&mut task).policy.as_mut().unwrap();
    policy.max_diff_lines = Some(1);
    policy.protected_files = vec!["migrations/*".into()];
    policy.budget_per_task_usd = Some(2.0);
    let res = result(&mut task);
    res.diff = "+one\n-two\n".into();
    res.changed_files = vec!["a.rs".into(), "migrations/001.sql".into()];
    res.cost_usd = 3.5;
    res.validation = vec![ValidationResult {
        name: "test".into(),
        command: "cargo test".into(),
        ok: false,
        output_tail: "1 failed".into(),
    }];
    res.review = Some(Review {
        findings: vec![finding(Severity::Warning), finding(Severity::Blocking)],
        executor: None,
    });
    assert_eq!(
        reasons(&task),
        Some((
            false,
            vec![
                "check test failed".into(),
                "blocking review finding: look at this".into(),
                "diff is 2 lines, limit 1".into(),
                "touches protected file migrations/001.sql".into(),
                "cost $3.50 over budget $2.00".into(),
            ]
        ))
    );
}

#[test]
fn a_follow_up_is_not_a_clean_run() {
    assert_eq!(
        decide(&task(TaskStatus::ChangesRequested, auto_approve())),
        None
    );
}

#[test]
fn policy_says_nothing_without_one() {
    assert_eq!(decide(&task(TaskStatus::AwaitingReview, None)), None);

    let mut off = task(TaskStatus::AwaitingReview, auto_approve());
    result(&mut off).policy = Some(Policy::default());
    assert_eq!(decide(&off), None);

    let mut no_result = task(TaskStatus::AwaitingReview, auto_approve());
    no_result.result = None;
    assert_eq!(decide(&no_result), None);

    let mut plan = task(TaskStatus::AwaitingReview, auto_approve());
    plan.spec.kind = TaskKind::Plan;
    result(&mut plan).plan = Some(Plan { steps: Vec::new() });
    assert_eq!(decide(&plan), None, "a plan is approved by hand");
}

/// An approved task with `auto_merge` on, a pull request and CI in `state`.
fn mergeable(state: Option<CiState>) -> Task {
    let mut task = task(
        TaskStatus::Approved,
        Some(Policy {
            auto_merge: true,
            ..Policy::default()
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
        reasons(&mergeable(Some(CiState::Success))),
        Some((true, vec!["ci success".into()]))
    );
    assert_eq!(
        reasons(&mergeable(Some(CiState::Failure))),
        Some((false, vec!["ci failure".into()]))
    );
    assert_eq!(
        decide(&mergeable(Some(CiState::Pending))),
        None,
        "pending is not a refusal, it is not decided yet"
    );
    assert_eq!(decide(&mergeable(None)), None);

    let mut no_pull = mergeable(Some(CiState::Success));
    no_pull.pull_request = None;
    assert_eq!(decide(&no_pull), None);

    let mut awaiting = mergeable(Some(CiState::Success));
    awaiting.status = TaskStatus::AwaitingReview;
    assert_eq!(decide(&awaiting), None, "auto_merge alone does not approve");
}

#[test]
fn diff_lines_ignores_the_file_headers() {
    assert_eq!(diff_lines("--- a/x\n+++ b/x\n+one\n-two\n keep\n"), 2);
    assert_eq!(diff_lines(""), 0);
}

#[test]
fn star_matches_any_run_of_characters() {
    assert!(glob_match("migrations/*", "migrations/001.sql"));
    assert!(glob_match("migrations/*", "migrations/"));
    assert!(!glob_match("migrations/*", "src/migrations/001.sql"));
    assert!(glob_match("Cargo.lock", "Cargo.lock"));
    assert!(!glob_match("Cargo.lock", "crates/Cargo.lock"));
    assert!(glob_match("*.lock", "a/b.lock"));
    assert!(!glob_match("*.lock", "a/b.toml"));
    assert!(glob_match("*", "anything/at/all"));
    assert!(glob_match("a*c*e", "abcde"));
    assert!(!glob_match("a*c*e", "abcd"));
}
