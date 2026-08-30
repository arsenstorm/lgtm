//! Unit tests for `todo.rs`: promoting a todo into a task, and patching one.

use super::*;
use lgtm_protocol::Priority;

/// A runner-free state that still lets `create_task` succeed, as if
/// provisioning were on.
fn state() -> State {
    State {
        queue_without_runners: true,
        ..State::default()
    }
}

fn into() -> PromoteInto {
    PromoteInto {
        base_branch: "main".into(),
        executor: Executor::Claude,
        runner: None,
    }
}

#[test]
fn promote_uses_the_title_alone_without_a_description() {
    let mut state = state();
    let todo = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "add a /health endpoint".into(),
        String::new(),
    );
    let (task, _) = state.promote_todo(&todo.id, into()).unwrap();
    assert_eq!(task.spec.prompt, "add a /health endpoint");
}

#[test]
fn promote_joins_title_and_description() {
    let mut state = state();
    let todo = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "add a /health endpoint".into(),
        "should return 200 while runners are connected".into(),
    );
    let (task, _) = state.promote_todo(&todo.id, into()).unwrap();
    assert_eq!(
        task.spec.prompt,
        "add a /health endpoint\n\nshould return 200 while runners are connected"
    );
}

#[test]
fn promoted_todo_moves_in_progress_and_points_at_its_task() {
    let mut state = state();
    let todo = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "add a /health endpoint".into(),
        String::new(),
    );
    let (task, _) = state.promote_todo(&todo.id, into()).unwrap();
    let stored = &state.todos[&todo.id];
    assert_eq!(stored.status, TodoStatus::InProgress);
    assert_eq!(stored.task.as_deref(), Some(task.id.as_str()));
}

#[test]
fn promoting_twice_errors() {
    let mut state = state();
    let todo = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "add a /health endpoint".into(),
        String::new(),
    );
    state.promote_todo(&todo.id, into()).unwrap();
    let err = state.promote_todo(&todo.id, into()).unwrap_err();
    assert_eq!(err, "todo is already in_progress");
}

#[test]
fn promote_without_a_repository_errors() {
    let mut state = state();
    let todo = state.create_todo(None, "add a /health endpoint".into(), String::new());
    let err = state.promote_todo(&todo.id, into()).unwrap_err();
    assert_eq!(err, "todo has no repository");
}

#[test]
fn promote_refuses_a_blocked_todo() {
    let mut state = state();
    let blocker = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "first".into(),
        String::new(),
    );
    let todo = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "add a /health endpoint".into(),
        String::new(),
    );
    state
        .update_todo(
            &todo.id,
            TodoPatch {
                blockers: Some(vec![blocker.id.clone()]),
                ..Default::default()
            },
        )
        .unwrap();
    let err = state.promote_todo(&todo.id, into()).unwrap_err();
    assert_eq!(err, format!("todo is blocked by {}", blocker.id));
}

#[test]
fn promote_allows_a_todo_whose_blocker_is_done() {
    let mut state = state();
    let blocker = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "first".into(),
        String::new(),
    );
    state.finish_todo(&blocker.id);
    let todo = state.create_todo(
        Some("https://example.com/repo.git".into()),
        "add a /health endpoint".into(),
        String::new(),
    );
    state
        .update_todo(
            &todo.id,
            TodoPatch {
                blockers: Some(vec![blocker.id.clone()]),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(state.promote_todo(&todo.id, into()).is_ok());
}

#[test]
fn update_unknown_todo_errors() {
    let mut state = state();
    assert!(matches!(
        state.update_todo("nope", TodoPatch::default()),
        Err(UpdateTodoError::NotFound)
    ));
}

#[test]
fn update_rejects_a_self_blocker() {
    let mut state = state();
    let todo = state.create_todo(None, "t".into(), String::new());
    let err = state
        .update_todo(
            &todo.id,
            TodoPatch {
                blockers: Some(vec![todo.id.clone()]),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(err, UpdateTodoError::SelfBlocker));
}

#[test]
fn update_rejects_an_unknown_blocker() {
    let mut state = state();
    let todo = state.create_todo(None, "t".into(), String::new());
    let err = state
        .update_todo(
            &todo.id,
            TodoPatch {
                blockers: Some(vec!["nope".into()]),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(err, UpdateTodoError::UnknownBlocker(id) if id == "nope"));
}

#[test]
fn update_sets_priority_and_assignee_and_leaves_absent_fields_alone() {
    let mut state = state();
    let todo = state.create_todo(None, "t".into(), String::new());
    let updated = state
        .update_todo(
            &todo.id,
            TodoPatch {
                priority: Some(Priority::High),
                assignee: Some(Some("arsen".into())),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(updated.priority, Priority::High);
    assert_eq!(updated.assignee.as_deref(), Some("arsen"));
    assert!(updated.blockers.is_empty());
}

#[test]
fn update_clears_the_assignee_when_patched_to_null() {
    let mut state = state();
    let todo = state.create_todo(None, "t".into(), String::new());
    state
        .update_todo(
            &todo.id,
            TodoPatch {
                assignee: Some(Some("arsen".into())),
                ..Default::default()
            },
        )
        .unwrap();
    let cleared = state
        .update_todo(
            &todo.id,
            TodoPatch {
                assignee: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(cleared.assignee, None);
}
