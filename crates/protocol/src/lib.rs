//! Wire types shared by the orchestrator, worker agent, and CLI.
//!
//! Every message crosses a process boundary as JSON. Enums are tagged with
//! `type` so a reader can dispatch on one field without knowing the variant.

use serde::{Deserialize, Serialize};

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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskResult {
    /// Branch on the worker holding the committed change, `lgtm/<task-id>`.
    pub branch: String,
    /// `git diff <merge-base> <branch>` output.
    pub diff: String,
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub validation: Vec<ValidationResult>,
}

impl TaskResult {
    pub fn validation_failed(&self) -> bool {
        self.validation.iter().any(|v| !v.ok)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Something that happened to one task on a worker.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEvent {
    /// Worktree is ready and the agent process has been spawned. Sent again
    /// for every follow-up run.
    Started,
    /// A follow-up from the developer, recorded before the run it triggers.
    Message {
        text: String,
    },
    /// One line from the agent process, without the trailing newline.
    Output {
        stream: OutputStream,
        line: String,
    },
    /// Agent exited 0 and the change is committed on the task branch.
    Completed {
        result: TaskResult,
    },
    /// Agent exited non-zero, or preparation failed.
    Failed {
        error: String,
    },
    Cancelled,
    /// Task branch pushed to origin after approval. `sha` is the branch head.
    Pushed {
        branch: String,
        #[serde(default)]
        sha: String,
    },
    /// Worktree and branch removed after rejection.
    Discarded,
}

/// An event with the orchestrator's receipt time, unix milliseconds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StoredEvent {
    pub at: u64,
    pub event: TaskEvent,
}

/// Worker → orchestrator, over the worker WebSocket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerMessage {
    /// First frame on every connection. `running` lists tasks the worker
    /// still has processes for, so a reconnect does not lose them.
    Hello {
        token: String,
        info: WorkerInfo,
        #[serde(default)]
        running: Vec<TaskId>,
    },
    Event {
        task_id: TaskId,
        event: TaskEvent,
    },
}

/// Orchestrator → worker, over the worker WebSocket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorMessage {
    HelloAck,
    Start {
        task: Box<Task>,
    },
    Cancel {
        task_id: TaskId,
    },
    /// Resume the task's agent session in its worktree with this follow-up.
    Message {
        task_id: TaskId,
        text: String,
    },
    /// Push `lgtm/<task-id>` to origin.
    Push {
        task_id: TaskId,
    },
    /// Remove the worktree and branch.
    Discard {
        task_id: TaskId,
    },
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
        };
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
        round_trip(WorkerStatus {
            info,
            running: vec!["0123abcd".into()],
        });
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
        let hello: WorkerMessage = serde_json::from_str(
            r#"{"type":"hello","token":"t","info":{"name":"w","os":"linux","arch":"x86_64","executors":[]}}"#,
        )
        .unwrap();
        assert!(matches!(hello, WorkerMessage::Hello { running, .. } if running.is_empty()));
        let result: TaskResult =
            serde_json::from_str(r#"{"branch":"lgtm/0123abcd","diff":"","changed_files":[]}"#)
                .unwrap();
        assert!(result.validation.is_empty());
        let task: Task = serde_json::from_str(
            r#"{"id":"0123abcd","spec":{"repository":"r","base_branch":"main","prompt":"p","executor":"claude","worker":null},"status":"approved","worker":"w","created_at":1,"result":null,"error":null}"#,
        )
        .unwrap();
        assert!(task.pull_request.is_none() && task.ci.is_none() && task.spec.issue.is_none());
        assert!(task.spec.linear.is_none());
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
