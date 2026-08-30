use futures_util::{SinkExt, StreamExt};
use lgtm_client::Client;
use lgtm_protocol::*;
use tokio_tungstenite::tungstenite::Message as TMsg;

async fn ws(
    url: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let req = url.into_client_request().unwrap();
    tokio_tungstenite::connect_async(req).await.unwrap().0
}

#[tokio::test]
async fn end_to_end() {
    let dir = std::env::temp_dir().join(format!("lgtm-client-smoke-{}", std::process::id()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(lgtm_orchestrator::serve_plain(
        addr,
        "tok".into(),
        dir.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let client = Client::new(format!("http://{addr}"), "tok");

    assert!(client.runners().await.unwrap().is_empty());

    assert!(client.batches().await.unwrap().is_empty());

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
        depends_on: vec![],
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: None,
        requirements: vec![],
        goal: None,
        review_executor: None,
        model: None,
        allowed_hosts: Vec::new(),
        session: None,
    };
    let err = client.create_task(&spec).await.unwrap_err();
    assert!(err.to_string().contains("no eligible runner"), "{err}");

    let mut w = ws(&format!("ws://{addr}{RUNNER_WS_PATH}")).await;
    let info = RunnerInfo {
        name: "w1".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 1,
        ephemeral: false,
        capabilities: vec![],
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
    w.next().await.unwrap().unwrap();

    let task = client.create_task(&spec).await.unwrap();
    let start = w.next().await.unwrap().unwrap();
    assert!(matches!(
        serde_json::from_str::<OrchestratorMessage>(start.to_text().unwrap()).unwrap(),
        OrchestratorMessage::Start { .. }
    ));

    let detail = client.task(&task.id).await.unwrap();
    assert_eq!(detail.task.status, task.status);
    assert!(matches!(
        detail.task.status,
        TaskStatus::Queued | TaskStatus::Running
    ));

    let mut events = client.events(&task.id, 0).await.unwrap();
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
    let event = events.next().await.unwrap();
    assert_eq!(event.event, TaskEvent::Started { model: None });

    std::fs::remove_dir_all(&dir).ok();
}
