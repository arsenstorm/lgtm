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
    Rejected,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Approved
                | TaskStatus::Rejected
                | TaskStatus::Failed
                | TaskStatus::Cancelled
        )
    }
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TaskResult {
    /// Branch on the worker holding the committed change, `lgtm/<task-id>`.
    pub branch: String,
    /// `git diff <merge-base> <branch>` output.
    pub diff: String,
    pub changed_files: Vec<String>,
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
    /// Worktree is ready and the agent process has been spawned.
    Started,
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
    /// Task branch pushed to origin after approval.
    Pushed {
        branch: String,
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
    /// First frame on every connection.
    Hello {
        token: String,
        info: WorkerInfo,
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
            },
            status: TaskStatus::Queued,
            worker: None,
            created_at: 1,
            result: None,
            error: None,
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
        };
        let result = TaskResult {
            branch: "lgtm/0123abcd".into(),
            diff: "--- a\n+++ b\n".into(),
            changed_files: vec!["HEALTH.md".into()],
        };
        for event in [
            TaskEvent::Started,
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
    fn tags_are_snake_case_type_fields() {
        let json = serde_json::to_string(&OrchestratorMessage::HelloAck).unwrap();
        assert_eq!(json, r#"{"type":"hello_ack"}"#);
        let json = serde_json::to_string(&TaskStatus::AwaitingReview).unwrap();
        assert_eq!(json, r#""awaiting_review""#);
    }
}
