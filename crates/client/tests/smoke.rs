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
    tokio::spawn(lgtm_orchestrator::serve(addr, "tok".into(), dir.clone()));
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let client = Client::new(format!("http://{addr}"), "tok");

    // no workers yet
    assert!(client.workers().await.unwrap().is_empty());

    // no eligible worker -> error
    let spec = TaskSpec {
        repository: "r".into(),
        base_branch: "main".into(),
        prompt: "p".into(),
        executor: Executor::Claude,
        worker: None,
        issue: None,
    };
    let err = client.create_task(&spec).await.unwrap_err();
    assert!(err.to_string().contains("no eligible worker"), "{err}");

    // fake worker connects with one slot
    let mut w = ws(&format!("ws://{addr}{WORKER_WS_PATH}")).await;
    let info = WorkerInfo {
        name: "w1".into(),
        os: "linux".into(),
        arch: "x86_64".into(),
        executors: vec![Executor::Claude],
        slots: 1,
    };
    w.send(TMsg::Text(
        serde_json::to_string(&WorkerMessage::Hello {
            token: "tok".into(),
            info,
            running: Vec::new(),
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    w.next().await.unwrap().unwrap(); // HelloAck

    let task = client.create_task(&spec).await.unwrap();
    // worker gets Start
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
        serde_json::to_string(&WorkerMessage::Event {
            task_id: task.id.clone(),
            event: TaskEvent::Started,
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let event = events.next().await.unwrap();
    assert_eq!(event.event, TaskEvent::Started);

    std::fs::remove_dir_all(&dir).ok();
}
