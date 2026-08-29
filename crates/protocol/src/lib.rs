//! Wire types shared by the orchestrator, worker agent, and CLI.
//!
//! Every message crosses a process boundary as JSON. Enums are tagged with
//! `type` so a reader can dispatch on one field without knowing the variant.

mod wire;

use serde::{Deserialize, Serialize};

pub use wire::*;

pub const DEFAULT_PORT: u16 = 4750;
/// Bumped on any incompatible change to the messages in `wire.rs`.
pub const PROTOCOL_VERSION: u32 = 1;
/// Orchestrator route the worker agent connects to.
pub const WORKER_WS_PATH: &str = "/ws/worker";

/// Eight lowercase hex characters, assigned by the orchestrator.
pub type TaskId = String;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    Claude,
    Codex,
}

impl Executor {
    /// Executable name looked up on the worker's PATH.
    pub fn binary(self) -> &'static str {
        match self {
            Executor::Claude => "claude",
            Executor::Codex => "codex",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    AwaitingReview,
    Approved,
    /// The pull request was merged from LGTM.
    Merged,
    Rejected,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Approved
                | TaskStatus::Merged
                | TaskStatus::Rejected
                | TaskStatus::Failed
                | TaskStatus::Cancelled
        )
    }
}

/// A GitHub issue a task was created from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct IssueRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}

/// A Linear issue a task was created from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct LinearRef {
    /// Linear's internal issue id (uuid), used by the API.
    pub id: String,
    /// Human identifier such as `ENG-123`.
    pub identifier: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Run the prompt and produce a diff.
    #[default]
    Run,
    /// Read the repository and propose steps; produces a `Plan`, no diff.
    Plan,
}

/// One proposed task in a plan. `depends_on` names other steps' keys.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlanStep {
    pub key: String,
    pub title: String,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<PlanStep>,
}

/// What the developer asked for. Also the body of `POST /api/tasks`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskSpec {
    /// Git URL the worker clones from.
    pub repository: String,
    pub base_branch: String,
    pub prompt: String,
    pub executor: Executor,
    /// Explicit worker name. `None` lets the orchestrator pick.
    pub worker: Option<String>,
    #[serde(default)]
    pub issue: Option<IssueRef>,
    #[serde(default)]
    pub linear: Option<LinearRef>,
    #[serde(default)]
    pub kind: TaskKind,
    /// The plan task this one was created from.
    #[serde(default)]
    pub parent: Option<TaskId>,
    /// Tasks that must be approved before this one may start.
    #[serde(default)]
    pub depends_on: Vec<TaskId>,
    /// The backlog batch this task was imported by.
    #[serde(default)]
    pub batch: Option<String>,
}

impl TaskSpec {
    /// Whether the spec lets `worker` run it: it names that worker, or none.
    pub fn pins(&self, worker: &str) -> bool {
        self.worker.as_deref().is_none_or(|name| name == worker)
    }
}

/// Where a batch's issues came from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BatchSource {
    GithubLabel {
        owner: String,
        repo: String,
        label: String,
    },
    Linear {
        team: String,
        state: String,
    },
}

/// One backlog import: the issues it found became these tasks.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub id: String,
    /// Unix milliseconds.
    pub created_at: u64,
    pub source: BatchSource,
    pub repository: String,
    pub task_ids: Vec<TaskId>,
    /// Approve plan tasks in this batch without a person.
    #[serde(default)]
    pub approve_plans: bool,
}

/// Task counts by state for `GET /api/batches/:id`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct BatchSummary {
    pub queued: u32,
    pub blocked: u32,
    pub running: u32,
    pub awaiting_review: u32,
    pub approved: u32,
    pub merged: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub rejected: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CiState {
    Pending,
    Success,
    Failure,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CiStatus {
    pub state: CiState,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
}

/// One check from the repository's `.lgtm/config.toml`, run by the worker
/// after the agent finished.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ValidationResult {
    pub name: String,
    pub command: String,
    pub ok: bool,
    /// Last lines of combined stdout and stderr.
    pub output_tail: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Blocking,
    Warning,
}

/// One remark from the reviewer agent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Review {
    pub findings: Vec<Finding>,
}

impl Review {
    pub fn has_blocking(&self) -> bool {
        self.findings
            .iter()
            .any(|f| f.severity == Severity::Blocking)
    }
}

/// The repository's `[policy]` bits the orchestrator acts on.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Policy {
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default)]
    pub auto_merge: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskResult {
    /// Branch on the worker holding the committed change, `lgtm/<task-id>`.
    pub branch: String,
    /// `git diff <merge-base> <branch>` output.
    pub diff: String,
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub validation: Vec<ValidationResult>,
    /// Set for `TaskKind::Plan` tasks; `diff` is empty then.
    #[serde(default)]
    pub plan: Option<Plan>,
    #[serde(default)]
    pub review: Option<Review>,
    #[serde(default)]
    pub policy: Option<Policy>,
    /// Sum of every agent run's reported cost for this task run.
    #[serde(default)]
    pub cost_usd: f64,
}

impl TaskResult {
    pub fn validation_failed(&self) -> bool {
        self.validation.iter().any(|v| !v.ok)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// One attempt at a task: from the runner spawning the agent to that run's
/// end. Fix-the-checks and review runs belong to the attempt that started them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Execution {
    /// 1-based, per task.
    pub attempt: u32,
    pub worker: String,
    pub executor: Executor,
    /// Unix milliseconds.
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: ExecutionStatus,
    pub error: Option<String>,
    /// The task's running total at the time this attempt ended; the runner
    /// reports one sum across attempts.
    pub cost_usd: f64,
    pub validation: Vec<ValidationResult>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub status: TaskStatus,
    /// Worker the task was assigned to, once assigned.
    pub worker: Option<String>,
    /// Unix milliseconds.
    pub created_at: u64,
    pub result: Option<TaskResult>,
    pub error: Option<String>,
    #[serde(default)]
    pub pull_request: Option<PullRequest>,
    #[serde(default)]
    pub ci: Option<CiStatus>,
    #[serde(default)]
    pub executions: Vec<Execution>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkerInfo {
    pub name: String,
    /// `std::env::consts::OS`.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    /// Executors whose binary was found on PATH at startup.
    pub executors: Vec<Executor>,
    /// Maximum tasks the worker runs at once.
    #[serde(default = "one")]
    pub slots: u32,
    /// Exits after its tasks; the orchestrator forgets it at once on `Goodbye`.
    #[serde(default)]
    pub ephemeral: bool,
}

fn one() -> u32 {
    1
}

/// Body of `GET /api/workers`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WorkerStatus {
    pub info: WorkerInfo,
    pub running: Vec<TaskId>,
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
