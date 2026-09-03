//! Handler-level tests for the routes the orchestration loop calls. The
//! handlers are exercised directly: axum's extractors are plain values.

use super::*;
use lgtm_protocol::{Review, Session, Severity, TaskResult};

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
        message(
            State(app.clone()),
            Path(id.clone()),
            headers,
            Extension(AuthedUser(None)),
            text(),
        )
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

async fn new_session(app: &Arc<App>) -> Session {
    let body = serde_json::from_value(serde_json::json!({
        "repository": "https://example.com/repo.git",
        "base_branch": "main",
    }))
    .unwrap();
    let (_, Json(session)) = sessions::create_session(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    session
}

async fn listed_sessions(app: &Arc<App>) -> Vec<Session> {
    let query = serde_json::from_value(serde_json::json!({})).unwrap();
    sessions::list_sessions(State(app.clone()), Query(query))
        .await
        .0
}

#[tokio::test]
async fn renaming_a_session_changes_its_listed_title() {
    let app = app();
    let session = new_session(&app).await;

    let patch = serde_json::from_value(serde_json::json!({ "title": "new title" })).unwrap();
    let Json(updated) = sessions::update_session(
        State(app.clone()),
        Path(session.id.clone()),
        Ok(Json(patch)),
    )
    .await
    .unwrap();
    assert_eq!(updated.title, "new title");
    assert_eq!(
        listed_sessions(&app)
            .await
            .into_iter()
            .find(|s| s.id == session.id)
            .unwrap()
            .title,
        "new title"
    );
}

#[tokio::test]
async fn archiving_a_session_sets_the_flag_but_keeps_it_listed() {
    let app = app();
    let session = new_session(&app).await;

    let patch = serde_json::from_value(serde_json::json!({ "archived": true })).unwrap();
    let Json(updated) = sessions::update_session(
        State(app.clone()),
        Path(session.id.clone()),
        Ok(Json(patch)),
    )
    .await
    .unwrap();
    assert!(updated.archived);
    assert!(listed_sessions(&app)
        .await
        .iter()
        .any(|s| s.id == session.id));
}

#[tokio::test]
async fn deleting_a_session_removes_it_but_leaves_its_tasks() {
    let app = app();
    app.state.lock().unwrap().queue_without_runners = true;
    let session = new_session(&app).await;
    let message = serde_json::from_value(serde_json::json!({
        "text": "do it",
        "executor": "claude",
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

    let status = sessions::delete_session(State(app.clone()), Path(session.id.clone()))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(!listed_sessions(&app)
        .await
        .iter()
        .any(|s| s.id == session.id));
    assert!(app.state.lock().unwrap().tasks.contains_key(&task.id));
}

#[tokio::test]
async fn an_unknown_session_id_404s_on_update_and_delete() {
    let app = app();
    let patch = serde_json::from_value(serde_json::json!({ "title": "x" })).unwrap();
    assert_eq!(
        sessions::update_session(State(app.clone()), Path("deadbeef".into()), Ok(Json(patch)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
    assert_eq!(
        sessions::delete_session(State(app.clone()), Path("deadbeef".into()))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn an_empty_title_is_rejected() {
    let app = app();
    let session = new_session(&app).await;

    let patch = serde_json::from_value(serde_json::json!({ "title": "   " })).unwrap();
    assert_eq!(
        sessions::update_session(State(app.clone()), Path(session.id), Ok(Json(patch)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::BAD_REQUEST)
    );
}

#[tokio::test]
async fn editing_an_agent_proposal_approves_the_memory() {
    let app = app();
    let body = serde_json::from_value(serde_json::json!({
        "content": "the build needs bun",
        "source": "agent",
    }))
    .unwrap();
    let (_, Json(memory)) = memories::create_memory(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    assert_eq!(
        memory.verification,
        lgtm_protocol::Verification::AgentProposed
    );

    let patch = serde_json::from_value(serde_json::json!({ "content": "needs bun 1.2" })).unwrap();
    let Json(edited) =
        memories::update_memory(State(app.clone()), Path(memory.id.clone()), Ok(Json(patch)))
            .await
            .unwrap();
    assert_eq!(edited.content, "needs bun 1.2");
    assert_eq!(
        edited.verification,
        lgtm_protocol::Verification::UserApproved
    );

    let blank = serde_json::from_value(serde_json::json!({ "content": "  " })).unwrap();
    assert_eq!(
        memories::update_memory(State(app.clone()), Path(memory.id), Ok(Json(blank)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::BAD_REQUEST)
    );
    let patch = serde_json::from_value(serde_json::json!({ "content": "x" })).unwrap();
    assert_eq!(
        memories::update_memory(State(app), Path("deadbeef".into()), Ok(Json(patch)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
}

async fn new_todo(app: &Arc<App>) -> lgtm_protocol::Todo {
    let body = serde_json::from_value(serde_json::json!({ "title": "ship it" })).unwrap();
    let (_, Json(todo)) = todos::create_todo(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    todo.todo
}

#[tokio::test]
async fn a_comment_is_attributed_and_read_back_with_its_todo() {
    let app = app();
    let todo = new_todo(&app).await;
    let body = serde_json::from_value(serde_json::json!({ "body": " looks good " })).unwrap();
    let (code, Json(comment)) = todos::create_comment(
        State(app.clone()),
        Extension(AuthedUser(Some("u1".into()))),
        Path(todo.id.clone()),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(comment.body, "looks good");
    assert_eq!(comment.author.as_deref(), Some("u1"));

    let Json(detail) = todos::get_todo(State(app.clone()), Path(todo.id.clone()))
        .await
        .unwrap();
    assert_eq!(detail.todo.todo.id, todo.id);
    assert_eq!(detail.comments, vec![comment]);

    todos::delete_todo(State(app.clone()), Path(todo.id.clone()))
        .await
        .unwrap();
    assert!(app.state.lock().unwrap().todo_comments.is_empty());
    assert_eq!(
        todos::get_todo(State(app), Path(todo.id))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn an_empty_comment_is_rejected_and_an_unknown_todo_404s() {
    let app = app();
    let todo = new_todo(&app).await;
    let blank = serde_json::from_value(serde_json::json!({ "body": "   " })).unwrap();
    assert_eq!(
        todos::create_comment(
            State(app.clone()),
            Extension(AuthedUser(None)),
            Path(todo.id),
            Ok(Json(blank)),
        )
        .await
        .err()
        .map(|err| err.0),
        Some(StatusCode::BAD_REQUEST)
    );
    let body = serde_json::from_value(serde_json::json!({ "body": "hi" })).unwrap();
    assert_eq!(
        todos::create_comment(
            State(app),
            Extension(AuthedUser(None)),
            Path("deadbeef".into()),
            Ok(Json(body)),
        )
        .await
        .err()
        .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn a_scratchpad_is_created_listed_archived_and_deleted() {
    let app = app();
    let body = serde_json::from_value(serde_json::json!({
        "repository": "https://example.com/repo.git",
        "content": "",
    }))
    .unwrap();
    let (code, Json(pad)) = scratchpads::create_scratchpad(
        State(app.clone()),
        Extension(AuthedUser(Some("u1".into()))),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    assert_eq!(code, StatusCode::CREATED);
    assert_eq!(pad.created_by.as_deref(), Some("u1"));
    assert!(pad.content.is_empty());

    let patch = serde_json::from_value(serde_json::json!({
        "content": "# notes",
        "archived": true,
    }))
    .unwrap();
    let Json(updated) =
        scratchpads::update_scratchpad(State(app.clone()), Path(pad.id.clone()), Ok(Json(patch)))
            .await
            .unwrap();
    assert_eq!(updated.content, "# notes");
    assert!(updated.archived);

    let query = serde_json::from_value(serde_json::json!({
        "repository": "https://example.com/repo.git",
    }))
    .unwrap();
    let Json(listed) = scratchpads::list_scratchpads(State(app.clone()), Query(query)).await;
    assert_eq!(listed, vec![updated]);

    let status = scratchpads::delete_scratchpad(State(app.clone()), Path(pad.id.clone()))
        .await
        .unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        scratchpads::get_scratchpad(State(app), Path(pad.id))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn a_todo_is_served_with_the_display_id_its_project_gives_it() {
    let app = app();
    let body = serde_json::from_value(serde_json::json!({
        "repository": "https://github.com/arsenstorm/lgtm.git",
        "title": "ship it",
    }))
    .unwrap();
    let (_, Json(created)) = todos::create_todo(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    assert_eq!(created.display_id, "L-1");

    let Json(detail) = todos::get_todo(State(app.clone()), Path(created.todo.id.clone()))
        .await
        .unwrap();
    assert_eq!(detail.todo.display_id, "L-1");

    let Json(done) = todos::finish_todo(State(app.clone()), Path(created.todo.id.clone()))
        .await
        .unwrap();
    assert_eq!(done.display_id, "L-1");

    let patch = serde_json::from_value(serde_json::json!({ "title": "ship it twice" })).unwrap();
    let Json(patched) =
        todos::update_todo(State(app.clone()), Path(created.todo.id), Ok(Json(patch)))
            .await
            .unwrap();
    assert_eq!(patched.display_id, "L-1");

    let query = serde_json::from_value(serde_json::json!({})).unwrap();
    let Json(listed) = todos::list_todos(State(app.clone()), Query(query)).await;
    assert_eq!(listed[0].display_id, "L-1");

    // The serialized todo carries the display id beside its own fields.
    let json = serde_json::to_value(&listed[0]).unwrap();
    assert_eq!(json["display_id"], "L-1");
    assert_eq!(json["number"], 1);
}

#[tokio::test]
async fn a_prefix_is_uppercased_and_refused_when_another_project_holds_it() {
    let app = app();
    for repository in [
        "https://github.com/arsenstorm/lgtm.git",
        "https://github.com/arsenstorm/LegalBase.git",
    ] {
        let body =
            serde_json::from_value(serde_json::json!({ "repository": repository, "title": "t" }))
                .unwrap();
        let _created = todos::create_todo(
            State(app.clone()),
            Extension(AuthedUser(None)),
            Ok(Json(body)),
        )
        .await
        .unwrap();
    }
    let Json(listed) = projects::list_projects(State(app.clone())).await;
    assert_eq!(
        listed
            .iter()
            .map(|p| (p.name.as_str(), p.prefix.as_str()))
            .collect::<Vec<_>>(),
        vec![("LegalBase", "LE"), ("lgtm", "L")]
    );
    let legal = listed[0].id.clone();

    let body = serde_json::from_value(serde_json::json!({ "prefix": "lb" })).unwrap();
    let Json(renamed) =
        projects::update_project(State(app.clone()), Path(legal.clone()), Ok(Json(body)))
            .await
            .unwrap();
    assert_eq!(renamed.prefix, "LB");

    let taken = serde_json::from_value(serde_json::json!({ "prefix": "l" })).unwrap();
    assert_eq!(
        projects::update_project(State(app.clone()), Path(legal.clone()), Ok(Json(taken)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::CONFLICT)
    );

    // "L1" is legal: derivation itself mints digit-suffixed prefixes when a
    // name is exhausted, so a person must be able to type one back.
    for bad in ["", "toolongaprefix", "1L", "L-1"] {
        let body = serde_json::from_value(serde_json::json!({ "prefix": bad })).unwrap();
        assert_eq!(
            projects::update_project(State(app.clone()), Path(legal.clone()), Ok(Json(body)))
                .await
                .err()
                .map(|err| err.0),
            Some(StatusCode::BAD_REQUEST),
            "{bad} should be refused"
        );
    }

    let body = serde_json::from_value(serde_json::json!({ "prefix": "X" })).unwrap();
    assert_eq!(
        projects::update_project(State(app), Path("deadbeef".into()), Ok(Json(body)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn tags_are_normalized_on_the_way_in_and_over_the_limit_is_a_400() {
    let app = app();
    let body = serde_json::from_value(serde_json::json!({
        "title": "ship it",
        "tags": [" api ", "api", "", "API"],
    }))
    .unwrap();
    let (_, Json(created)) = todos::create_todo(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(body)),
    )
    .await
    .unwrap();
    assert_eq!(
        created.todo.tags,
        vec!["api".to_string(), "API".to_string()]
    );

    let patch = serde_json::from_value(serde_json::json!({ "tags": ["  rust  "] })).unwrap();
    let Json(patched) = todos::update_todo(
        State(app.clone()),
        Path(created.todo.id.clone()),
        Ok(Json(patch)),
    )
    .await
    .unwrap();
    assert_eq!(patched.todo.tags, vec!["rust".to_string()]);

    let long = serde_json::json!({ "tags": ["x".repeat(lgtm_protocol::TAG_LEN_MAX + 1)] });
    let patch = serde_json::from_value(long).unwrap();
    assert_eq!(
        todos::update_todo(State(app.clone()), Path(created.todo.id), Ok(Json(patch)))
            .await
            .err()
            .map(|err| err.0),
        Some(StatusCode::BAD_REQUEST)
    );

    let pad = serde_json::from_value(serde_json::json!({ "tags": [" notes ", "notes"] })).unwrap();
    let (_, Json(scratchpad)) = scratchpads::create_scratchpad(
        State(app.clone()),
        Extension(AuthedUser(None)),
        Ok(Json(pad)),
    )
    .await
    .unwrap();
    assert_eq!(scratchpad.tags, vec!["notes".to_string()]);

    let patch = serde_json::from_value(serde_json::json!({ "tags": ["a", "a", " b "] })).unwrap();
    let Json(updated) =
        scratchpads::update_scratchpad(State(app), Path(scratchpad.id), Ok(Json(patch)))
            .await
            .unwrap();
    assert_eq!(updated.tags, vec!["a".to_string(), "b".to_string()]);
}
