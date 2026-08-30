//! Handler-level tests for the routes the orchestration loop calls. The
//! handlers are exercised directly: axum's extractors are plain values.

use super::*;
use lgtm_protocol::{Review, Severity, TaskResult};

fn app() -> Arc<App> {
    let (persist, _rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(App {
        token: "tok".into(),
        state: std::sync::Mutex::new(crate::state::State::default()),
        persist,
        github: None,
        linear: None,
        webhook: None,
        orchestrate: None,
        base_url: "http://127.0.0.1:1".into(),
        orchestrating: std::sync::Mutex::new(Default::default()),
    })
}

/// A completed task, with `ok` checks and `blocking` review findings.
fn completed(app: &App, ok: bool, blocking: bool) -> String {
    let mut state = app.state.lock().unwrap();
    state.queue_without_runners = true;
    let spec = TaskSpec {
        repository: "https://example.com/repo.git".into(),
        base_branch: "main".into(),
        prompt: "do it".into(),
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
        review_executor: None,
        model: None,
        goal: None,
        allowed_hosts: Vec::new(),
        session: None,
    };
    let (task, _) = state.create_task(spec).unwrap();
    let result = TaskResult {
        branch: "lgtm/x".into(),
        diff: String::new(),
        changed_files: Vec::new(),
        validation: vec![lgtm_protocol::ValidationResult {
            name: "test".into(),
            command: "cargo test".into(),
            ok,
            output_tail: String::new(),
        }],
        plan: None,
        review: blocking.then(|| Review {
            findings: vec![lgtm_protocol::Finding {
                severity: Severity::Blocking,
                file: String::new(),
                line: None,
                message: "unsafe".into(),
            }],
            executor: None,
        }),
        policy: None,
        cost_usd: 0.0,
    };
    state.apply_event(&task.id, TaskEvent::Completed { result });
    task.id
}

#[tokio::test]
async fn an_orchestrated_step_lands_on_the_task_event_log() {
    let app = app();
    let id = completed(&app, true, false);
    let body = serde_json::json!({
        "action": "task_create", "reason": "the goal needs docs",
        "applied": true, "note": "ab12",
    });
    let code = orchestrated(
        State(app.clone()),
        Path(id.clone()),
        Ok(Json(serde_json::from_value(body).unwrap())),
    )
    .await
    .unwrap();
    assert_eq!(code, StatusCode::NO_CONTENT);

    let state = app.state.lock().unwrap();
    let last = &state.tasks[&id].events.last().unwrap().event;
    let TaskEvent::Orchestrated {
        action,
        reason,
        applied,
        note,
    } = last
    else {
        panic!("not an orchestrated event: {last:?}");
    };
    assert_eq!(
        (action.as_str(), reason.as_str()),
        ("task_create", "the goal needs docs")
    );
    assert!(applied);
    assert_eq!(note, "ab12");
}

#[tokio::test]
async fn an_orchestrated_step_for_an_unknown_task_is_a_404() {
    let app = app();
    let body = serde_json::json!({ "action": "wait" });
    let err = orchestrated(
        State(app),
        Path("nothing".into()),
        Ok(Json(serde_json::from_value(body).unwrap())),
    )
    .await
    .expect_err("a refusal");
    assert_eq!(err.0, StatusCode::NOT_FOUND);
}

/// The gate the loop is held to; a person's approve never sees it.
#[test]
fn policy_clean_refuses_what_the_checks_and_the_review_did_not_clear() {
    let app = app();
    let clean = completed(&app, true, false);
    let failed = completed(&app, false, false);
    let blocked = completed(&app, true, true);
    let state = app.state.lock().unwrap();
    let task = |id: &str| state.tasks[id].task.clone();

    assert!(policy_clean(&task(&clean)).is_ok());
    assert_eq!(policy_clean(&task(&failed)).unwrap_err().1, "checks failed");
    assert_eq!(
        policy_clean(&task(&blocked)).unwrap_err().1,
        "blocking review findings"
    );
}
