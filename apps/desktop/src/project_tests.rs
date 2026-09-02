use super::goals::counts as goal_counts;
use super::overview::{numbers, Numbers};
use super::*;
use lgtm_protocol::{BatchSummary, Executor, TaskKind, TaskResult, TaskSpec, TaskStatus};

pub(super) fn task(repository: &str, status: TaskStatus) -> Task {
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
            created_by: None,
        },
        status,
        runner: None,
        created_at: 0,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        pr_review: None,
        executions: Vec::new(),
        scratchpad: String::new(),
        files: Vec::new(),
        workspace: None,
        created_by: None,
    }
}

fn cost(usd: f64) -> TaskResult {
    TaskResult {
        branch: "b".into(),
        diff: String::new(),
        changed_files: vec![],
        validation: vec![],
        plan: None,
        review: None,
        policy: None,
        cost_usd: usd,
    }
}

#[test]
fn the_strip_counts_only_the_project_it_was_given() {
    let mine = |status| task("https://x/one.git", status);
    let mut paid = mine(TaskStatus::Merged);
    paid.result = Some(cost(1.5));
    let tasks = vec![
        mine(TaskStatus::Running),
        mine(TaskStatus::AwaitingReview),
        mine(TaskStatus::Conflicted),
        paid,
        mine(TaskStatus::Approved),
        // Another project's task: never counted, whatever it is doing.
        task("https://x/two.git", TaskStatus::Running),
    ];
    let mine: Vec<&Task> = tasks
        .iter()
        .filter(|task| task.spec.repository.ends_with("one.git"))
        .collect();
    assert_eq!(
        numbers(&mine, &tasks),
        Numbers {
            running: 1,
            needs_review: 2,
            blocked: 0,
            completed: 2,
            cost_usd: 1.5,
        }
    );
    assert_eq!(numbers(&[], &tasks), Numbers::default());
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
