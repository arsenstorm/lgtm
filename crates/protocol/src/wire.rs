//! The messages themselves: task events and the two directions of the runner
//! socket.

use serde::{Deserialize, Serialize};

use crate::{Executor, Memory, ReviewState, RunnerInfo, Task, TaskId, TaskResult};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

/// Something that happened to one task on a runner.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskEvent {
    /// Worktree is ready and the agent process has been spawned. Sent again
    /// for every follow-up run.
    Started {
        /// The task spec's requested model, carried through so it lands on
        /// the execution without the runner needing the store.
        #[serde(default)]
        model: Option<String>,
    },
    /// A follow-up from the developer, recorded before the run it triggers.
    Message {
        text: String,
    },
    /// One line from the agent process, without the trailing newline.
    Output {
        stream: OutputStream,
        line: String,
    },
    /// The agent ran a shell command.
    Command {
        command: String,
    },
    /// The agent wrote or edited a file, path relative to the worktree when
    /// it can be.
    FileChanged {
        path: String,
    },
    /// Text the agent addressed to the reader, as it works.
    Progress {
        text: String,
    },
    /// The scratchpad after a run, when it changed.
    Scratchpad {
        content: String,
    },
    /// The repository's checks are about to run; results arrive in
    /// `Completed`.
    Validating {
        names: Vec<String>,
    },
    /// The run tried to reach a host outside its allowlist; the connection was refused.
    NetworkDenied {
        host: String,
    },
    /// The agent asked for something its sandbox refused; a person answers with `lgtm allow`.
    PermissionRequested {
        kind: String,
        target: String,
        reason: String,
    },
    /// A person added `host` to this task's allowlist for its next run.
    HostAllowed {
        host: String,
    },
    /// Agent exited 0 and the change is committed on the task branch.
    Completed {
        result: TaskResult,
    },
    /// The runner is running the agent again: a crash with retries left, or
    /// failing checks it was told to fix.
    Retry {
        attempt: u32,
        reason: String,
    },
    /// A retry: the task goes back to the queue as a new attempt, possibly
    /// elsewhere.
    Requeued {
        #[serde(alias = "worker")]
        runner: Option<String>,
        executor: Executor,
    },
    /// What policy decided on its own and why, so an automatic approval is
    /// never a mystery afterwards.
    PolicyDecision {
        action: String,
        allowed: bool,
        reasons: Vec<String>,
    },
    /// What the orchestration model asked for and whether LGTM did it.
    Orchestrated {
        action: String,
        reason: String,
        applied: bool,
        note: String,
    },
    /// Approved by policy rather than by hand; a `Pushed` event follows.
    AutoApproved,
    /// Merged by policy once CI passed.
    AutoMerged,
    /// Agent exited non-zero, or preparation failed.
    Failed {
        error: String,
    },
    /// The agent ran past the repository's timeout and was killed.
    TimedOut {
        secs: u64,
    },
    /// The runner's socket expired with this task still running.
    RunnerLost,
    Cancelled,
    /// Rebasing onto `base` before the push stopped on these files.
    Conflicted {
        base: String,
        files: Vec<String>,
    },
    /// Task branch pushed to origin after approval. `sha` is the branch head.
    Pushed {
        branch: String,
        #[serde(default)]
        sha: String,
    },
    /// Worktree and branch removed after rejection.
    Discarded,
    /// GitHub reported a review on the pull request.
    PrReviewed {
        state: ReviewState,
        url: String,
    },
}

/// An event with the orchestrator's receipt time, unix milliseconds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StoredEvent {
    pub at: u64,
    pub event: TaskEvent,
}

/// Runner → orchestrator, over the runner WebSocket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunnerMessage {
    /// First frame on every connection. `running` lists tasks the runner
    /// still has processes for, so a reconnect does not lose them.
    Hello {
        token: String,
        info: RunnerInfo,
        #[serde(default)]
        running: Vec<TaskId>,
        /// `PROTOCOL_VERSION` of the runner; 0 from runners that predate it.
        #[serde(default)]
        version: u32,
    },
    Event {
        task_id: TaskId,
        event: TaskEvent,
    },
    /// Output from the task's attached shell. Not a `TaskEvent`: terminal
    /// traffic is a live pipe, not task history, so it is never stored.
    /// `data` is UTF-8 text with invalid bytes replaced by U+FFFD.
    Terminal {
        task_id: TaskId,
        data: String,
    },
    /// The task's shell exited or was killed. Not a `TaskEvent`, for the same
    /// reason as `Terminal`.
    TerminalClosed {
        task_id: TaskId,
    },
    /// The runner is exiting on purpose and runs nothing.
    Goodbye,
}

/// Orchestrator → runner, over the runner WebSocket.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorMessage {
    HelloAck,
    /// Sent instead of `HelloAck` and followed by a close; the runner must
    /// not retry.
    Rejected {
        reason: String,
    },
    Start {
        task: Box<Task>,
        /// What the runner prepends to the prompt; resolved by the
        /// orchestrator so a runner never needs the store.
        #[serde(default)]
        memories: Vec<Memory>,
    },
    Cancel {
        task_id: TaskId,
    },
    /// Ask the agent to stop gracefully: SIGINT, then a kill after a grace period.
    Interrupt {
        task_id: TaskId,
    },
    /// Resume the task's agent session in its worktree with this follow-up.
    Message {
        task_id: TaskId,
        text: String,
        /// What the runner prepends to the prompt; resolved by the
        /// orchestrator so a runner never needs the store.
        #[serde(default)]
        memories: Vec<Memory>,
        /// The task as the orchestrator has it now, so a follow-up carries a
        /// spec change (e.g. an allowed host) the runner's own stored copy
        /// predates.
        #[serde(default)]
        task: Option<Box<Task>>,
    },
    /// Push `lgtm/<task-id>` to origin.
    Push {
        task_id: TaskId,
        /// Bearer token for this push; `None` leaves it to the runner's own
        /// git credentials.
        #[serde(default)]
        token: Option<String>,
    },
    /// Remove the worktree and branch.
    Discard {
        task_id: TaskId,
    },
    /// Start a shell in the task's worktree, if one is not running already.
    /// Not a `TaskEvent`: terminal traffic is not task history.
    TerminalOpen {
        task_id: TaskId,
    },
    /// Keystrokes for that shell's stdin.
    TerminalInput {
        task_id: TaskId,
        data: String,
    },
    /// Kill the task's shell. Only a person detaching does not send this; the
    /// shell is meant to survive a client going away.
    TerminalClose {
        task_id: TaskId,
    },
}
