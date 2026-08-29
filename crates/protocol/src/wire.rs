//! The messages themselves: task events and the two directions of the worker
//! socket.

use serde::{Deserialize, Serialize};

use crate::{Task, TaskId, TaskResult, WorkerInfo};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Something that happened to one task on a worker.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
    /// The worker is running the agent again: a crash with retries left, or
    /// failing checks it was told to fix.
    Retry {
        attempt: u32,
        reason: String,
    },
    /// Approved by policy rather than by hand; a `Pushed` event follows.
    AutoApproved,
    /// Merged by policy once CI passed.
    AutoMerged,
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoredEvent {
    pub at: u64,
    pub event: TaskEvent,
}

/// Worker → orchestrator, over the worker WebSocket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
    /// The worker is exiting on purpose and runs nothing.
    Goodbye,
}

/// Orchestrator → worker, over the worker WebSocket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
