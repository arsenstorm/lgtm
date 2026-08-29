//! Everything that talks to the orchestrator.
//!
//! reqwest and tungstenite need a tokio reactor, GPUI runs its own executor,
//! so network work lives on one process-wide tokio runtime and results come
//! back over an unbounded channel that the GPUI side drains (see `App::pump`).

use lgtm_client::{Client, TaskDetail};
use lgtm_protocol::{StoredEvent, Task, TaskSpec, WorkerStatus};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub type Sender = UnboundedSender<Msg>;

pub enum Msg {
    Lists(Result<(Vec<Task>, Vec<WorkerStatus>), String>),
    Detail {
        generation: u64,
        detail: TaskDetail,
    },
    Live {
        generation: u64,
        event: StoredEvent,
    },
    /// `Ok(Some(task))` is a freshly created task the UI should select.
    Action(Result<Option<Task>, String>),
}

pub enum Action {
    Create(Box<TaskSpec>),
    Cancel,
    Approve,
    Reject,
    Merge,
    Tell(String),
}

fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

/// Refreshes the task and worker lists every two seconds, forever.
pub fn poll(client: Client, tx: Sender) {
    runtime().spawn(async move {
        loop {
            if tx.send(Msg::Lists(fetch_lists(&client).await)).is_err() {
                return;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// One immediate list refresh, so an action's effect shows before the next poll.
pub fn refresh(client: Client, tx: Sender) {
    runtime().spawn(async move {
        let _ = tx.send(Msg::Lists(fetch_lists(&client).await));
    });
}

async fn fetch_lists(client: &Client) -> Result<(Vec<Task>, Vec<WorkerStatus>), String> {
    let tasks = client.tasks().await.map_err(|e| e.to_string())?;
    let workers = client.workers().await.map_err(|e| e.to_string())?;
    Ok((tasks, workers))
}

/// Loads the task's stored events, then streams live ones until the task ends.
/// The caller aborts the handle when another task is selected; `generation`
/// lets it drop events already in flight from the previous stream.
pub fn watch(client: Client, id: String, generation: u64, tx: Sender) -> JoinHandle<()> {
    runtime().spawn(async move {
        let Ok(detail) = client.task(&id).await else {
            return;
        };
        let from = detail.events.len();
        if tx.send(Msg::Detail { generation, detail }).is_err() {
            return;
        }
        let Ok(mut stream) = client.events(&id, from).await else {
            return;
        };
        while let Some(event) = stream.next().await {
            if tx.send(Msg::Live { generation, event }).is_err() {
                return;
            }
        }
    })
}

pub fn act(client: Client, id: String, action: Action, tx: Sender) {
    runtime().spawn(async move {
        let result = match action {
            Action::Create(spec) => client.create_task(&spec).await.map(Some),
            Action::Cancel => client.cancel(&id).await.map(|_| None),
            Action::Approve => client.approve(&id).await.map(|_| None),
            Action::Reject => client.reject(&id).await.map(|_| None),
            Action::Merge => client.merge(&id).await.map(|_| None),
            Action::Tell(text) => client.tell(&id, &text).await.map(|_| None),
        };
        let _ = tx.send(Msg::Action(result.map_err(|e| e.to_string())));
    });
}
