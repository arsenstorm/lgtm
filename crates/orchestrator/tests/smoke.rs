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

#[tokio::test]
async fn end_to_end() {
    let dir = std::env::temp_dir().join(format!("lgtm-smoke-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve(addr, "tok".into(), dir.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let base = format!("http://{addr}");
    let http = reqwest::Client::new();

    // unauthorized
    let r = http
        .get(format!("{base}/api/workers"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(r.text().await.unwrap(), r#"{"error":"unauthorized"}"#);

    // no worker yet -> 409
    let spec = TaskSpec {
        repository: "r".into(),
        base_branch: "main".into(),
        prompt: "p".into(),
        executor: Executor::Claude,
        worker: None,
    };
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(r.text().await.unwrap(), r#"{"error":"no eligible worker"}"#);

    // bad json -> 400
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .header("content-type", "application/json")
        .body("{")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // worker connects
    let mut w = ws(&format!("ws://{addr}{WORKER_WS_PATH}"), false).await;
    let info = WorkerInfo {
        name: "w1".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
    };
    w.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Hello {
            token: "tok".into(),
            info,
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let ack = w.next().await.unwrap().unwrap();
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(ack.to_text().unwrap()).unwrap(),
        OrchestratorMessage::HelloAck
    ));

    let workers: Vec<WorkerStatus> = http
        .get(format!("{base}/api/workers"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(workers.len(), 1);

    // create task -> worker gets Start
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    let task: Task = r.json().await.unwrap();
    let start = w.next().await.unwrap().unwrap();
    let OrchestratorMessage::Start { task: started } =
        serde_json::from_str(start.to_text().unwrap()).unwrap()
    else {
        panic!()
    };
    assert_eq!(started.id, task.id);
    assert_eq!(started.worker.as_deref(), Some("w1"));

    // approve before review -> 409
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

    // events socket, live
    let mut ev = ws(&format!("ws://{addr}/api/tasks/{}/events", task.id), true).await;
    w.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Started,
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let first: StoredEvent =
        serde_json::from_str(ev.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert_eq!(first.event, TaskEvent::Started);

    let result = TaskResult {
        branch: format!("lgtm/{}", task.id),
        diff: "d".into(),
        changed_files: vec!["a".into()],
    };
    w.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Completed { result },
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let second: StoredEvent =
        serde_json::from_str(ev.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
    assert!(matches!(second.event, TaskEvent::Completed { .. }));
    // socket closes after a terminal event
    assert!(matches!(ev.next().await, Some(Ok(TMsg::Close(_))) | None));

    // detail endpoint
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

    // approve -> worker gets Push
    let r = http
        .post(format!("{base}/api/tasks/{}/approve", task.id))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let push = w.next().await.unwrap().unwrap();
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(push.to_text().unwrap()).unwrap(),
        OrchestratorMessage::Push { .. }
    ));

    // replay on a terminal task closes right away
    w.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Pushed { branch: "b".into() },
        })
        .unwrap(),
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

    // second task, then worker drops -> synthetic Failed
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    let task2: Task = r.json().await.unwrap();
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
    assert_eq!(detail["task"]["status"], "failed");
    assert_eq!(detail["task"]["error"], "worker disconnected");
    let workers: Vec<WorkerStatus> = http
        .get(format!("{base}/api/workers"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(workers.is_empty());

    // persisted files, sorted list
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
    assert!(tasks[0].created_at <= tasks[1].created_at);

    // reconnect under the same name: the old socket's cleanup must not evict the new one
    let mut w1 = ws(&format!("ws://{addr}{WORKER_WS_PATH}"), false).await;
    let info = WorkerInfo {
        name: "w2".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
    };
    w1.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Hello {
            token: "tok".into(),
            info: info.clone(),
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    w1.next().await.unwrap().unwrap();
    let mut w2 = ws(&format!("ws://{addr}{WORKER_WS_PATH}"), false).await;
    w2.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Hello {
            token: "tok".into(),
            info,
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    w2.next().await.unwrap().unwrap();
    drop(w1);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let workers: Vec<WorkerStatus> = http
        .get(format!("{base}/api/workers"))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        workers.len(),
        1,
        "new registration survives the old socket's cleanup"
    );
    // the live connection still works
    let r = http
        .post(format!("{base}/api/tasks"))
        .bearer_auth("tok")
        .json(&spec)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(
            w2.next().await.unwrap().unwrap().to_text().unwrap()
        )
        .unwrap(),
        OrchestratorMessage::Start { .. }
    ));

    // bad token is rejected
    let mut bad = ws(&format!("ws://{addr}{WORKER_WS_PATH}"), false).await;
    let info = WorkerInfo {
        name: "evil".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
    };
    bad.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Hello {
            token: "nope".into(),
            info,
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    assert!(matches!(bad.next().await, Some(Ok(TMsg::Close(_))) | None));

    // 404s
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
    // restart: queued/running tasks are failed on load
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve(addr2, "tok".into(), dir.clone()));
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
    assert_eq!(tasks.len(), 3);
    let queued = tasks
        .iter()
        .find(|t| t.id != task.id && t.id != task2.id)
        .unwrap();
    assert_eq!(queued.status, TaskStatus::Failed);
    assert_eq!(queued.error.as_deref(), Some("orchestrator restarted"));
    assert_eq!(
        tasks.iter().find(|t| t.id == task.id).unwrap().status,
        TaskStatus::Approved
    );
    std::fs::remove_dir_all(&dir).ok();
}
