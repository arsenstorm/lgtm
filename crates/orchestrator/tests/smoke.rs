use futures_util::{SinkExt, StreamExt};
use lgtm_protocol::*;
use tokio_tungstenite::tungstenite::Message as TMsg;

async fn ws(
    url: &str,
    auth: bool,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut req = url.into_client_request().unwrap();
    if auth {
        req.headers_mut()
            .insert("authorization", "Bearer tok".parse().unwrap());
    }
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

/// The next frame that is not the title lane's background `Infer` call:
/// creating a task fires one at the connected runner, and these tests assert
/// on the frames around it.
async fn next_frame(
    w: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> TMsg {
    loop {
        let msg = w.next().await.unwrap().unwrap();
        if let Ok(OrchestratorMessage::Infer { .. }) =
            serde_json::from_str::<OrchestratorMessage>(msg.to_text().unwrap_or_default())
        {
            continue;
        }
        return msg;
    }
}

#[tokio::test]
async fn end_to_end() {
    // The GitHub routes must answer as if no token were configured, whatever
    // the machine running the test has. `GitHub::from_env` falls back to
    // `gh auth token`, so the empty PATH is what keeps a developer's login out
    // of the test; nothing else here shells out.
    std::env::remove_var("GITHUB_TOKEN");
    std::env::remove_var("LINEAR_API_KEY");
    std::env::set_var("PATH", "");
    let dir = std::env::temp_dir().join(format!("lgtm-smoke-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let r = http
        .get(format!("{base}/api/runners"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(r.text().await.unwrap(), r#"{"error":"unauthorized"}"#);

    let spec = TaskSpec {
        repository: "r".into(),
        base_branch: "main".into(),
        prompt: "p".into(),
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
        requirements: vec![],
        goal: None,
        review_executor: None,
        model: None,
        reasoning_effort: None,
        allowed_hosts: Vec::new(),
        session: None,
        created_by: None,
    };
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(r.text().await.unwrap(), r#"{"error":"no eligible runner"}"#);

    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .header("content-type", "application/json")
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    let mut w = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    let info = RunnerInfo {
        name: "w1".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 1,
        ephemeral: false,
        capabilities: vec![],
        cpu_cores: 0,
        memory_mb: 0,
    };
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "tok".into(),
            info,
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let ack = next_frame(&mut w).await;
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(ack.to_text().unwrap()).unwrap(),
        OrchestratorMessage::HelloAck
    ));

    let runners: Vec<RunnerStatus> = http
        .get(format!("{base}/api/runners"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(runners.len(), 1);

    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let task: Task = r.json().await.unwrap();
    let start = next_frame(&mut w).await;
    let OrchestratorMessage::Start { task: started, .. } =
        serde_json::from_str(start.to_text().unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(started.id, task.id);
    assert_eq!(started.runner.as_deref(), Some("w1"));

    let r = http
        .post(format!("{base}/api/tasks/{}/approve", task.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.text().await.unwrap(),
        r#"{"error":"task is not awaiting review"}"#
    );

    let mut ev = ws(&format!("ws://{addr}/api/tasks/{}/events", task.id), true).await;
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Started { model: None },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let first: StoredEvent =
        serde_json::from_str(ev.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(first.event, TaskEvent::Started { model: None });

    let result = TaskResult {
        branch: format!("lgtm/{}", task.id),
        diff: "d".into(),
        changed_files: vec!["a".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Completed { result },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let second: StoredEvent =
        serde_json::from_str(ev.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert!(matches!(second.event, TaskEvent::Completed { .. }));
    assert!(matches!(ev.next().await, Some(Ok(TMsg::Close(_))) | None));

    let detail: serde_json::Value = http
        .get(format!("{base}/api/tasks/{}", task.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["task"]["status"], "awaiting_review");
    assert_eq!(detail["events"].as_array().unwrap().len(), 2);
    let executions = detail["task"]["executions"].as_array().unwrap();
    assert_eq!(executions.len(), 1);
    assert_eq!(executions[0]["status"], "completed");

    let r = http
        .post(format!("{base}/api/tasks/{}/approve", task.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let push = next_frame(&mut w).await;
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(push.to_text().unwrap()).unwrap(),
        OrchestratorMessage::Push { .. }
    ));

    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Pushed {
                branch: "b".into(),
                sha: "deadbeef".into(),
            },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let mut ev2 = ws(&format!("ws://{addr}/api/tasks/{}/events", task.id), true).await;
    let mut count = 0;
    while let Some(Ok(m)) = ev2.next().await {
        if m.is_text() {
            count += 1
        } else {
            break;
        }
    }
    assert_eq!(count, 3);

    // `from` past a terminal task's history: nothing to replay, and the
    // status is terminal, so the socket closes right away.
    let mut ev3 = ws(
        &format!("ws://{addr}/api/tasks/{}/events?from={count}", task.id),
        true,
    )
    .await;
    let closed = match ev3.next().await {
        None => true,
        Some(Ok(m)) => m.is_close(),
        Some(Err(_)) => true,
    };
    assert!(closed, "terminal task must close even with from={count}");

    // second task, started, then the runner drops: the grace period keeps it
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    let task2: Task = r.json().await.unwrap();
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Event {
            task_id: task2.id.clone(),
            event: TaskEvent::Started { model: None },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    drop(w);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let detail: serde_json::Value = http
        .get(format!("{base}/api/tasks/{}", task2.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["task"]["status"], "running");

    let r = http
        .post(format!("{base}/api/tasks/{}/message", task2.id))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "text": "still going" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    let runners: Vec<RunnerStatus> = http
        .get(format!("{base}/api/runners"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        runners.is_empty(),
        "a runner inside its grace period is not connected"
    );

    // Writes land from a background task, so give it a moment.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(dir.join("tasks").join(format!("{}.json", task.id)).exists());
    let tasks: Vec<Task> = http
        .get(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tasks.len(), 2);

    // reconnect under the same name: the old socket's cleanup must not evict the new one
    let mut w1 = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    let info = RunnerInfo {
        name: "w2".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 1,
        ephemeral: false,
        capabilities: vec![],
        cpu_cores: 0,
        memory_mb: 0,
    };
    w1.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "tok".into(),
            info: info.clone(),
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    w1.next().await.unwrap().unwrap();
    let mut w2 = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    w2.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "tok".into(),
            info,
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    next_frame(&mut w2).await;
    drop(w1);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let runners: Vec<RunnerStatus> = http
        .get(format!("{base}/api/runners"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        runners.len(),
        1,
        "new registration survives the old socket's cleanup"
    );
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let task_a: Task = r.json().await.unwrap();
    let start = next_frame(&mut w2).await;
    let OrchestratorMessage::Start { task: started, .. } =
        serde_json::from_str(start.to_text().unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(started.id, task_a.id);

    // the runner's one slot is taken, so the next task queues instead of 409
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let task_b: Task = r.json().await.unwrap();
    assert_eq!(task_b.runner, None);
    let detail: serde_json::Value = http
        .get(format!("{base}/api/tasks/{}", task_b.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["task"]["status"], "queued");
    assert!(detail["task"]["runner"].is_null());

    let result = TaskResult {
        branch: format!("lgtm/{}", task_a.id),
        diff: "d".into(),
        changed_files: vec!["a".into()],
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    w2.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Event {
            task_id: task_a.id.clone(),
            event: TaskEvent::Completed { result },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let start = next_frame(&mut w2).await;
    let OrchestratorMessage::Start { task: started, .. } =
        serde_json::from_str(start.to_text().unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(started.id, task_b.id);
    assert_eq!(started.runner.as_deref(), Some("w2"));

    let mut bad = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    let info = RunnerInfo {
        name: "evil".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 1,
        ephemeral: false,
        capabilities: vec![],
        cpu_cores: 0,
        memory_mb: 0,
    };
    bad.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "nope".into(),
            info,
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    assert!(matches!(bad.next().await, Some(Ok(TMsg::Close(_))) | None));

    let mut stale = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    let info = RunnerInfo {
        name: "stale".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 1,
        ephemeral: false,
        capabilities: vec![],
        cpu_cores: 0,
        memory_mb: 0,
    };
    stale
        .send(TMsg::Text(
            serde_json::to_string(&RunnerMessage::Hello {
                token: "tok".into(),
                info,
                running: Vec::new(),
                version: 0,
            })
            .unwrap()
            .into(),
        ))
        .await
        .unwrap();
    let rejected = stale.next().await.unwrap().unwrap();
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(rejected.to_text().unwrap()).unwrap(),
        OrchestratorMessage::Rejected { .. }
    ));
    assert!(matches!(
        stale.next().await,
        Some(Ok(TMsg::Close(_))) | None
    ));

    let r = http
        .post(format!("{base}/api/tasks/from-issue"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "issue": "arsenstorm/lgtm#7",
            "base_branch": "main",
            "executor": "claude",
            "runner": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.text().await.unwrap(),
        r#"{"error":"GITHUB_TOKEN is not configured"}"#
    );

    let r = http
        .post(format!("{base}/api/tasks/from-linear"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "issue": "ENG-7",
            "repository": "https://github.com/arsenstorm/lgtm.git",
            "base_branch": "main",
            "executor": "claude",
            "runner": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.text().await.unwrap(),
        r#"{"error":"LINEAR_API_KEY is not configured"}"#
    );

    let r = http
        .post(format!("{base}/api/tasks/{}/merge", task_b.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.text().await.unwrap(),
        r#"{"error":"task is not approved"}"#
    );

    assert_eq!(
        http.get(format!("{base}/api/tasks/nope"))
            .bearer_auth("tok")
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        http.post(format!("{base}/api/tasks/nope/cancel"))
            .bearer_auth("tok")
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    // a batch needs the integration its source names
    let r = http
        .post(format!("{base}/api/batches"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "source": { "type": "github_label", "owner": "o", "repo": "r", "label": "lgtm" },
            "base_branch": "main",
            "executor": "claude",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(
        r.text().await.unwrap(),
        r#"{"error":"GITHUB_TOKEN is not configured"}"#
    );
    let batches: Vec<Batch> = http
        .get(format!("{base}/api/batches"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(batches.is_empty());

    // a goal and the one task it starts with
    let r = http
        .post(format!("{base}/api/goals"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "objective": "ship the health endpoint",
            "repository": "r",
            "base_branch": "main",
            "executor": "claude",
            "plan": false,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let created: GoalSummary = r.json().await.unwrap();
    let goals: Vec<GoalSummary> = http
        .get(format!("{base}/api/goals"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(goals.len(), 1);
    assert_eq!(goals[0].goal.id, created.goal.id);
    // a runner is connected, so the goal's only task is queued or running
    assert_eq!(goals[0].status, GoalStatus::Running);
    let detail: serde_json::Value = http
        .get(format!("{base}/api/goals/{}", created.goal.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["tasks"][0]["spec"]["goal"], created.goal.id);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr2,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let tasks: Vec<Task> = http
        .get(format!("http://{addr2}/api/tasks"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tasks.len(), 5);
    let by_id = |id: &str| tasks.iter().find(|t| t.id == id).unwrap();
    assert_eq!(by_id(&task.id).status, TaskStatus::Approved);
    let interrupted = by_id(&task2.id);
    assert_eq!(interrupted.status, TaskStatus::Failed);
    assert_eq!(interrupted.error.as_deref(), Some("orchestrator restarted"));
    assert_eq!(by_id(&task_a.id).status, TaskStatus::AwaitingReview);
    assert_eq!(
        by_id(&task_b.id).status,
        TaskStatus::Queued,
        "a queued task waits for a runner instead of failing"
    );
    let goals: Vec<GoalSummary> = http
        .get(format!("http://{addr2}/api/goals"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(goals.len(), 1);
    // membership survives the restart because the tasks carry it
    let detail: serde_json::Value = http
        .get(format!("http://{addr2}/api/goals/{}", goals[0].goal.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["tasks"].as_array().unwrap().len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_message_becomes_a_task_under_its_session() {
    let dir = std::env::temp_dir().join(format!("lgtm-sessions-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let mut w = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "tok".into(),
            info: RunnerInfo {
                name: "w1".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                executors: vec![Executor::Claude],
                slots: 1,
                ephemeral: false,
                capabilities: vec![],
                cpu_cores: 0,
                memory_mb: 0,
            },
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    next_frame(&mut w).await;

    let r = http
        .post(format!("{base}/api/sessions"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "repository": "r", "base_branch": "main" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let session: Session = r.json().await.unwrap();
    assert_eq!(session.title, "");

    let r = http
        .post(format!("{base}/api/sessions/{}/messages", session.id))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "text": "add a /health endpoint", "executor": "claude" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let task: Task = r.json().await.unwrap();
    assert_eq!(task.spec.session.as_deref(), Some(session.id.as_str()));

    let detail: SessionDetail = http
        .get(format!("{base}/api/sessions/{}", session.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail.session.title, "add a /health endpoint");
    assert_eq!(detail.tasks, vec![task]);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_memory_reaches_the_runner() {
    let dir = std::env::temp_dir().join(format!("lgtm-memories-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let r = http
        .post(format!("{base}/api/memories"))
        .bearer_auth("tok")
        .json(&serde_json::json!({ "repository": "r", "content": "deploys are manual" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let memory: Memory = r.json().await.unwrap();

    let mut w = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "tok".into(),
            info: RunnerInfo {
                name: "w1".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                executors: vec![Executor::Claude],
                slots: 1,
                ephemeral: false,
                capabilities: vec![],
                cpu_cores: 0,
                memory_mb: 0,
            },
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    next_frame(&mut w).await;

    let spec = TaskSpec {
        repository: "r".into(),
        base_branch: "main".into(),
        prompt: "p".into(),
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
        requirements: vec![],
        goal: None,
        review_executor: None,
        model: None,
        reasoning_effort: None,
        allowed_hosts: Vec::new(),
        session: None,
        created_by: None,
    };
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let start = next_frame(&mut w).await;
    let OrchestratorMessage::Start { memories, .. } =
        serde_json::from_str(start.to_text().unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(memories, vec![memory]);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_goals_plan_is_listed_once_the_agent_completes_it() {
    let dir = std::env::temp_dir().join(format!("lgtm-plans-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let mut w = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Hello {
            token: "tok".into(),
            info: RunnerInfo {
                name: "w1".into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                executors: vec![Executor::Claude],
                slots: 1,
                ephemeral: false,
                capabilities: Vec::new(),
                cpu_cores: 0,
                memory_mb: 0,
            },
            running: Vec::new(),
            version: PROTOCOL_VERSION,
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    next_frame(&mut w).await;

    let r = http
        .post(format!("{base}/api/goals"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "objective": "ship the health endpoint",
            "repository": "r",
            "base_branch": "main",
            "executor": "claude",
            "plan": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let created: GoalSummary = r.json().await.unwrap();

    let start = next_frame(&mut w).await;
    let OrchestratorMessage::Start { task, .. } =
        serde_json::from_str(start.to_text().unwrap()).unwrap()
    else {
        panic!()
    };

    let mut ev = ws(&format!("ws://{addr}/api/tasks/{}/events", task.id), true).await;
    let plan = Plan {
        steps: vec![PlanStep {
            key: "a".into(),
            title: "do a".into(),
            prompt: "do a".into(),
            depends_on: Vec::new(),
        }],
    };
    let result = TaskResult {
        branch: format!("lgtm/{}", task.id),
        diff: String::new(),
        changed_files: Vec::new(),
        validation: Vec::new(),
        plan: Some(plan),
        review: None,
        policy: None,
        cost_usd: 0.0,
    };
    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Completed { result },
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let completed: StoredEvent =
        serde_json::from_str(ev.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert!(matches!(completed.event, TaskEvent::Completed { .. }));

    let versions: Vec<PlanVersion> = http
        .get(format!("{base}/api/goals/{}/plans", created.goal.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].status, PlanStatus::AwaitingApproval);
    assert_eq!(versions[0].task, task.id);

    let task_versions: Vec<PlanVersion> = http
        .get(format!("{base}/api/tasks/{}/plans", task.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(task_versions, versions);

    assert_eq!(
        http.get(format!("{base}/api/tasks/nope/plans"))
            .bearer_auth("tok")
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_terminal_reaches_the_runner_and_its_output_comes_back() {
    let dir = std::env::temp_dir().join(format!("lgtm-terminal-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let http = reqwest::Client::new();

    let mut w = ws(&format!("ws://{addr}{RUNNER_WS_PATH}"), false).await;
    let hello = serde_json::json!({
        "type": "hello",
        "token": "tok",
        "info": { "name": "w1", "os": "linux", "arch": "x86_64", "executors": ["claude"] },
        "version": PROTOCOL_VERSION,
    });
    w.send(TMsg::Text(hello.to_string().into())).await.unwrap();
    next_frame(&mut w).await;

    let task: Task = http
        .post(format!("http://{addr}/api/tasks"))
        .bearer_auth("tok")
        .json(&serde_json::json!({
            "repository": "r",
            "base_branch": "main",
            "prompt": "p",
            "executor": "claude",
            "runner": null,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    next_frame(&mut w).await;

    let mut attached = ws(&format!("ws://{addr}/api/tasks/{}/terminal", task.id), true).await;
    let open = next_frame(&mut w).await;
    assert!(matches!(
        serde_json::from_str(open.to_text().unwrap()).unwrap(),
        OrchestratorMessage::TerminalOpen { task_id } if task_id == task.id
    ));

    w.send(TMsg::Text(
        serde_json::to_string(&RunnerMessage::Terminal {
            task_id: task.id.clone(),
            data: "$ ".into(),
        })
        .unwrap()
        .into(),
    ))
    .await
    .unwrap();
    let output = attached.next().await.unwrap().unwrap();
    assert_eq!(output.to_text().unwrap(), "$ ");
    std::fs::remove_dir_all(&dir).ok();
}

/// A pre-rename runner still dials `LEGACY_WORKER_WS_PATH` and lists itself
/// through `/api/workers`; both are kept for one release.
#[tokio::test]
async fn a_pre_rename_runner_still_connects_and_lists() {
    let dir = std::env::temp_dir().join(format!("lgtm-legacy-ws-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut w = ws(&format!("ws://{addr}{LEGACY_WORKER_WS_PATH}"), false).await;
    let hello = serde_json::json!({
        "type": "hello",
        "token": "tok",
        "info": { "name": "old", "os": "linux", "arch": "x86_64", "executors": ["claude"] },
        "version": PROTOCOL_VERSION,
    });
    w.send(TMsg::Text(hello.to_string().into())).await.unwrap();
    let ack = next_frame(&mut w).await;
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(ack.to_text().unwrap()).unwrap(),
        OrchestratorMessage::HelloAck
    ));

    let runners: Vec<RunnerStatus> = reqwest::Client::new()
        .get(format!("http://{addr}/api/workers"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(runners.len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn per_user_tokens_login_revoke_and_survive_restart() {
    std::env::remove_var("GITHUB_TOKEN");
    std::env::remove_var("LINEAR_API_KEY");
    std::env::set_var("PATH", "");
    let dir = std::env::temp_dir().join(format!("lgtm-users-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    let create = |name: &str| {
        let http = http.clone();
        let base = base.clone();
        let name = name.to_string();
        async move {
            let r = http
                .post(format!("{base}/api/users"))
                .bearer_auth("tok")
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 201);
            r.json::<CreatedUser>().await.unwrap()
        }
    };
    let alice = create("alice").await;
    let bob = create("bob").await;
    assert_ne!(alice.token, bob.token);

    // A minted token authenticates; a stranger's does not.
    let r = http
        .get(format!("{base}/api/tasks"))
        .bearer_auth(&alice.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = http
        .get(format!("{base}/api/tasks"))
        .bearer_auth("not-a-token")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // Work created under a per-user token carries its creator; the same
    // request under bob's token carries his.
    let todo = |token: &str| {
        let http = http.clone();
        let base = base.clone();
        let token = token.to_string();
        async move {
            let r = http
                .post(format!("{base}/api/todos"))
                .bearer_auth(&token)
                .json(&serde_json::json!({ "title": "check attribution" }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 201);
            r.json::<Todo>().await.unwrap()
        }
    };
    assert_eq!(
        todo(&alice.token).await.created_by,
        Some(alice.user.id.clone())
    );
    assert_eq!(todo(&bob.token).await.created_by, Some(bob.user.id.clone()));
    assert_eq!(todo("tok").await.created_by, None);

    // Minting and revoking need the shared token; a per-user token gets 403.
    let r = http
        .post(format!("{base}/api/users"))
        .bearer_auth(&alice.token)
        .json(&serde_json::json!({ "name": "spare" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = http
        .post(format!("{base}/api/users/{}/revoke", bob.user.id))
        .bearer_auth(&alice.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    // The shared token revokes bob; his token stops working, his record
    // stays listed.
    let r = http
        .post(format!("{base}/api/users/{}/revoke", bob.user.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = http
        .get(format!("{base}/api/tasks"))
        .bearer_auth(&bob.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let users: Vec<User> = http
        .get(format!("{base}/api/users"))
        .bearer_auth(&alice.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(users.len(), 2);
    assert!(users.iter().any(|u| u.id == bob.user.id && u.revoked));

    // A restart reloads users.json: alice still gets in, bob still does not.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr2,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base2 = format!("http://{addr2}");
    let r = http
        .get(format!("{base2}/api/tasks"))
        .bearer_auth(&alice.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = http
        .get(format!("{base2}/api/tasks"))
        .bearer_auth(&bob.token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    std::fs::remove_dir_all(&dir).ok();
}
