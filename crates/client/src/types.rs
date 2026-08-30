//! Request and response bodies, and the events socket handle.

use serde::{Deserialize, Serialize};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

#[derive(Deserialize)]
pub(crate) struct ErrorBody {
    pub(crate) error: String,
}

/// Body of `GET /api/tasks/:id`.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct TaskDetail {
    pub task: lgtm_protocol::Task,
    pub events: Vec<lgtm_protocol::StoredEvent>,
}

#[derive(Serialize)]
pub(crate) struct FollowUp<'a> {
    pub(crate) text: &'a str,
}

/// Body of `POST /api/tasks/:id/retry`. Both `None` retries where it was.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Retry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<lgtm_protocol::Executor>,
}

/// Body of `POST /api/tasks/:id/scratchpad`.
#[derive(Serialize)]
pub(crate) struct Notes<'a> {
    pub(crate) content: &'a str,
}

/// Body of `POST /api/memories`.
#[derive(Serialize)]
pub(crate) struct NewMemory<'a> {
    pub(crate) repository: Option<&'a str>,
    pub(crate) content: &'a str,
}

#[derive(Serialize)]
pub(crate) struct FromIssue<'a> {
    pub(crate) issue: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) executor: lgtm_protocol::Executor,
    pub(crate) worker: Option<&'a str>,
    pub(crate) sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub(crate) requirements: Vec<String>,
}

/// Body of `POST /api/batches`.
#[derive(Serialize, Clone, Debug)]
pub struct BatchRequest {
    pub source: lgtm_protocol::BatchSource,
    pub repository: Option<String>,
    pub base_branch: String,
    pub executor: lgtm_protocol::Executor,
    pub worker: Option<String>,
    pub plan: bool,
    pub approve_plans: bool,
    pub max: u32,
    pub dry_run: bool,
    pub sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub requirements: Vec<String>,
}

/// One issue found for a batch, previewed before (or instead of) import.
#[derive(Deserialize, Clone, Debug)]
pub struct IssuePreview {
    pub key: String,
    pub title: String,
    pub url: String,
}

/// Body of `POST /api/batches`.
#[derive(Deserialize, Clone, Debug)]
pub struct BatchResponse {
    pub batch: Option<lgtm_protocol::Batch>,
    pub issues: Vec<IssuePreview>,
}

/// Body of `GET /api/batches/:id`.
#[derive(Deserialize, Clone, Debug)]
pub struct BatchDetail {
    pub batch: lgtm_protocol::Batch,
    pub summary: lgtm_protocol::BatchSummary,
    pub tasks: Vec<lgtm_protocol::Task>,
}

/// Body of `POST /api/goals`.
#[derive(Serialize, Clone, Debug)]
pub struct NewGoal {
    pub objective: String,
    pub repository: String,
    pub base_branch: String,
    pub executor: lgtm_protocol::Executor,
    pub worker: Option<String>,
    pub plan: bool,
}

/// Body of `GET /api/goals/:id`.
#[derive(Deserialize, Clone, Debug)]
pub struct GoalDetail {
    pub summary: lgtm_protocol::GoalSummary,
    pub tasks: Vec<lgtm_protocol::Task>,
}

#[derive(Serialize)]
/// Body of `POST /api/tasks/from-linear`.
pub struct FromLinear<'a> {
    pub issue: &'a str,
    pub repository: &'a str,
    pub base_branch: &'a str,
    pub executor: lgtm_protocol::Executor,
    pub worker: Option<&'a str>,
    pub sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub requirements: Vec<String>,
}

pub struct EventStream {
    pub(crate) stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

/// Body of `POST /api/todos`.
#[derive(Serialize)]
pub(crate) struct NewTodo<'a> {
    pub(crate) repository: Option<&'a str>,
    pub(crate) title: &'a str,
    pub(crate) description: &'a str,
}

/// Body of `POST /api/todos/:id/promote`.
#[derive(Serialize, Clone, Debug)]
pub struct PromoteTodo {
    pub base_branch: String,
    pub executor: lgtm_protocol::Executor,
    pub worker: Option<String>,
}
