//! Everything that talks to the orchestrator.
//!
//! reqwest and tungstenite need a tokio reactor, GPUI runs its own executor,
//! so network work lives on one process-wide tokio runtime and results come
//! back over an unbounded channel that the GPUI side drains (see `App::pump`).

use lgtm_client::{BatchRequest, BatchResponse, Client, NewSession, SessionMessage, TaskDetail};
use lgtm_protocol::{
    Batch, GoalSummary, RunnerStatus, Session, SessionDetail, Stats, StoredEvent, Task, TaskSpec,
    TaskStatus,
};
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
/// How much of the first message names the thread it starts.
const SESSION_TITLE: usize = 60;

pub type Sender = UnboundedSender<Msg>;

/// One refresh of everything the chrome lists.
pub struct Lists {
    pub tasks: Vec<Task>,
    pub runners: Vec<RunnerStatus>,
    pub batches: Vec<Batch>,
    pub goals: Vec<GoalSummary>,
    pub sessions: Vec<Session>,
    /// `None` on the polls that skipped stats; the view keeps the last ones.
    pub stats: Option<Stats>,
}

pub enum Msg {
    Lists(Result<Lists, String>),
    Detail {
        generation: u64,
        detail: Box<TaskDetail>,
    },
    Live {
        generation: u64,
        event: StoredEvent,
    },
    /// The open session, and the events of the one task whose detail this poll
    /// fetched, if any.
    Session {
        generation: u64,
        detail: Box<SessionDetail>,
        task: Option<(String, Vec<StoredEvent>)>,
    },
    /// An action went through; the next poll shows what it did.
    Action(Result<(), String>),
    /// A session to open: one just created, with or without a first message.
    Opened(Result<String, String>),
    /// A dry run's issue list, or the batch an import created.
    Batch(Result<BatchResponse, String>),
}

pub enum Action {
    /// A new thread, with the composer's text as its first message.
    StartSession(Box<TaskSpec>),
    /// One more message in the thread `act` was given the id of.
    SendMessage(Box<TaskSpec>),
    Cancel,
    Approve,
    Reject,
    Merge,
    Retry,
    Tell(String),
    SetScratchpad(String),
    AllowHost(String),
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
            tokio::time::timeout(PROBE_TIMEOUT, client.runners()).await,
            Ok(Ok(_))
        )
    })
}

/// Refreshes the task and runner lists every two seconds, forever.
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
    let runners = client.runners().await.map_err(|e| e.to_string())?;
    // A failing secondary call must not take down the whole refresh.
    let batches = client.batches().await.unwrap_or_default();
    let goals = client.goals().await.unwrap_or_default();
    let sessions = client.sessions(None).await.unwrap_or_default();
    let stats = match with_stats {
        true => client.stats(None).await.ok(),
        false => None,
    };
    Ok(Lists {
        tasks,
        runners,
        batches,
        goals,
        sessions,
        stats,
    })
}

/// Refreshes the open session every two seconds until the caller aborts it.
/// A failed poll is skipped rather than ending the loop, so a blip does not
/// leave the page frozen on what it last had.
pub fn watch_session(client: Client, id: String, generation: u64, tx: Sender) -> JoinHandle<()> {
    runtime().spawn(async move {
        loop {
            if let Some((detail, task)) = session_snapshot(&client, &id).await {
                let msg = Msg::Session {
                    generation,
                    detail: Box::new(detail),
                    task,
                };
                if tx.send(msg).is_err() {
                    return;
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
}

/// The session, plus the events of its newest running task. Only that one
/// task's detail is fetched: it is the only card whose body changes per tick.
async fn session_snapshot(
    client: &Client,
    id: &str,
) -> Option<(SessionDetail, Option<(String, Vec<StoredEvent>)>)> {
    let detail = client.session(id).await.ok()?;
    let running = detail
        .tasks
        .iter()
        .rev()
        .find(|task| task.status == TaskStatus::Running);
    let events = match running {
        Some(task) => (client.task(&task.id).await)
            .ok()
            .map(|detail| (task.id.clone(), detail.events)),
        None => None,
    };
    Some((detail, events))
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
        let detail = Box::new(detail);
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

/// An empty thread, waiting for its first message to name it.
pub fn new_session(client: Client, repository: String, base_branch: String, tx: Sender) {
    runtime().spawn(async move {
        let body = NewSession {
            repository: &repository,
            base_branch: &base_branch,
            title: "",
        };
        let result = client.create_session(&body).await.map(|session| session.id);
        let _ = tx.send(Msg::Opened(result.map_err(|e| e.to_string())));
    });
}

/// The composer's choices as one message of a thread. `kind` has no place in
/// a thread, so a Plan chip does not survive the trip.
fn message(spec: &TaskSpec) -> SessionMessage<'_> {
    SessionMessage {
        text: &spec.prompt,
        executor: spec.executor,
        runner: spec.runner.as_deref(),
        sandbox: spec.sandbox,
        requirements: spec.requirements.clone(),
        model: spec.model.as_deref(),
        review_executor: spec.review_executor,
    }
}

async fn start_session(client: &Client, spec: &TaskSpec) -> anyhow::Result<String> {
    let body = NewSession {
        repository: &spec.repository,
        base_branch: &spec.base_branch,
        title: &crate::labels::prompt_preview(&spec.prompt, SESSION_TITLE),
    };
    let session = client.create_session(&body).await?;
    client.send_message(&session.id, &message(spec)).await?;
    Ok(session.id)
}

pub fn act(client: Client, id: String, action: Action, tx: Sender) {
    runtime().spawn(async move {
        let msg = match action {
            Action::StartSession(spec) => Msg::Opened(
                start_session(&client, &spec)
                    .await
                    .map_err(|e| e.to_string()),
            ),
            other => Msg::Action(
                one_task(&client, &id, other)
                    .await
                    .map_err(|e| e.to_string()),
            ),
        };
        let _ = tx.send(msg);
    });
}

/// Every action that acts on the task or session `id` names.
async fn one_task(client: &Client, id: &str, action: Action) -> anyhow::Result<()> {
    match action {
        Action::SendMessage(spec) => client.send_message(id, &message(&spec)).await.map(|_| ()),
        Action::Cancel => client.cancel(id).await.map(|_| ()),
        Action::Approve => client.approve(id).await.map(|_| ()),
        Action::Reject => client.reject(id).await.map(|_| ()),
        Action::Merge => client.merge(id).await.map(|_| ()),
        Action::Retry => client
            .retry(id, &lgtm_client::Retry::default())
            .await
            .map(|_| ()),
        Action::Tell(text) => client.tell(id, &text).await.map(|_| ()),
        Action::SetScratchpad(notes) => client.set_scratchpad(id, &notes).await.map(|_| ()),
        Action::AllowHost(host) => client.allow_host(id, &host).await.map(|_| ()),
        // Answered before this is reached; it does not act on one task.
        Action::StartSession(_) => Ok(()),
    }
}
