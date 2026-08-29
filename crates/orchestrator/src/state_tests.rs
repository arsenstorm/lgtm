//! Unit tests for `state.rs`: pure transitions, no sockets and no files.

use super::*;
use lgtm_protocol::{Executor, TaskResult};

fn info(name: &str, slots: u32, executors: Vec<Executor>) -> WorkerInfo {
    WorkerInfo {
        name: name.to_string(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors,
        slots,
    }
}

/// Connects a worker over the same path `Hello` uses. The receiver comes back
/// so the caller can keep it alive and read what the worker was sent.
fn connect(
    state: &mut State,
    name: &str,
    slots: u32,
    conn_id: u64,
) -> mpsc::UnboundedReceiver<OrchestratorMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.worker_hello(
        info(name, slots, vec![Executor::Claude]),
        Vec::new(),
        Conn { tx, conn_id },
    );
    rx
}

fn spec(executor: Executor, worker: Option<&str>) -> TaskSpec {
    TaskSpec {
        repository: "https://example.com/repo.git".into(),
        base_branch: "main".into(),
        prompt: "do the thing".into(),
        executor,
        worker: worker.map(str::to_string),
    }
}

fn create(state: &mut State, executor: Executor) -> Task {
    state.create_task(spec(executor, None)).unwrap().0
}

fn status(state: &State, id: &str) -> TaskStatus {
    state.tasks[id].task.status
}

#[test]
fn refuses_tasks_no_worker_could_run() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let (tx, _codex) = mpsc::unbounded_channel();
    state.worker_hello(
        info("codexonly", 1, vec![Executor::Codex]),
        Vec::new(),
        Conn { tx, conn_id: 2 },
    );

    assert!(state.check_eligible(&spec(Executor::Claude, None)).is_ok());
    assert_eq!(state.candidate(&spec(Executor::Claude, None)).unwrap(), "a");
    // The only Codex worker wins despite the Claude worker being idle.
    assert_eq!(
        state.candidate(&spec(Executor::Codex, None)).unwrap(),
        "codexonly"
    );
    assert_eq!(
        state
            .check_eligible(&spec(Executor::Claude, Some("codexonly")))
            .unwrap_err(),
        "worker codexonly does not have claude"
    );
    assert_eq!(
        state
            .check_eligible(&spec(Executor::Claude, Some("ghost")))
            .unwrap_err(),
        "worker ghost is not connected"
    );
    state.workers.clear();
    assert_eq!(
        state
            .check_eligible(&spec(Executor::Claude, None))
            .unwrap_err(),
        "no eligible worker"
    );
}

#[test]
fn fifo_assignment_respects_slots() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let _b = connect(&mut state, "b", 2, 2);

    let one = create(&mut state, Executor::Claude);
    let two = create(&mut state, Executor::Claude);
    let three = create(&mut state, Executor::Claude);
    let four = create(&mut state, Executor::Claude);

    assert_eq!(
        one.worker.as_deref(),
        Some("b"),
        "b has the most free slots"
    );
    assert_eq!(
        two.worker.as_deref(),
        Some("a"),
        "one free each, a sorts first"
    );
    assert_eq!(three.worker.as_deref(), Some("b"));
    assert_eq!(four.worker, None);
    assert_eq!(status(&state, &four.id), TaskStatus::Queued);
    assert_eq!(state.workers["a"].running.len(), 1);
    assert_eq!(state.workers["b"].running.len(), 2);
}

#[test]
fn queued_task_assigned_on_connect() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    create(&mut state, Executor::Claude);
    let queued = create(&mut state, Executor::Claude);
    assert_eq!(queued.worker, None);

    let mut b = connect(&mut state, "b", 2, 2);
    assert_eq!(
        state.tasks[&queued.id].task.worker.as_deref(),
        Some("b"),
        "the new worker picks up the backlog"
    );
    assert!(matches!(
        b.try_recv().unwrap(),
        OrchestratorMessage::Start { task } if task.id == queued.id
    ));
}

#[test]
fn reconnect_within_grace_keeps_tasks() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(&task.id, TaskEvent::Started);

    let stale = state.disconnect("a", 1).unwrap();
    assert!(!state.workers["a"].is_connected());
    assert_eq!(status(&state, &task.id), TaskStatus::Running);

    let (tx, _rx) = mpsc::unbounded_channel();
    state.worker_hello(
        info("a", 1, vec![Executor::Claude]),
        vec![task.id.clone()],
        Conn { tx, conn_id: 2 },
    );
    assert_eq!(status(&state, &task.id), TaskStatus::Running);
    assert!(state.workers["a"].running.contains(&task.id));

    state.expire_worker("a", stale);
    assert!(state.workers.contains_key("a"), "the old timer is a no-op");
    assert_eq!(status(&state, &task.id), TaskStatus::Running);
}

#[test]
fn reconnect_missing_task_is_lost() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(&task.id, TaskEvent::Started);
    state.disconnect("a", 1).unwrap();

    let (tx, _rx) = mpsc::unbounded_channel();
    let changed = state.worker_hello(
        info("a", 1, vec![Executor::Claude]),
        Vec::new(),
        Conn { tx, conn_id: 2 },
    );
    assert!(changed.contains(&task.id));
    assert_eq!(status(&state, &task.id), TaskStatus::Failed);
    assert_eq!(
        state.tasks[&task.id].task.error.as_deref(),
        Some("lost on worker")
    );
    assert!(state.workers["a"].running.is_empty());
}

#[test]
fn grace_expiry_fails_tasks() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let task = create(&mut state, Executor::Claude);
    state.apply_event(&task.id, TaskEvent::Started);

    let generation = state.disconnect("a", 1).unwrap();
    let changed = state.expire_worker("a", generation);
    assert_eq!(changed, vec![task.id.clone()]);
    assert_eq!(status(&state, &task.id), TaskStatus::Failed);
    assert_eq!(
        state.tasks[&task.id].task.error.as_deref(),
        Some("worker disconnected")
    );
    assert!(!state.workers.contains_key("a"));
}

#[test]
fn cancel_queued_task() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    create(&mut state, Executor::Claude);
    let queued = create(&mut state, Executor::Claude);

    let cancelled = state.cancel(&queued.id).unwrap();
    assert_eq!(cancelled.status, TaskStatus::Cancelled);
    assert_eq!(status(&state, &queued.id), TaskStatus::Cancelled);
}

#[test]
fn apply_event_transitions() {
    let mut state = State::default();
    let _idle = connect(&mut state, "idle", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    assert!(state.workers["idle"].running.contains(&id));

    state.apply_event(&id, TaskEvent::Started);
    assert_eq!(status(&state, &id), TaskStatus::Running);

    let result = TaskResult {
        branch: format!("lgtm/{id}"),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
    };
    state.apply_event(&id, TaskEvent::Completed { result });
    assert_eq!(status(&state, &id), TaskStatus::AwaitingReview);
    assert!(state.tasks[&id].task.result.is_some());
    assert!(state.workers["idle"].running.is_empty());

    state.apply_event(
        &id,
        TaskEvent::Pushed {
            branch: format!("lgtm/{id}"),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Approved);
    assert_eq!(state.tasks[&id].events.len(), 3);
}

#[test]
fn message_requires_awaiting_review() {
    let mut state = State::default();
    let _a = connect(&mut state, "a", 1, 1);
    let id = create(&mut state, Executor::Claude).id;
    state.apply_event(&id, TaskEvent::Started);
    assert_eq!(status(&state, &id), TaskStatus::Running);

    assert!(matches!(
        state.message(&id, "too soon".into()),
        Err(CmdError::Conflict(_))
    ));

    let result = TaskResult {
        branch: format!("lgtm/{id}"),
        diff: "diff".into(),
        changed_files: vec!["a.rs".into()],
        validation: Vec::new(),
    };
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: result.clone(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::AwaitingReview);
    assert!(
        state.workers["a"].running.is_empty(),
        "slot freed on completion"
    );

    let (task, changed) = state.message(&id, "keep going".into()).unwrap();
    assert_eq!(task.status, TaskStatus::AwaitingReview);
    assert!(changed.contains(&id));
    match &state.tasks[&id].events.last().unwrap().event {
        TaskEvent::Message { text } => assert_eq!(text.as_str(), "keep going"),
        other => panic!("expected a Message event, got {other:?}"),
    }
    assert!(
        state.workers["a"].running.contains(&id),
        "slot taken again for the follow-up"
    );

    state.apply_event(&id, TaskEvent::Started);
    assert_eq!(status(&state, &id), TaskStatus::Running);

    state.apply_event(&id, TaskEvent::Completed { result });
    assert_eq!(status(&state, &id), TaskStatus::AwaitingReview);
    assert!(
        state.workers["a"].running.is_empty(),
        "slot freed again after the follow-up run"
    );
}

#[test]
fn terminal_status_survives_late_events() {
    let mut state = State::default();
    let _idle = connect(&mut state, "idle", 1, 1);
    let id = create(&mut state, Executor::Claude).id;

    state.apply_event(&id, TaskEvent::Cancelled);
    assert_eq!(status(&state, &id), TaskStatus::Cancelled);

    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "worker disconnected".into(),
        },
    );
    assert_eq!(status(&state, &id), TaskStatus::Cancelled);
    assert!(state.tasks[&id].task.error.is_none());
    assert_eq!(state.tasks[&id].events.len(), 2);
}
