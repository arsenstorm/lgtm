//! Wire types shared by the orchestrator, runner agent, and CLI.
//!
//! Every message crosses a process boundary as JSON. Enums are tagged with
//! `type` so a reader can dispatch on one field without knowing the variant.

mod wire;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use wire::*;

pub const DEFAULT_PORT: u16 = 4750;
/// Bumped on any incompatible change to the messages in `wire.rs`.
pub const PROTOCOL_VERSION: u32 = 1;
/// Orchestrator route the runner agent connects to.
pub const RUNNER_WS_PATH: &str = "/ws/runner";
/// Kept one release so a runner built before the rename can still connect.
pub const LEGACY_WORKER_WS_PATH: &str = "/ws/worker";

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
    /// Executable name looked up on the runner's PATH.
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
    /// Standard plus the repository's own `[sandbox] readable/writable/denied`.
    Custom,
}

impl SandboxProfile {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(SandboxProfile::Off),
            "standard" => Some(SandboxProfile::Standard),
            "strict" => Some(SandboxProfile::Strict),
            "custom" => Some(SandboxProfile::Custom),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            SandboxProfile::Off => "off",
            SandboxProfile::Standard => "standard",
            SandboxProfile::Strict => "strict",
            SandboxProfile::Custom => "custom",
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
    /// The branch no longer rebases onto its base; a follow-up asks the agent
    /// to resolve it.
    Conflicted,
    Approved,
    /// The pull request was merged from LGTM.
    Merged,
    Rejected,
    Failed,
    /// The runner killed the agent at the policy's `timeout_secs`.
    TimedOut,
    /// The runner running it went away and did not come back.
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

/// What a dependency must have reached for a task waiting on it to start.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DependsOn {
    /// The dependency was approved or merged (its branch exists on origin).
    #[default]
    Approved,
    /// The dependency finished a run: awaiting review or later.
    Completed,
    /// The dependency's pull request was merged.
    Merged,
}

impl DependsOn {
    /// Whether a dependency in `status` satisfies this condition.
    pub fn met(self, status: TaskStatus) -> bool {
        match self {
            DependsOn::Approved => matches!(status, TaskStatus::Approved | TaskStatus::Merged),
            DependsOn::Completed => matches!(
                status,
                TaskStatus::AwaitingReview
                    | TaskStatus::ChangesRequested
                    | TaskStatus::Conflicted
                    | TaskStatus::Approved
                    | TaskStatus::Merged
            ),
            DependsOn::Merged => status == TaskStatus::Merged,
        }
    }
}

/// What the developer asked for. Also the body of `POST /api/tasks`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskSpec {
    /// Git URL the runner clones from.
    pub repository: String,
    pub base_branch: String,
    pub prompt: String,
    pub executor: Executor,
    /// Explicit runner name. `None` lets the orchestrator pick.
    #[serde(alias = "worker")]
    pub runner: Option<String>,
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
    /// What every id in `depends_on` must have reached before this task starts.
    #[serde(default)]
    pub depends_on_condition: DependsOn,
    /// The backlog batch this task was imported by.
    #[serde(default)]
    pub batch: Option<String>,
    /// `None` defers to the repository's `[sandbox] profile`, then `Standard`.
    #[serde(default)]
    pub sandbox: Option<SandboxProfile>,
    /// Every one must appear in a runner's `capabilities` for it to run this.
    #[serde(default)]
    pub requirements: Vec<String>,
    /// The goal this task works toward.
    #[serde(default)]
    pub goal: Option<String>,
    /// Harness for the review pass; `None` defers to `[policy] review_executor`, then auto.
    #[serde(default)]
    pub review_executor: Option<Executor>,
    /// Passed to the harness as its model flag; `None` is the harness default.
    #[serde(default)]
    pub model: Option<String>,
    /// Hosts a person allowed for this task on top of the repository's allowlist.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    /// The chat thread this task was sent from.
    #[serde(default)]
    pub session: Option<String>,
    /// Stamped by the orchestrator from the authenticated user; anything a
    /// client sends here is overwritten. Only `POST /api/tasks` deserializes
    /// a client `TaskSpec` today — a new endpoint that does must overwrite
    /// this field the same way, or it accepts spoofed attribution.
    #[serde(default)]
    pub created_by: Option<String>,
}

impl TaskSpec {
    /// Whether the spec lets `runner` run it: it names that runner, or none.
    pub fn pins(&self, runner: &str) -> bool {
        self.runner.as_deref().is_none_or(|name| name == runner)
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
    /// The workspace this belongs to; one per orchestrator until teams exist.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The user who created this; `None` for the shared token or automation.
    #[serde(default)]
    pub created_by: Option<String>,
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

/// Who wrote a `Memory`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    #[default]
    User,
    Agent,
}

/// Whether a person has signed off on a `Memory`. An agent should not be
/// able to write what every later run is told, so an agent-sourced memory
/// starts `AgentProposed` and stays out of `knowledge_block` until approved.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    AgentProposed,
    UserApproved,
}

/// Stored memories predate `Verification`; treat them as already approved.
fn approved() -> Verification {
    Verification::UserApproved
}

/// A person who uses this orchestrator. Their tokens live in the
/// orchestrator's own store and never travel in this type.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub name: String,
    /// Unix milliseconds.
    pub created_at: u64,
    /// A revoked user's tokens stop authenticating; the record stays so
    /// `created_by` on old objects still resolves to a name.
    #[serde(default)]
    pub revoked: bool,
}

/// `POST /api/users` response: the one place the minted token is ever shown.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CreatedUser {
    pub user: User,
    pub token: String,
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
    #[serde(default)]
    pub source: MemorySource,
    #[serde(default = "approved")]
    pub verification: Verification,
    /// The task that proposed it, when `source` is `Agent`.
    #[serde(default)]
    pub proposed_by: Option<TaskId>,
    /// The workspace this belongs to; one per orchestrator until teams exist.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The user who created this; `None` for the shared token or automation.
    #[serde(default)]
    pub created_by: Option<String>,
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
    /// Why the orchestration loop stopped and left the goal to a person.
    /// Cleared by the next message or task under the goal.
    #[serde(default)]
    pub attention: Option<String>,
    /// The workspace this belongs to; one per orchestrator until teams exist.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The user who created this; `None` for the shared token or automation.
    #[serde(default)]
    pub created_by: Option<String>,
}

impl Memory {
    /// Whether a run in `repository` should be told this: it applies to the
    /// repository, and a person has approved it.
    pub fn is_told_to(&self, repository: &str) -> bool {
        self.verification == Verification::UserApproved
            && self.repository.as_deref().is_none_or(|r| r == repository)
    }
}

/// One chat thread in a repository; each message in it becomes a task.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub repository: String,
    pub base_branch: String,
    /// The first message, cut to 60 chars; empty until one is sent.
    pub title: String,
    /// Unix milliseconds.
    pub created_at: u64,
    /// The workspace this belongs to; one per orchestrator until teams exist.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The user who created this; `None` for the shared token or automation.
    #[serde(default)]
    pub created_by: Option<String>,
}

/// Body of `GET /api/sessions/:id`: the thread is its tasks in creation order.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SessionDetail {
    pub session: Session,
    pub tasks: Vec<Task>,
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
pub fn goal_status(goal: &Goal, tasks: &[&Task]) -> GoalStatus {
    let any = |f: fn(&Task) -> bool| tasks.iter().any(|task| f(task));
    let all = |f: fn(&Task) -> bool| tasks.iter().all(|task| f(task));
    if goal.attention.is_some() {
        GoalStatus::Blocked
    } else if tasks.is_empty() {
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
    } else if any(|t| {
        matches!(
            t.status,
            TaskStatus::AwaitingReview | TaskStatus::Conflicted
        )
    }) {
        GoalStatus::Review
    } else if all(|t| matches!(t.status, TaskStatus::Approved | TaskStatus::Merged)) {
        GoalStatus::Completed
    } else if all(|t| matches!(t.status, TaskStatus::Cancelled | TaskStatus::Rejected)) {
        GoalStatus::Cancelled
    } else {
        GoalStatus::Blocked
    }
}

/// How much of the prompt a notification line carries.
const TITLE_LEN: usize = 60;

/// The first line of `text`, cut to [`TITLE_LEN`] characters.
pub fn first_line_title(text: &str) -> String {
    let first = text.lines().next().unwrap_or_default().trim();
    first.chars().take(TITLE_LEN).collect()
}

/// The first line of the prompt, cut to [`TITLE_LEN`] characters.
fn title(task: &Task) -> String {
    first_line_title(&task.spec.prompt)
}

fn line(task: &Task, why: &str) -> String {
    format!("{}: {why}", title(task))
}

fn review_why(task: &Task) -> &'static str {
    if task.spec.kind == TaskKind::Plan {
        "plan ready"
    } else {
        "ready for review"
    }
}

fn failed_why(error: Option<&str>) -> String {
    match error.and_then(|error| error.lines().next()) {
        Some(first) => format!("failed: {first}"),
        None => "failed".to_string(),
    }
}

/// Why a person might want to look now; `None` for everything routine.
pub fn attention(task: &Task, event: &TaskEvent) -> Option<String> {
    let why = match event {
        TaskEvent::Completed { .. } => review_why(task).to_string(),
        TaskEvent::Failed { error } => failed_why(Some(error)),
        TaskEvent::TimedOut { secs } => format!("timed out after {secs}s"),
        TaskEvent::RunnerLost => "runner lost".to_string(),
        TaskEvent::AutoMerged => "merged by policy".to_string(),
        TaskEvent::PermissionRequested { target, .. } => format!("asks for {target}"),
        TaskEvent::Conflicted { base, .. } => format!("conflicts with {base}"),
        TaskEvent::PrReviewed {
            state: ReviewState::ChangesRequested,
            ..
        } => "PR review requested changes".to_string(),
        _ => return None,
    };
    Some(line(task, &why))
}

/// The same question for a reader that sees statuses rather than events, such
/// as the desktop app comparing one poll with the last.
pub fn attention_for_status(task: &Task, previous: TaskStatus) -> Option<String> {
    if task.status == previous {
        return None;
    }
    let why = match task.status {
        TaskStatus::AwaitingReview => review_why(task).to_string(),
        TaskStatus::Failed => failed_why(task.error.as_deref()),
        TaskStatus::TimedOut => "timed out".to_string(),
        TaskStatus::RunnerLost => "runner lost".to_string(),
        TaskStatus::Merged => "merged".to_string(),
        TaskStatus::Conflicted => format!("conflicts with {}", task.spec.base_branch),
        _ => return None,
    };
    Some(line(task, &why))
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Open,
    InProgress,
    Done,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
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
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub assignee: Option<String>,
    /// Ids of other todos that must be `Done` first.
    #[serde(default)]
    pub blockers: Vec<String>,
    /// The workspace this belongs to; one per orchestrator until teams exist.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The user who created this; `None` for the shared token or automation.
    #[serde(default)]
    pub created_by: Option<String>,
}

impl Todo {
    /// `blocked` is derived rather than a `TodoStatus` variant: a blocker
    /// finishing later would otherwise mean rewriting every todo it blocked.
    pub fn is_blocked(&self, todos: &HashMap<String, Todo>) -> bool {
        self.blockers.iter().any(|id| {
            todos
                .get(id)
                .is_some_and(|blocker| blocker.status != TodoStatus::Done)
        })
    }
}

/// A partial update for `PATCH /todos/:id`. A field absent from the request
/// body leaves that part of the todo unchanged; `assignee`'s outer `Option`
/// carries that distinction (`None` = unchanged) while the inner one clears
/// the assignee (`Some(None)`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct TodoPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_some"
    )]
    pub assignee: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blockers: Option<Vec<String>>,
}

/// Wraps a present value in `Some` so `Option<Option<T>>` can tell "absent"
/// (the `#[serde(default)]` on the field) from "present and null".
fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Approved,
    ChangesRequested,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PrReview {
    pub state: ReviewState,
    pub url: String,
}

/// One check from the repository's `.lgtm/config.toml`, run by the runner
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
    /// Which harness reviewed; `None` on reviews recorded before this existed.
    #[serde(default)]
    pub executor: Option<Executor>,
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
    /// Move a lost or failed task to another runner this many times.
    #[serde(default)]
    pub reassign: u32,
    /// Stop scheduling new tasks in the repository once its last 24h of
    /// `cost_usd` passes this.
    #[serde(default)]
    pub budget_daily_usd: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TaskResult {
    /// Branch on the runner holding the committed change, `lgtm/<task-id>`.
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
    #[serde(alias = "worker")]
    pub runner: String,
    pub executor: Executor,
    /// The model the task asked for; the harness default when `None`.
    #[serde(default)]
    pub model: Option<String>,
    /// Unix milliseconds.
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: ExecutionStatus,
    pub error: Option<String>,
    /// The task's running total at the time this attempt ended; the runner
    /// reports one sum across attempts.
    pub cost_usd: f64,
    pub validation: Vec<ValidationResult>,
    /// Names of the files this attempt left for the reviewer.
    #[serde(default)]
    pub artefacts: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Task {
    pub id: TaskId,
    pub spec: TaskSpec,
    pub status: TaskStatus,
    /// Runner the task was assigned to, once assigned.
    #[serde(alias = "worker")]
    pub runner: Option<String>,
    /// Unix milliseconds.
    pub created_at: u64,
    pub result: Option<TaskResult>,
    pub error: Option<String>,
    #[serde(default)]
    pub pull_request: Option<PullRequest>,
    #[serde(default)]
    pub ci: Option<CiStatus>,
    /// The latest human review on the pull request, when GitHub reported one.
    #[serde(default)]
    pub pr_review: Option<PrReview>,
    #[serde(default)]
    pub executions: Vec<Execution>,
    /// The agent's own notes from `.lgtm/scratchpad.md`, kept so a retry or a
    /// person can pick up where it stopped.
    #[serde(default)]
    pub scratchpad: String,
    /// The workspace this belongs to; one per orchestrator until teams exist.
    #[serde(default)]
    pub workspace: Option<String>,
    /// The user who created this; `None` for the shared token or automation.
    #[serde(default)]
    pub created_by: Option<String>,
}

/// Files another unfinished task in the same repository has changed too.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Overlap {
    pub task: TaskId,
    pub files: Vec<String>,
}

fn changed_files(task: &Task) -> &[String] {
    match &task.result {
        Some(result) => &result.changed_files,
        None => &[],
    }
}

/// Where `task` and the other live tasks in its repository touched the same
/// files, so two agents racing on one piece of code shows up before the pull
/// request does. Derived from what the tasks already report, never stored.
// ponytail: a scan of every other task's files per task; both lists are short,
// and an index by path is the upgrade if a repository ever runs many at once.
pub fn overlaps(task: &Task, others: &[&Task]) -> Vec<Overlap> {
    let mine = changed_files(task);
    others
        .iter()
        .filter(|other| other.id != task.id && other.spec.repository == task.spec.repository)
        .filter(|other| !other.status.is_terminal())
        .filter_map(|other| {
            let mut files: Vec<String> = changed_files(other)
                .iter()
                .filter(|path| mine.contains(path))
                .cloned()
                .collect();
            files.sort();
            (!files.is_empty()).then(|| Overlap {
                task: other.id.clone(),
                files,
            })
        })
        .collect()
}

fn as_request(event: &TaskEvent) -> Option<(String, String)> {
    match event {
        TaskEvent::PermissionRequested {
            kind,
            target,
            reason,
        } if kind == "network" => Some((target.clone(), reason.clone())),
        TaskEvent::NetworkDenied { host } => {
            Some((host.clone(), "refused by the allowlist".into()))
        }
        _ => None,
    }
}

/// A host an agent asked for, or was refused, that a person has not yet
/// granted for this task's next run: distinct `(target, reason)` pairs, in
/// the order first seen.
pub fn pending_requests(events: &[StoredEvent], spec: &TaskSpec) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in events.iter().filter_map(|stored| as_request(&stored.event)) {
        if !spec.allowed_hosts.contains(&pair.0) && !out.contains(&pair) {
            out.push(pair);
        }
    }
    out
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

/// Why one commit exists, as LGTM's own records answer it. Body of
/// `GET /api/provenance/:sha`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Provenance {
    pub task: Task,
    pub goal: Option<Goal>,
    pub plan: Option<PlanVersion>,
    pub review: Option<Review>,
    pub decisions: Vec<StoredEvent>,
    pub approval: Option<StoredEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ExecutorStats {
    pub executor: Executor,
    pub attempts: u32,
    pub completed: u32,
    pub failed: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct RunnerStats {
    pub runner: String,
    pub attempts: u32,
    pub failed: u32,
    /// Median of this runner's finished executions' `finished_at - started_at`, ms.
    pub median_ms: u64,
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
    pub by_runner: Vec<RunnerStats>,
    /// The highest `[policy] budget_daily_usd` any repository in view
    /// declared; `None` when none did.
    pub budget_daily_usd: Option<f64>,
    /// Cost over the last 24h across every repository in view, regardless
    /// of `since`: "today" is always a real day, not the report's window.
    pub spent_today: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RunnerInfo {
    pub name: String,
    /// `std::env::consts::OS`.
    pub os: String,
    /// `std::env::consts::ARCH`.
    pub arch: String,
    /// Executors whose binary was found on PATH at startup.
    pub executors: Vec<Executor>,
    /// Maximum tasks the runner runs at once.
    #[serde(default = "one")]
    pub slots: u32,
    /// Exits after its tasks; the orchestrator forgets it at once on `Goodbye`.
    #[serde(default)]
    pub ephemeral: bool,
    /// Lowercase tags: `os:<os>`, `arch:<arch>`, and each toolchain binary found on PATH.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// `std::thread::available_parallelism`; 0 when unknown.
    #[serde(default)]
    pub cpu_cores: u32,
    /// Total physical memory, in megabytes; 0 when unknown.
    #[serde(default)]
    pub memory_mb: u64,
}

impl RunnerInfo {
    /// True when every requirement is met: `memory_mb:<n>` and `cpu_cores:<n>`
    /// compare numerically against this runner's resources, a malformed
    /// number never matches, and anything else must be in `capabilities`.
    pub fn has_all(&self, requirements: &[String]) -> bool {
        requirements.iter().all(|r| self.meets(r))
    }

    fn meets(&self, requirement: &str) -> bool {
        if let Some(n) = requirement.strip_prefix("memory_mb:") {
            return n.parse().is_ok_and(|n: u64| self.memory_mb >= n);
        }
        if let Some(n) = requirement.strip_prefix("cpu_cores:") {
            return n.parse().is_ok_and(|n: u32| self.cpu_cores >= n);
        }
        self.capabilities.iter().any(|c| c == requirement)
    }
}

fn one() -> u32 {
    1
}

/// Body of `GET /api/runners`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RunnerStatus {
    pub info: RunnerInfo,
    pub running: Vec<TaskId>,
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
