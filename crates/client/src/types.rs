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
    #[serde(default)]
    pub overlaps: Vec<lgtm_protocol::Overlap>,
}

#[derive(Serialize)]
pub(crate) struct FollowUp<'a> {
    pub(crate) text: &'a str,
}

/// Body of `POST /api/tasks/:id/allow`.
#[derive(Serialize)]
pub(crate) struct AllowHost<'a> {
    pub(crate) host: &'a str,
}

/// Body of `POST /api/tasks/:id/permissions`.
#[derive(Serialize)]
pub(crate) struct PermissionRequest<'a> {
    pub(crate) kind: &'a str,
    pub(crate) target: &'a str,
    pub(crate) reason: &'a str,
}

/// Body of `POST /api/tasks/:id/retry`. Both `None` retries where it was.
#[derive(Serialize, Clone, Debug, Default)]
pub struct Retry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor: Option<lgtm_protocol::Executor>,
}

/// Body of `POST /api/tasks/:id/orchestrated`: one step of the loop.
#[derive(Serialize)]
pub struct Orchestrated<'a> {
    pub action: &'a str,
    pub reason: &'a str,
    pub applied: bool,
    pub note: &'a str,
}

/// Body of `POST /api/goals/:id/attention`; `None` clears it.
#[derive(Serialize)]
pub(crate) struct Attention<'a> {
    pub(crate) reason: Option<&'a str>,
}

/// Body of `POST /api/tasks/:id/scratchpad`.
#[derive(Serialize)]
pub(crate) struct Notes<'a> {
    pub(crate) content: &'a str,
}

/// Body of `POST /api/users`.
#[derive(Serialize)]
pub(crate) struct NewUser<'a> {
    pub(crate) name: &'a str,
}

/// Body of `POST /api/memories`.
#[derive(Serialize)]
pub(crate) struct NewMemory<'a> {
    pub(crate) repository: Option<&'a str>,
    pub(crate) content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<lgtm_protocol::MemorySource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) proposed_by: Option<&'a str>,
}

/// Body of `POST /api/tasks/from-issue`.
#[derive(Serialize)]
pub struct FromIssue<'a> {
    pub issue: &'a str,
    pub base_branch: &'a str,
    pub executor: lgtm_protocol::Executor,
    pub runner: Option<&'a str>,
    pub sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub requirements: Vec<String>,
    pub review_executor: Option<lgtm_protocol::Executor>,
    pub model: Option<String>,
}

/// Body of `POST /api/batches`.
#[derive(Serialize, Clone, Debug)]
pub struct BatchRequest {
    pub source: lgtm_protocol::BatchSource,
    pub repository: Option<String>,
    pub base_branch: String,
    pub executor: lgtm_protocol::Executor,
    pub runner: Option<String>,
    pub plan: bool,
    pub approve_plans: bool,
    pub max: u32,
    pub dry_run: bool,
    pub sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub requirements: Vec<String>,
    pub review_executor: Option<lgtm_protocol::Executor>,
    pub model: Option<String>,
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
    pub runner: Option<String>,
    pub plan: bool,
}

/// Body of `POST /api/sessions`.
#[derive(Serialize)]
pub struct NewSession<'a> {
    pub repository: &'a str,
    pub base_branch: &'a str,
    pub title: &'a str,
}

/// Body of `POST /api/sessions/:id/messages`: one message of the thread, and
/// the settings the task it becomes is run with.
#[derive(Serialize)]
pub struct SessionMessage<'a> {
    pub text: &'a str,
    pub executor: lgtm_protocol::Executor,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub requirements: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_executor: Option<lgtm_protocol::Executor>,
    pub kind: lgtm_protocol::TaskKind,
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
    pub runner: Option<&'a str>,
    pub sandbox: Option<lgtm_protocol::SandboxProfile>,
    pub requirements: Vec<String>,
    pub review_executor: Option<lgtm_protocol::Executor>,
    pub model: Option<String>,
}

pub struct EventStream {
    pub(crate) stream: Socket,
}

/// Both directions of a task's terminal: keystrokes out, output back.
pub struct TerminalStream {
    pub(crate) stream: Socket,
}

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Body of `POST /api/todos`.
#[derive(Serialize)]
pub(crate) struct NewTodo<'a> {
    pub(crate) repository: Option<&'a str>,
    pub(crate) title: &'a str,
    pub(crate) description: &'a str,
    pub(crate) priority: lgtm_protocol::Priority,
    pub(crate) assignee: Option<&'a str>,
    pub(crate) blockers: &'a [String],
}

/// Body of `POST /api/todos/:id/promote`.
#[derive(Serialize, Clone, Debug)]
pub struct PromoteTodo {
    pub base_branch: String,
    pub executor: lgtm_protocol::Executor,
    pub runner: Option<String>,
}

/// One line of `GET /api/activity`.
#[derive(Deserialize)]
pub struct ActivityLine {
    pub at: u64,
    pub task: String,
    /// The creator's display name, or `None` for the shared token.
    pub owner: Option<String>,
    pub repository: String,
    pub event: String,
    pub detail: String,
}

/// Body of `POST /api/ask`.
#[derive(Serialize)]
pub(crate) struct AskRequest<'a> {
    pub(crate) question: &'a str,
}

#[derive(Deserialize)]
pub(crate) struct AskResponse {
    pub(crate) answer: String,
}
