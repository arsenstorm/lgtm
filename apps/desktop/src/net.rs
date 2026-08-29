//! Everything that talks to the orchestrator.
//!
//! reqwest and tungstenite need a tokio reactor, GPUI runs its own executor,
//! so network work lives on one process-wide tokio runtime and results come
//! back over an unbounded channel that the GPUI side drains (see `App::pump`).

use lgtm_client::{BatchRequest, BatchResponse, Client, TaskDetail};
use lgtm_protocol::{Batch, GoalSummary, Stats, StoredEvent, Task, TaskSpec, WorkerStatus};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Stats are computed over every task the orchestrator has, so they ride
/// along on one poll in ten rather than on all of them.
const STATS_EVERY: u32 = 10;
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

pub type Sender = UnboundedSender<Msg>;

/// One refresh of everything the chrome lists.
pub struct Lists {
    pub tasks: Vec<Task>,
    pub workers: Vec<WorkerStatus>,
    pub batches: Vec<Batch>,
    pub goals: Vec<GoalSummary>,
    /// `None` on the polls that skipped stats; the view keeps the last ones.
    pub stats: Option<Stats>,
}

pub enum Msg {
    Lists(Result<Lists, String>),
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
    /// A dry run's issue list, or the batch an import created.
    Batch(Result<BatchResponse, String>),
}

pub enum Action {
    Create(Box<TaskSpec>),
    Cancel,
    Approve,
    Reject,
    Merge,
    Retry,
    Tell(String),
    SetScratchpad(String),
}

pub fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("tokio runtime"))
}

/// Startup probe, before the window: is an orchestrator already answering
/// here? Blocks for at most [`PROBE_TIMEOUT`].
pub fn reachable(orchestrator: &str, token: &str) -> bool {
    let client = Client::new(orchestrator, token);
    runtime().block_on(async move {
        matches!(
            tokio::time::timeout(PROBE_TIMEOUT, client.workers()).await,
            Ok(Ok(_))
        )
    })
}

/// Refreshes the task and worker lists every two seconds, forever.
pub fn poll(client: Client, tx: Sender) {
    runtime().spawn(async move {
        let mut tick = 0u32;
        loop {
            let lists = fetch_lists(&client, tick.is_multiple_of(STATS_EVERY)).await;
            if tx.send(Msg::Lists(lists)).is_err() {
                return;
            }
            tick = tick.wrapping_add(1);
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// One immediate list refresh, so an action's effect shows before the next poll.
pub fn refresh(client: Client, tx: Sender) {
    runtime().spawn(async move {
        let _ = tx.send(Msg::Lists(fetch_lists(&client, false).await));
    });
}

async fn fetch_lists(client: &Client, with_stats: bool) -> Result<Lists, String> {
    let tasks = client.tasks().await.map_err(|e| e.to_string())?;
    let workers = client.workers().await.map_err(|e| e.to_string())?;
    // A failing secondary call must not take down the whole refresh.
    let batches = client.batches().await.unwrap_or_default();
    let goals = client.goals().await.unwrap_or_default();
    let stats = match with_stats {
        true => client.stats(None).await.ok(),
        false => None,
    };
    Ok(Lists {
        tasks,
        workers,
        batches,
        goals,
        stats,
    })
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

/// Previews (`dry_run`) or creates a batch.
pub fn create_batch(client: Client, request: BatchRequest, tx: Sender) {
    runtime().spawn(async move {
        let result = client.create_batch(&request).await;
        let _ = tx.send(Msg::Batch(result.map_err(|e| e.to_string())));
    });
}

pub fn act(client: Client, id: String, action: Action, tx: Sender) {
    runtime().spawn(async move {
        let result = match action {
            Action::Create(spec) => client.create_task(&spec).await.map(Some),
            Action::Cancel => client.cancel(&id).await.map(|_| None),
            Action::Approve => client.approve(&id).await.map(|_| None),
            Action::Reject => client.reject(&id).await.map(|_| None),
            Action::Merge => client.merge(&id).await.map(|_| None),
            Action::Retry => client
                .retry(&id, &lgtm_client::Retry::default())
                .await
                .map(|_| None),
            Action::Tell(text) => client.tell(&id, &text).await.map(|_| None),
            Action::SetScratchpad(notes) => client.set_scratchpad(&id, &notes).await.map(|_| None),
        };
        let _ = tx.send(Msg::Action(result.map_err(|e| e.to_string())));
    });
}
