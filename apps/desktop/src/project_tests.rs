use super::goals::counts as goal_counts;
use super::*;
use lgtm_protocol::{BatchSummary, Executor, TaskKind, TaskSpec};

fn task(repository: &str, status: TaskStatus) -> Task {
    Task {
        id: "t".into(),
        spec: TaskSpec {
            repository: repository.into(),
            base_branch: "main".into(),
            prompt: "p".into(),
            executor: Executor::Claude,
            runner: None,
            issue: None,
            linear: None,
            kind: TaskKind::Run,
            parent: None,
            depends_on: vec![],
            depends_on_condition: Default::default(),
            batch: None,
            sandbox: None,
            requirements: vec![],
            review_executor: None,
            model: None,
            goal: None,
            allowed_hosts: Vec::new(),
            session: None,
        },
        status,
        runner: None,
        created_at: 0,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        executions: Vec::new(),
        scratchpad: String::new(),
    }
}

#[test]
fn the_header_counts_running_review_and_every_flavour_of_failure() {
    let tasks = [
        task("x", TaskStatus::Running),
        task("x", TaskStatus::AwaitingReview),
        task("x", TaskStatus::Failed),
        task("x", TaskStatus::TimedOut),
        task("x", TaskStatus::RunnerLost),
        task("x", TaskStatus::Merged),
    ];

    let rows: Vec<&Task> = tasks.iter().collect();
    assert_eq!(counts(&rows), (1, 1, 3));
    assert_eq!(counts(&[]), (0, 0, 0));
}

#[test]
fn goal_counts_drop_the_empty_states_and_fold_the_lost_ones_into_failed() {
    let tasks = BatchSummary {
        running: 2,
        awaiting_review: 1,
        failed: 1,
        cancelled: 1,
        rejected: 1,
        ..BatchSummary::default()
    };
    assert_eq!(
        goal_counts(&tasks),
        vec![("running", 2), ("review", 1), ("failed", 3)]
    );
    assert!(goal_counts(&BatchSummary::default()).is_empty());
}
