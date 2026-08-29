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

#[derive(Serialize)]
pub(crate) struct FromIssue<'a> {
    pub(crate) issue: &'a str,
    pub(crate) base_branch: &'a str,
    pub(crate) executor: lgtm_protocol::Executor,
    pub(crate) worker: Option<&'a str>,
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

#[derive(Serialize)]
/// Body of `POST /api/tasks/from-linear`.
pub struct FromLinear<'a> {
    pub issue: &'a str,
    pub repository: &'a str,
    pub base_branch: &'a str,
    pub executor: lgtm_protocol::Executor,
    pub worker: Option<&'a str>,
}

pub struct EventStream {
    pub(crate) stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}
