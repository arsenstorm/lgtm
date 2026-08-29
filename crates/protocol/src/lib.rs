//! Wire types shared by the orchestrator, worker agent, and CLI.
//!
//! Every message crosses a process boundary as JSON. Enums are tagged with
//! `type` so a reader can dispatch on one field without knowing the variant.

mod wire;

use serde::{Deserialize, Serialize};

pub use wire::*;

pub const DEFAULT_PORT: u16 = 4750;
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
mod tests {
    use super::*;

    fn sample_task() -> Task {
        Task {
            id: "0123abcd".into(),
            spec: TaskSpec {
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                base_branch: "main".into(),
                prompt: "add a /health endpoint".into(),
                executor: Executor::Claude,
                worker: Some("compute".into()),
                issue: Some(IssueRef {
                    owner: "arsenstorm".into(),
                    repo: "lgtm".into(),
                    number: 7,
                }),
                linear: Some(LinearRef {
                    id: "uuid".into(),
                    identifier: "ENG-123".into(),
                    url: "https://linear.app/w/issue/ENG-123".into(),
                }),
                kind: TaskKind::Plan,
                parent: Some("00000000".into()),
                depends_on: vec!["11111111".into()],
                batch: Some("b1".into()),
            },
            status: TaskStatus::Queued,
            worker: None,
            created_at: 1,
            result: None,
            error: None,
            pull_request: Some(PullRequest {
                number: 12,
                url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
            }),
            ci: Some(CiStatus {
                state: CiState::Pending,
                url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
            }),
        }
    }

    fn round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(v: T) {
        let json = serde_json::to_string(&v).unwrap();
        let back: T = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back, "{json}");
    }

    #[test]
    fn every_message_round_trips() {
        let info = WorkerInfo {
            name: "compute".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            executors: vec![Executor::Claude],
            slots: 2,
            ephemeral: true,
        };
        let result = TaskResult {
            branch: "lgtm/0123abcd".into(),
            diff: "--- a\n+++ b\n".into(),
            changed_files: vec!["HEALTH.md".into()],
            validation: vec![ValidationResult {
                name: "test".into(),
                command: "bun test".into(),
                ok: false,
                output_tail: "1 failed".into(),
            }],
            plan: Some(Plan {
                steps: vec![PlanStep {
                    key: "schema".into(),
                    title: "Add schema".into(),
                    prompt: "Add the table".into(),
                    depends_on: vec![],
                }],
            }),
            review: Some(Review {
                findings: vec![Finding {
                    severity: Severity::Blocking,
                    file: "src/a.rs".into(),
                    line: Some(3),
                    message: "unwrap on user input".into(),
                }],
            }),
            policy: Some(Policy {
                auto_approve: true,
                auto_merge: false,
            }),
            cost_usd: 0.42,
        };
        assert!(result.review.as_ref().unwrap().has_blocking());
        assert!(result.validation_failed());
        for event in [
            TaskEvent::Started,
            TaskEvent::Message {
                text: "use the existing helper".into(),
            },
            TaskEvent::Output {
                stream: OutputStream::Stdout,
                line: "{}".into(),
            },
            TaskEvent::Completed {
                result: result.clone(),
            },
            TaskEvent::Failed {
                error: "boom".into(),
            },
            TaskEvent::Cancelled,
            TaskEvent::Retry {
                attempt: 1,
                reason: "checks failed".into(),
            },
            TaskEvent::AutoApproved,
            TaskEvent::AutoMerged,
            TaskEvent::Pushed {
                branch: "lgtm/0123abcd".into(),
                sha: "abc123".into(),
            },
            TaskEvent::Discarded,
        ] {
            round_trip(StoredEvent {
                at: 2,
                event: event.clone(),
            });
            round_trip(WorkerMessage::Event {
                task_id: "0123abcd".into(),
                event,
            });
        }
        round_trip(WorkerMessage::Hello {
            token: "t".into(),
            info: info.clone(),
            running: vec!["0123abcd".into()],
        });
        round_trip(WorkerMessage::Goodbye);
        round_trip(WorkerStatus {
            info,
            running: vec!["0123abcd".into()],
        });
        round_trip(Batch {
            id: "b1".into(),
            created_at: 3,
            source: BatchSource::GithubLabel {
                owner: "o".into(),
                repo: "r".into(),
                label: "P1".into(),
            },
            repository: "https://github.com/o/r.git".into(),
            task_ids: vec!["0123abcd".into()],
            approve_plans: true,
        });
        round_trip(BatchSource::Linear {
            team: "ENG".into(),
            state: "Todo".into(),
        });
        round_trip(BatchSummary::default());
        for msg in [
            OrchestratorMessage::HelloAck,
            OrchestratorMessage::Start {
                task: Box::new(sample_task()),
            },
            OrchestratorMessage::Cancel {
                task_id: "0123abcd".into(),
            },
            OrchestratorMessage::Message {
                task_id: "0123abcd".into(),
                text: "again".into(),
            },
            OrchestratorMessage::Push {
                task_id: "0123abcd".into(),
            },
            OrchestratorMessage::Discard {
                task_id: "0123abcd".into(),
            },
        ] {
            round_trip(msg);
        }
    }

    #[test]
    fn phase_one_frames_still_parse() {
        let info: WorkerInfo = serde_json::from_str(
            r#"{"name":"w","os":"linux","arch":"x86_64","executors":["claude"]}"#,
        )
        .unwrap();
        assert_eq!(info.slots, 1);
        assert!(!info.ephemeral);
        let hello: WorkerMessage = serde_json::from_str(
            r#"{"type":"hello","token":"t","info":{"name":"w","os":"linux","arch":"x86_64","executors":[]}}"#,
        )
        .unwrap();
        assert!(matches!(hello, WorkerMessage::Hello { running, .. } if running.is_empty()));
        let result: TaskResult =
            serde_json::from_str(r#"{"branch":"lgtm/0123abcd","diff":"","changed_files":[]}"#)
                .unwrap();
        assert!(result.validation.is_empty());
        assert!(result.review.is_none() && result.policy.is_none() && result.cost_usd == 0.0);
        let task: Task = serde_json::from_str(
            r#"{"id":"0123abcd","spec":{"repository":"r","base_branch":"main","prompt":"p","executor":"claude","worker":null},"status":"approved","worker":"w","created_at":1,"result":null,"error":null}"#,
        )
        .unwrap();
        assert!(task.pull_request.is_none() && task.ci.is_none() && task.spec.issue.is_none());
        assert!(task.spec.linear.is_none());
        assert_eq!(task.spec.kind, TaskKind::Run);
        assert!(task.spec.parent.is_none() && task.spec.depends_on.is_empty());
        assert!(task.spec.batch.is_none());
        let pushed: TaskEvent = serde_json::from_str(r#"{"type":"pushed","branch":"b"}"#).unwrap();
        assert!(matches!(pushed, TaskEvent::Pushed { sha, .. } if sha.is_empty()));
    }

    #[test]
    fn tags_are_snake_case_type_fields() {
        let json = serde_json::to_string(&OrchestratorMessage::HelloAck).unwrap();
        assert_eq!(json, r#"{"type":"hello_ack"}"#);
        let json = serde_json::to_string(&TaskStatus::AwaitingReview).unwrap();
        assert_eq!(json, r#""awaiting_review""#);
    }
}
