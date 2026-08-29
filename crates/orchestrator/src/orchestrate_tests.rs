//! Unit tests for `orchestrate.rs`: reading a decision, and what LGTM will
//! and will not do with one. No model and no sockets.

use super::*;
use lgtm_protocol::{TaskResult, ValidationResult, WorkerInfo};
use tokio::sync::mpsc;

use crate::state::{Conn, TaskRecord};

fn connect(state: &mut State) -> mpsc::UnboundedReceiver<OrchestratorMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    state.worker_hello(
        WorkerInfo {
            name: "w".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            executors: vec![Executor::Claude],
            slots: 4,
            ephemeral: false,
            capabilities: Vec::new(),
        },
        Vec::new(),
        Conn { tx, conn_id: 1 },
    );
    rx
}

fn spec(goal: Option<String>) -> TaskSpec {
    TaskSpec {
        repository: "https://example.com/repo.git".into(),
        base_branch: "main".into(),
        prompt: "do the thing\nin detail".into(),
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
        review_executor: None,
        model: None,
        goal,
    }
}

/// A connected worker, a goal, and one task under it.
fn goal_task(state: &mut State) -> (String, TaskId) {
    let goal = state.create_goal(
        "ship the thing".into(),
        "https://example.com/repo.git".into(),
    );
    let task = state.create_task(spec(Some(goal.id.clone()))).unwrap().0;
    (goal.id, task.id)
}

fn result(ok: bool) -> TaskResult {
    TaskResult {
        branch: "lgtm/x".into(),
        diff: "+ a\n".into(),
        changed_files: vec!["a.rs".into()],
        validation: vec![ValidationResult {
            name: "test".into(),
            command: "cargo test".into(),
            ok,
            output_tail: String::new(),
        }],
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.5,
    }
}

#[test]
fn reads_every_decision_shape() {
    let block = |json: &str| format!("Here is my call.\n```json\n{json}\n```\n");
    assert_eq!(
        parse_decision(&block(r#"{"action":"approve","reason":"clean"}"#)).unwrap(),
        Decision::Approve {
            reason: "clean".into()
        }
    );
    assert_eq!(
        parse_decision(&block(r#"{"action":"retry","reason":"crashed"}"#)).unwrap(),
        Decision::Retry {
            reason: "crashed".into()
        }
    );
    assert_eq!(
        parse_decision(&block(
            r#"{"action":"message","text":"fix it","reason":"finding"}"#
        ))
        .unwrap(),
        Decision::Message {
            text: "fix it".into(),
            reason: "finding".into()
        }
    );
    assert_eq!(
        parse_decision(&block(
            r#"{"action":"create_task","title":"T","prompt":"P","reason":"gap"}"#
        ))
        .unwrap(),
        Decision::CreateTask {
            title: "T".into(),
            prompt: "P".into(),
            depends_on: Vec::new(),
            reason: "gap".into()
        }
    );
    assert_eq!(
        parse_decision(&block(
            r#"{"action":"wait","reason":"a person should look"}"#
        ))
        .unwrap(),
        Decision::Wait {
            reason: "a person should look".into()
        }
    );
}

#[test]
fn refuses_an_answer_that_is_not_a_decision() {
    let err = parse_decision("I think we should ship it").unwrap_err();
    assert!(err.contains("was not a decision"), "{err}");
    assert!(parse_decision(
        r#"```json
{"action":"merge","reason":"why not"}
```"#
    )
    .is_err());
}

#[test]
fn will_not_approve_a_task_whose_checks_failed() {
    let mut state = State::default();
    let _worker = connect(&mut state);
    let (_goal, id) = goal_task(&mut state);
    state.apply_event(&id, TaskEvent::Started { model: None });
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: result(false),
        },
    );

    let refused = apply(
        &mut state,
        &id,
        &Decision::Approve {
            reason: "looks fine".into(),
        },
        None,
    )
    .unwrap_err();
    assert_eq!(refused, "checks failed");
    assert_eq!(state.tasks[&id].task.status, TaskStatus::AwaitingReview);

    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: result(true),
        },
    );
    assert!(apply(
        &mut state,
        &id,
        &Decision::Approve {
            reason: "clean".into()
        },
        None,
    )
    .is_ok());
}

#[test]
fn retries_a_failed_task() {
    let mut state = State::default();
    let _worker = connect(&mut state);
    let (_goal, id) = goal_task(&mut state);
    state.apply_event(
        &id,
        TaskEvent::Failed {
            error: "boom".into(),
        },
    );

    apply(
        &mut state,
        &id,
        &Decision::Retry {
            reason: "crashed".into(),
        },
        None,
    )
    .unwrap();
    assert_eq!(state.tasks[&id].task.status, TaskStatus::Queued);
    assert!(state.tasks[&id].task.error.is_none());
}

#[test]
fn will_not_depend_a_new_task_on_a_task_outside_the_goal() {
    let mut state = State::default();
    let _worker = connect(&mut state);
    let (_goal, id) = goal_task(&mut state);
    let outside = state.create_task(spec(None)).unwrap().0;

    let decision = |depends_on: Vec<TaskId>| Decision::CreateTask {
        title: "Add the docs".into(),
        prompt: "Write them.".into(),
        depends_on,
        reason: "the goal needs docs".into(),
    };
    let refused = apply(&mut state, &id, &decision(vec![outside.id.clone()]), None).unwrap_err();
    assert!(refused.contains("not a task under this goal"), "{refused}");

    apply(&mut state, &id, &decision(vec![id.clone()]), None).unwrap();
    let created = state
        .tasks
        .values()
        .find(|rec| rec.task.spec.prompt.starts_with("Add the docs"))
        .expect("the new task");
    assert_eq!(created.task.spec.goal, state.tasks[&id].task.spec.goal);
    assert_eq!(created.task.spec.depends_on, vec![id]);
}

#[test]
fn waiting_changes_nothing() {
    let mut state = State::default();
    let _worker = connect(&mut state);
    let (_goal, id) = goal_task(&mut state);
    let before = state.tasks[&id].task.status;

    let changed = apply(
        &mut state,
        &id,
        &Decision::Wait {
            reason: "a person should look".into(),
        },
        None,
    )
    .unwrap();
    assert!(changed.is_empty());
    assert_eq!(state.tasks[&id].task.status, before);
}

#[test]
fn the_prompt_carries_the_goal_the_subject_and_the_shapes() {
    let mut state = State::default();
    let _worker = connect(&mut state);
    let (_goal, id) = goal_task(&mut state);
    state.apply_event(&id, TaskEvent::Started { model: None });
    state.apply_event(
        &id,
        TaskEvent::Command {
            command: "cargo test".into(),
        },
    );
    state.apply_event(
        &id,
        TaskEvent::Completed {
            result: result(false),
        },
    );
    state.memories.insert(
        "m".into(),
        Memory {
            id: "m".into(),
            repository: None,
            content: "the tests are slow".into(),
            created_at: 1,
        },
    );

    let text = prompt(&build_context(&state, &id).expect("a context"));
    assert!(text.contains("ship the thing"), "{text}");
    assert!(
        text.contains(&format!("- {id} [awaiting_review]")),
        "{text}"
    );
    assert!(text.contains("check test failed"), "{text}");
    assert!(text.contains("$ cargo test"), "{text}");
    assert!(text.contains("the tests are slow"), "{text}");
    for shape in [
        r#""action": "approve""#,
        r#""action": "retry""#,
        r#""action": "message""#,
        r#""action": "create_task""#,
        r#""action": "wait""#,
    ] {
        assert!(text.contains(shape), "{shape} missing from {text}");
    }
}

#[test]
fn a_task_without_a_goal_has_no_context() {
    let mut state = State::default();
    let _worker = connect(&mut state);
    let task = state.create_task(spec(None)).unwrap().0;
    assert!(build_context(&state, &task.id).is_none());
    assert!(build_context(&state, "nothing").is_none());
    // A goal that was removed leaves its tasks alone too.
    let orphan = TaskRecord::new(
        Task {
            spec: spec(Some("gone".into())),
            ..task
        },
        Vec::new(),
    );
    let id = orphan.task.id.clone();
    state.tasks.insert(id.clone(), orphan);
    assert!(build_context(&state, &id).is_none());
}

#[test]
fn reads_the_answer_out_of_each_executor() {
    let claude = r#"{"type":"result","result":"```json\n{\"action\":\"wait\"}\n```"}"#;
    assert!(answer(Executor::Claude, claude).unwrap().contains("wait"));
    assert!(answer(Executor::Claude, "not json").is_none());

    let codex = "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"first\"}}\n{\"type\":\"agent_message\",\"message\":\"second\"}\n";
    assert_eq!(answer(Executor::Codex, codex).unwrap(), "first\nsecond");
    assert!(answer(Executor::Codex, "{\"type\":\"token_count\"}").is_none());
}
