//! Unit tests for `todo.rs`: promoting a todo into a task.

use super::*;

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
