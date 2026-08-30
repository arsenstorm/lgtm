//! Unit tests for `stats.rs`: pure aggregation over hand-built tasks.

use super::*;
use lgtm_protocol::{ExecutionStatus, Executor, TaskKind, TaskResult, TaskSpec};

fn spec() -> TaskSpec {
    TaskSpec {
        repository: "https://example.com/repo.git".into(),
        base_branch: "main".into(),
        prompt: "do it".into(),
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
    }
}

fn task(id: &str, status: TaskStatus, created_at: u64, executions: Vec<Execution>) -> Task {
    Task {
        id: id.into(),
        spec: spec(),
        status,
        worker: None,
        created_at,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        executions,
        scratchpad: String::new(),
    }
}

fn with_cost(mut task: Task, cost_usd: f64) -> Task {
    task.result = Some(TaskResult {
        branch: "lgtm/x".into(),
        diff: String::new(),
        changed_files: Vec::new(),
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd,
    });
    task
}

fn execution(
    attempt: u32,
    worker: &str,
    executor: Executor,
    started_at: u64,
    finished_at: Option<u64>,
    status: ExecutionStatus,
) -> Execution {
    Execution {
        attempt,
        worker: worker.into(),
        executor,
        started_at,
        finished_at,
        status,
        error: None,
        cost_usd: 0.0,
        validation: Vec::new(),
    }
}

fn started(at: u64) -> StoredEvent {
    StoredEvent {
        at,
        event: TaskEvent::Started,
    }
}

#[test]
fn compute_on_no_records_is_zeroed_at_since() {
    let stats = compute(&[], 42);
    assert_eq!(
        stats,
        Stats {
            since: 42,
            ..Stats::default()
        }
    );
}

#[test]
fn compute_aggregates_every_field_across_the_window() {
    // Created before `since`: excluded, and would otherwise wreck every count.
    let before = task("before", TaskStatus::Merged, 500, Vec::new());
    let before = with_cost(before, 99.0);

    let merged = task(
        "merged",
        TaskStatus::Merged,
        1_000,
        vec![execution(
            1,
            "w1",
            Executor::Claude,
            1_000,
            Some(1_100),
            ExecutionStatus::Completed,
        )],
    );
    let merged = with_cost(merged, 0.5);
    let merged_events = [started(1_050)];

    let retried = task(
        "retried",
        TaskStatus::Failed,
        1_200,
        vec![
            execution(
                1,
                "w1",
                Executor::Claude,
                1_200,
                Some(1_400),
                ExecutionStatus::Failed,
            ),
            execution(
                2,
                "w2",
                Executor::Codex,
                1_400,
                Some(1_700),
                ExecutionStatus::Completed,
            ),
        ],
    );
    let retried = with_cost(retried, 0.3);
    let retried_events = [started(1_250), started(1_450)];

    let cancelled = task("cancelled", TaskStatus::Cancelled, 1_300, Vec::new());
    let queued = task("queued", TaskStatus::Queued, 1_400, Vec::new());

    let records: Vec<(&Task, &[StoredEvent])> = vec![
        (&before, &[]),
        (&merged, &merged_events),
        (&retried, &retried_events),
        (&cancelled, &[]),
        (&queued, &[]),
    ];

    let stats = compute(&records, 1_000);

    assert_eq!(stats.since, 1_000);
    assert_eq!(stats.tasks, 4);
    assert_eq!(stats.queued, 1);
    assert_eq!(stats.running, 0);
    assert_eq!(stats.awaiting_review, 0);
    assert_eq!(stats.approved, 0);
    assert_eq!(stats.merged, 1);
    assert_eq!(stats.failed, 1);
    assert_eq!(stats.cancelled, 1);
    assert_eq!(stats.rejected, 0);
    // Durations 100, 200, 300 -> median 200.
    assert_eq!(stats.median_execution_ms, 200);
    // Queue waits 50 and 50 (cancelled/queued never started, so excluded).
    assert_eq!(stats.median_queue_ms, 50);
    assert_eq!(stats.retried_tasks, 1);
    assert!((stats.cost_usd - 0.8).abs() < f64::EPSILON);

    let claude = stats
        .by_executor
        .iter()
        .find(|e| e.executor == Executor::Claude)
        .unwrap();
    assert_eq!(
        (claude.attempts, claude.completed, claude.failed),
        (2, 1, 1)
    );
    let codex = stats
        .by_executor
        .iter()
        .find(|e| e.executor == Executor::Codex)
        .unwrap();
    assert_eq!((codex.attempts, codex.completed, codex.failed), (1, 1, 0));
    assert_eq!(stats.by_executor[0].executor, Executor::Claude);
    assert_eq!(stats.by_executor[1].executor, Executor::Codex);
}

#[test]
fn median_handles_empty_odd_and_even() {
    assert_eq!(median(&mut []), 0);
    assert_eq!(median(&mut [5]), 5);
    assert_eq!(median(&mut [1, 3, 2]), 2);
    assert_eq!(median(&mut [1, 2, 3, 4]), 2);
}
