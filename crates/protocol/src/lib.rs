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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Executor {
    #[default]
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

/// How much of the host an agent run may touch. Enforcement is the runner's
/// job and is per platform, so a profile can mean less than it says here.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxProfile {
    /// Ordinary host-user access.
    Off,
    /// Isolated worktree, project files only, stripped environment, limits.
    #[default]
    Standard,
    /// Standard plus a container/VM boundary and no network by default.
    Strict,
}

impl SandboxProfile {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(SandboxProfile::Off),
            "standard" => Some(SandboxProfile::Standard),
            "strict" => Some(SandboxProfile::Strict),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SandboxProfile::Off => "off",
            SandboxProfile::Standard => "standard",
            SandboxProfile::Strict => "strict",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    AwaitingReview,
    /// A follow-up was sent; the run for it has not started yet.
    ChangesRequested,
    Approved,
    /// The pull request was merged from LGTM.
    Merged,
    Rejected,
    Failed,
    /// The runner killed the agent at the policy's `timeout_secs`.
    TimedOut,
    /// The worker running it went away and did not come back.
    RunnerLost,
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
                | TaskStatus::TimedOut
                | TaskStatus::RunnerLost
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
    /// `None` defers to the repository's `[sandbox] profile`, then `Standard`.
    #[serde(default)]
    pub sandbox: Option<SandboxProfile>,
    /// Every one must appear in a worker's `capabilities` for it to run this.
    #[serde(default)]
    pub requirements: Vec<String>,
    /// The goal this task works toward.
    #[serde(default)]
    pub goal: Option<String>,
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

/// A fact, constraint, or decision every run in a repository should know.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Memory {
    pub id: String,
    /// `None` applies to every repository the orchestrator sees.
    pub repository: Option<String>,
    pub content: String,
    /// Unix milliseconds.
    pub created_at: u64,
}

impl BatchSummary {
    /// Every task counted, whatever state it is in.
    pub fn total(&self) -> u32 {
        self.queued
            + self.blocked
            + self.running
            + self.awaiting_review
            + self.approved
            + self.merged
            + self.failed
            + self.cancelled
            + self.rejected
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Draft,
    Planning,
    Running,
    Review,
    Blocked,
    Completed,
    Cancelled,
}

/// What the developer wants; tasks are how LGTM gets there.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Goal {
    pub id: String,
    pub objective: String,
    pub repository: String,
    /// Unix milliseconds.
    pub created_at: u64,
}

impl Memory {
    /// Whether a run in `repository` should be told this.
    pub fn applies_to(&self, repository: &str) -> bool {
        self.repository.as_deref().is_none_or(|r| r == repository)
    }
}

/// The block prepended to an agent prompt, or empty when there is nothing.
pub fn knowledge_block(memories: &[Memory]) -> String {
    if memories.is_empty() {
        return String::new();
    }
    let mut out = String::from("Project knowledge (from the team, treat as fact):\n");
    for memory in memories {
        out.push_str("- ");
        out.push_str(&memory.content);
        out.push('\n');
    }
    out.push('\n');
    out
}

/// Body of `GET /api/goals` items and `GET /api/goals/:id`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct GoalSummary {
    pub goal: Goal,
    pub status: GoalStatus,
    pub tasks: BatchSummary,
}

/// Derived from the goal's tasks, so it is never stored.
pub fn goal_status(tasks: &[&Task]) -> GoalStatus {
    let any = |f: fn(&Task) -> bool| tasks.iter().any(|task| f(task));
    let all = |f: fn(&Task) -> bool| tasks.iter().all(|task| f(task));
    if tasks.is_empty() {
        GoalStatus::Draft
    } else if any(|t| t.spec.kind == TaskKind::Plan && !t.status.is_terminal()) {
        GoalStatus::Planning
    } else if any(|t| {
        matches!(
            t.status,
            TaskStatus::Queued | TaskStatus::Running | TaskStatus::ChangesRequested
        )
    }) {
        GoalStatus::Running
    } else if any(|t| t.status == TaskStatus::AwaitingReview) {
        GoalStatus::Review
    } else if all(|t| matches!(t.status, TaskStatus::Approved | TaskStatus::Merged)) {
        GoalStatus::Completed
    } else if all(|t| matches!(t.status, TaskStatus::Cancelled | TaskStatus::Rejected)) {
        GoalStatus::Cancelled
    } else {
        GoalStatus::Blocked
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Open,
    InProgress,
    Done,
}

/// A note about work to do; not yet a task, and cheaper than one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Todo {
    pub id: String,
    /// Git URL; `None` is not tied to a repository.
    pub repository: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: TodoStatus,
    /// Unix milliseconds.
    pub created_at: u64,
    /// The task it was promoted into, which moves it to `InProgress`.
    #[serde(default)]
    pub task: Option<TaskId>,
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Policy {
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default)]
    pub auto_merge: bool,
    /// Refuse auto-approve when the diff has more added+removed lines than this.
    #[serde(default)]
    pub max_diff_lines: Option<u32>,
    /// Paths (with `*` wildcards) an automatic approval must not touch.
    #[serde(default)]
    pub protected_files: Vec<String>,
    /// Refuse auto-approve when the run cost more than this.
    #[serde(default)]
    pub budget_per_task_usd: Option<f64>,
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
    /// The agent's own notes from `.lgtm/scratchpad.md`, kept so a retry or a
    /// person can pick up where it stopped.
    #[serde(default)]
    pub scratchpad: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    AwaitingApproval,
    Approved,
    Rejected,
    Replanning,
    Superseded,
}

/// One version of a plan task's plan, read back out of its events.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlanVersion {
    pub task: TaskId,
    pub goal: Option<String>,
    /// 1-based, per plan task.
    pub version: u32,
    pub status: PlanStatus,
    /// Unix milliseconds, when the agent produced it.
    pub created_at: u64,
    pub plan: Plan,
}

/// What a plan task's latest version is doing, from its task status. A
/// status this doesn't otherwise recognize (failed, timed out, lost,
/// cancelled) leaves the plan as good as rejected: nothing will act on it.
fn plan_status_for(status: TaskStatus) -> PlanStatus {
    match status {
        TaskStatus::AwaitingReview => PlanStatus::AwaitingApproval,
        TaskStatus::Approved | TaskStatus::Merged => PlanStatus::Approved,
        TaskStatus::Rejected => PlanStatus::Rejected,
        TaskStatus::Running | TaskStatus::Queued | TaskStatus::ChangesRequested => {
            PlanStatus::Replanning
        }
        _ => PlanStatus::Rejected,
    }
}

fn completed_plan(stored: &StoredEvent) -> Option<(u64, Plan)> {
    let TaskEvent::Completed { result } = &stored.event else {
        return None;
    };
    result.plan.clone().map(|plan| (stored.at, plan))
}

/// Every version a plan task has produced, oldest first; empty for a run
/// task. Nothing new is stored for this: each `Completed` event that carried
/// a plan already is one version.
pub fn plan_versions(task: &Task, events: &[StoredEvent]) -> Vec<PlanVersion> {
    let mut out: Vec<PlanVersion> = events
        .iter()
        .filter_map(completed_plan)
        .enumerate()
        .map(|(i, (created_at, plan))| PlanVersion {
            task: task.id.clone(),
            goal: task.spec.goal.clone(),
            version: i as u32 + 1,
            status: PlanStatus::Superseded,
            created_at,
            plan,
        })
        .collect();
    if let Some(last) = out.last_mut() {
        last.status = plan_status_for(task.status);
    }
    out
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ExecutorStats {
    pub executor: Executor,
    pub attempts: u32,
    pub completed: u32,
    pub failed: u32,
}

/// Counts and medians over the tasks created inside one window.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Stats {
    /// Unix milliseconds; tasks created before this are left out.
    pub since: u64,
    pub tasks: u32,
    pub queued: u32,
    pub running: u32,
    pub awaiting_review: u32,
    pub approved: u32,
    pub merged: u32,
    pub failed: u32,
    pub cancelled: u32,
    pub rejected: u32,
    /// Median of every finished execution's `finished_at - started_at`, ms.
    pub median_execution_ms: u64,
    /// Median of `first Started - created_at` over tasks that started, ms.
    pub median_queue_ms: u64,
    pub retried_tasks: u32,
    pub cost_usd: f64,
    pub by_executor: Vec<ExecutorStats>,
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
    /// Lowercase tags: `os:<os>`, `arch:<arch>`, and each toolchain binary found on PATH.
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl WorkerInfo {
    /// True when every requirement is in `capabilities`.
    pub fn has_all(&self, requirements: &[String]) -> bool {
        requirements.iter().all(|r| self.capabilities.contains(r))
    }
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
