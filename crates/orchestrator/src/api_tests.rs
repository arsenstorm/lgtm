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
        asking: tokio::sync::Semaphore::new(crate::ASK_SLOTS),
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
        created_by: None,
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

/// Identity comes from the token, so a spec claiming a user is overwritten
/// and the authenticated user lands on the task.
#[tokio::test]
async fn create_task_stamps_the_authenticated_user_over_the_body() {
    let app = app();
    app.state.lock().unwrap().queue_without_runners = true;
    let mut spec = TaskSpec {
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
        created_by: Some("liar".into()),
    };
    let (code, Json(task)) = create_task(
        State(app.clone()),
        Extension(AuthedUser(Some("u1".into()))),
        Ok(Json(spec.clone())),
    )
    .await
    .unwrap();
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(task.created_by.as_deref(), Some("u1"));
    assert_eq!(task.spec.created_by.as_deref(), Some("u1"));

    // The shared token stamps nothing, whatever the body says.
    spec.created_by = Some("liar".into());
    let (_, Json(task)) = create_task(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(spec)),
    )
    .await
    .unwrap();
    assert_eq!(task.created_by, None);
}

/// The workspace field's reader: a task stamped with a different explicit
/// workspace stays out of the list; legacy unstamped tasks stay in.
#[tokio::test]
async fn list_tasks_hides_tasks_from_another_workspace() {
    let app = app();
    app.state.lock().unwrap().workspace = Some("other".into());
    let foreign = completed(&app, true, false);
    app.state.lock().unwrap().workspace = None;
    let legacy = completed(&app, true, false);
    app.state.lock().unwrap().workspace = Some("acme".into());
    let own = completed(&app, true, false);

    let Json(tasks) = list_tasks(State(app.clone())).await;
    let ids: Vec<&str> = tasks.iter().map(|task| task.id.as_str()).collect();
    assert!(ids.contains(&own.as_str()));
    assert!(ids.contains(&legacy.as_str()));
    assert!(!ids.contains(&foreign.as_str()));
}

/// The activity feed's types keep their fields to their own module, so the
/// tests read the JSON the endpoint actually returns.
fn activity_lines(value: &serde_json::Value) -> &Vec<serde_json::Value> {
    value.as_array().expect("an array")
}

async fn activity_json(app: &Arc<App>, limit: Option<u32>) -> serde_json::Value {
    let query = serde_json::from_value(serde_json::json!({ "limit": limit })).unwrap();
    let Json(lines) = workspace::activity(State(app.clone()), Query(query)).await;
    serde_json::to_value(&lines).unwrap()
}

#[tokio::test]
async fn activity_lists_newest_first_with_owner_names() {
    let app = app();
    let id = completed(&app, true, false);
    {
        let mut state = app.state.lock().unwrap();
        let (user, _token) = state.create_user("alice");
        state.tasks.get_mut(&id).unwrap().task.created_by = Some(user.id);
    }

    let json = activity_json(&app, None).await;
    let lines = activity_lines(&json);
    assert!(!lines.is_empty());
    assert!(lines
        .windows(2)
        .all(|pair| pair[0]["at"].as_u64() >= pair[1]["at"].as_u64()));
    let mine: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|line| line["task"] == serde_json::json!(id))
        .collect();
    assert!(!mine.is_empty());
    assert!(mine.iter().all(|line| line["owner"] == "alice"), "{mine:?}");
    assert!(mine.iter().any(|line| line["event"] == "completed"));
}

#[tokio::test]
async fn activity_respects_the_workspace() {
    let app = app();
    app.state.lock().unwrap().workspace = Some("other".into());
    let foreign = completed(&app, true, false);
    app.state.lock().unwrap().workspace = Some("acme".into());
    let own = completed(&app, true, false);

    let json = activity_json(&app, None).await;
    let tasks: Vec<&serde_json::Value> = activity_lines(&json)
        .iter()
        .map(|line| &line["task"])
        .collect();
    assert!(tasks.contains(&&serde_json::json!(own)));
    assert!(!tasks.contains(&&serde_json::json!(foreign)));
}

#[tokio::test]
async fn ask_without_orchestrate_is_a_conflict() {
    let app = app();
    let body =
        serde_json::from_value(serde_json::json!({ "question": "who is on auth?" })).unwrap();
    let err = workspace::ask(State(app), Extension(AuthedUser(None)), Ok(Json(body)))
        .await
        .map(|_| ())
        .expect_err("a refusal");
    assert_eq!(err.0, StatusCode::CONFLICT);
}

/// Like [`app`], but with an orchestration executor, so `ask` gets past the
/// not-configured refusal. No agent ever runs: the tests below stop earlier.
fn app_with_orchestrate() -> Arc<App> {
    let (persist, _rx) = tokio::sync::mpsc::unbounded_channel();
    Arc::new(App {
        token: "tok".into(),
        state: std::sync::Mutex::new(crate::state::State::default()),
        persist,
        github: None,
        linear: None,
        webhook: None,
        orchestrate: Some(Executor::Claude),
        base_url: "http://127.0.0.1:1".into(),
        orchestrating: std::sync::Mutex::new(Default::default()),
        asking: tokio::sync::Semaphore::new(crate::ASK_SLOTS),
    })
}

#[tokio::test]
async fn a_full_ask_house_refuses_the_next_question() {
    let app = app_with_orchestrate();
    app.asking
        .acquire_many(crate::ASK_SLOTS as u32)
        .await
        .unwrap()
        .forget();
    let body =
        serde_json::from_value(serde_json::json!({ "question": "who is on auth?" })).unwrap();
    let err = workspace::ask(State(app), Extension(AuthedUser(None)), Ok(Json(body)))
        .await
        .map(|_| ())
        .expect_err("a refusal");
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert!(err.1.contains("too many questions"), "{}", err.1);
}

#[tokio::test]
async fn activity_hides_the_per_line_output_flood() {
    let app = app();
    let id = completed(&app, true, false);
    {
        let mut state = app.state.lock().unwrap();
        for _ in 0..40 {
            state.apply_event(
                &id,
                TaskEvent::Output {
                    stream: lgtm_protocol::OutputStream::Stdout,
                    line: "line".into(),
                },
            );
        }
    }
    let json = activity_json(&app, None).await;
    let lines = activity_lines(&json);
    assert!(!lines.is_empty());
    assert!(
        lines.iter().all(|line| line["event"] != "output"),
        "{lines:?}"
    );
    // The flood must not push the real events out of the window either.
    assert!(lines.iter().any(|line| line["event"] == "completed"));
}

#[tokio::test]
async fn a_plan_message_becomes_a_plan_task() {
    let app = app();
    app.state.lock().unwrap().queue_without_runners = true;
    let body = serde_json::from_value(serde_json::json!({
        "repository": "https://example.com/repo.git",
        "base_branch": "develop",
        "title": "",
    }))
    .unwrap();
    let (_, Json(session)) = sessions::create_session(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(body)),
    )
    .await
    .unwrap();

    let message = serde_json::from_value(serde_json::json!({
        "text": "propose the steps",
        "executor": "claude",
        "kind": "plan",
    }))
    .unwrap();
    let (_, Json(task)) = sessions::send_message(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Path(session.id.clone()),
        Ok(Json(message)),
    )
    .await
    .unwrap();
    assert_eq!(task.spec.kind, TaskKind::Plan);
    // The session's branch is the task's base, and a plain message stays a run.
    assert_eq!(task.spec.base_branch, "develop");
}

/// The goal header is the server-side half of "a pass acts only on its own
/// goal's tasks"; without it a person's client is unaffected.
#[tokio::test]
async fn the_goal_header_scopes_a_write_to_that_goal() {
    let app = app();
    let id = completed(&app, true, false);
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-lgtm-goal", "g1".parse().unwrap());
    let text = || {
        Ok(Json(
            serde_json::from_value::<MessageBody>(serde_json::json!({ "text": "more" })).unwrap(),
        ))
    };

    // The task has no runner, so the write past the gate is a 409, not a 200:
    // what this test reads is only whether the gate refused it.
    let refused = async |headers| {
        message(State(app.clone()), Path(id.clone()), headers, text())
            .await
            .err()
            .map(|err| err.0)
            == Some(StatusCode::FORBIDDEN)
    };

    assert!(refused(headers.clone()).await);
    // No header: a person's client is unaffected.
    assert!(!refused(Default::default()).await);

    // Under the named goal: scoping passes.
    app.state
        .lock()
        .unwrap()
        .tasks
        .get_mut(&id)
        .unwrap()
        .task
        .spec
        .goal = Some("g1".into());
    assert!(!refused(headers).await);
}
