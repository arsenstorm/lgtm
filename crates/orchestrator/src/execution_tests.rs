//! Unit tests for `execution.rs`: one pure fold over events.

use super::*;
use lgtm_protocol::{Executor, TaskKind, TaskResult, TaskSpec, TaskStatus, ValidationResult};

fn running() -> Task {
    Task {
        id: "0123abcd".into(),
        spec: TaskSpec {
            repository: "https://example.com/repo.git".into(),
            base_branch: "main".into(),
            prompt: "do the thing".into(),
            executor: Executor::Claude,
            runner: None,
            issue: None,
            linear: None,
            kind: TaskKind::Run,
            parent: None,
            depends_on: Vec::new(),
            depends_on_condition: Default::default(),
            batch: None,
            sandbox: None,
            requirements: Vec::new(),
            goal: None,
            review_executor: None,
            model: None,
            allowed_hosts: Vec::new(),
            session: None,
        },
        status: TaskStatus::Running,
        runner: Some("w1".into()),
        created_at: 1,
        result: None,
        error: None,
        pull_request: None,
        ci: None,
        executions: Vec::new(),
        scratchpad: String::new(),
    }
}

fn started(model: Option<&str>) -> TaskEvent {
    TaskEvent::Started {
        model: model.map(str::to_string),
    }
}

fn completed(cost: f64) -> TaskEvent {
    TaskEvent::Completed {
        result: TaskResult {
            branch: "lgtm/0123abcd".into(),
            diff: String::new(),
            changed_files: Vec::new(),
            validation: vec![ValidationResult {
                name: "test".into(),
                command: "cargo test".into(),
                ok: true,
                output_tail: String::new(),
            }],
            plan: None,
            review: None,
            policy: None,
            cost_usd: cost,
        },
    }
}

#[test]
fn started_opens_the_first_attempt() {
    let mut task = running();
    record(&mut task, &started(None), 10);
    let exec = &task.executions[0];
    assert_eq!(exec.attempt, 1);
    assert_eq!(exec.runner, "w1");
    assert_eq!(exec.executor, Executor::Claude);
    assert_eq!(exec.started_at, 10);
    assert_eq!(exec.status, ExecutionStatus::Running);
    assert!(exec.finished_at.is_none());
}

#[test]
fn started_copies_the_requested_model_onto_the_attempt() {
    let mut task = running();
    record(&mut task, &started(Some("opus")), 10);
    assert_eq!(task.executions[0].model.as_deref(), Some("opus"));
}

#[test]
fn a_second_started_stays_in_the_open_attempt() {
    let mut task = running();
    record(&mut task, &started(None), 10);
    record(&mut task, &started(None), 20);
    assert_eq!(task.executions.len(), 1);
    assert_eq!(task.executions[0].started_at, 10);
}

#[test]
fn retry_closes_the_attempt_and_the_next_started_opens_another() {
    let mut task = running();
    record(&mut task, &started(None), 10);
    record(
        &mut task,
        &TaskEvent::Retry {
            attempt: 1,
            reason: "checks failed".into(),
        },
        20,
    );
    record(&mut task, &started(None), 30);
    assert_eq!(task.executions.len(), 2);
    let first = &task.executions[0];
    assert_eq!(first.status, ExecutionStatus::Failed);
    assert_eq!(first.error.as_deref(), Some("checks failed"));
    assert_eq!(first.finished_at, Some(20));
    assert_eq!(task.executions[1].attempt, 2);
    assert_eq!(task.executions[1].status, ExecutionStatus::Running);
}

#[test]
fn completed_copies_cost_and_validation() {
    let mut task = running();
    record(&mut task, &started(None), 10);
    record(&mut task, &completed(0.42), 20);
    let exec = &task.executions[0];
    assert_eq!(exec.status, ExecutionStatus::Completed);
    assert_eq!(exec.finished_at, Some(20));
    assert_eq!(exec.cost_usd, 0.42);
    assert_eq!(exec.validation.len(), 1);
}

#[test]
fn failed_and_cancelled_close_the_running_attempt() {
    let mut task = running();
    record(&mut task, &started(None), 10);
    record(
        &mut task,
        &TaskEvent::Failed {
            error: "boom".into(),
        },
        20,
    );
    assert_eq!(task.executions[0].status, ExecutionStatus::Failed);
    assert_eq!(task.executions[0].error.as_deref(), Some("boom"));

    let mut task = running();
    record(&mut task, &started(None), 10);
    record(&mut task, &TaskEvent::Cancelled, 20);
    assert_eq!(task.executions[0].status, ExecutionStatus::Cancelled);
    assert_eq!(task.executions[0].finished_at, Some(20));
}

#[test]
fn an_end_without_an_open_attempt_records_nothing() {
    let mut task = running();
    record(&mut task, &completed(0.42), 20);
    assert!(task.executions.is_empty());

    record(&mut task, &started(None), 10);
    record(&mut task, &completed(0.42), 20);
    record(&mut task, &completed(9.99), 30);
    assert_eq!(task.executions.len(), 1);
    assert_eq!(task.executions[0].cost_usd, 0.42);
}
