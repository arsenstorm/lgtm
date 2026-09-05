//! The messages themselves: task events and the two directions of the runner
//! socket.

use serde::{Deserialize, Serialize};

use crate::{
    Authorship, Executor, Memory, ReviewState, RunnerInfo, Skill, SkillRef, Task, TaskId,
    TaskResult,
};

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
        /// What the runner put in the worktree for this run.
        #[serde(default)]
        skills: Vec<SkillRef>,
    },
    /// A follow-up from the developer, recorded before the run it triggers.
    Message {
        text: String,
        /// The user who sent it; `None` for the shared token. This is how a
        /// second person joins a task, and so how their agent earns a
        /// co-author trailer.
        #[serde(default)]
        by: Option<String>,
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
    /// A file the run left in `.lgtm/artefacts/` for whoever reviews it.
    /// `bytes_base64` only travels from the runner: the orchestrator writes
    /// the payload to a file of its own, so what is stored and served back is
    /// the name and the size.
    Artefact {
        name: String,
        size: usize,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        bytes_base64: String,
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
    /// The answer to one [`OrchestratorMessage::Infer`]. Not a `TaskEvent`:
    /// a utility call is not task history, so it is never stored.
    Inferred {
        id: String,
        /// Model text on success, empty on failure.
        #[serde(default)]
        text: String,
        /// Why it failed; `None` on success.
        #[serde(default)]
        error: Option<String>,
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
        /// Written into the worktree before the run; resolved by the
        /// orchestrator for the same reason as `memories`.
        #[serde(default)]
        skills: Vec<Skill>,
        /// Whose names go on the commit, resolved by the orchestrator from
        /// the workspace's credentials.
        #[serde(default)]
        authorship: Authorship,
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
        /// Written into the worktree before the run; resolved by the
        /// orchestrator for the same reason as `memories`.
        #[serde(default)]
        skills: Vec<Skill>,
        /// The task as the orchestrator has it now, so a follow-up carries a
        /// spec change (e.g. an allowed host) the runner's own stored copy
        /// predates.
        #[serde(default)]
        task: Option<Box<Task>>,
        /// Resolved again, so a credential registered since the task started
        /// is the one that signs the follow-up.
        #[serde(default)]
        authorship: Authorship,
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
    /// One-shot model call for the orchestrator's own features (prompt
    /// enhancement, planning). Not a task: no worktree, no slot, and never
    /// stored as task history — the reasoning that keeps Terminal frames out
    /// of events. A runner that predates it answers nothing and the
    /// orchestrator's timeout covers that.
    Infer {
        id: String,
        executor: Executor,
        /// Instructions that frame the call; the runner passes it as the
        /// system prompt, or prepends it when the executor has no such flag.
        system: String,
        prompt: String,
    },
}

/// One file a run left for the reviewer, as the API lists it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Artefact {
    pub name: String,
    pub size: usize,
}

/// The sanitised form of an artefact file name, or `None` when nothing usable
/// is left. The runner sanitises with it and the orchestrator validates with
/// it, so a name that reaches a file path is never the runner's word alone.
pub fn artefact_name(raw: &str) -> Option<String> {
    let name: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let usable = name.len() <= 128 && name.trim_matches(['.', '-', '_']).chars().next().is_some();
    usable.then_some(name)
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// No crate in the workspace speaks base64; the alphabet is small enough to
/// spell out here.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        let chars = [
            BASE64_ALPHABET[(n >> 18 & 0x3f) as usize],
            BASE64_ALPHABET[(n >> 12 & 0x3f) as usize],
            BASE64_ALPHABET[(n >> 6 & 0x3f) as usize],
            BASE64_ALPHABET[(n & 0x3f) as usize],
        ];
        out.push(chars[0] as char);
        out.push(chars[1] as char);
        out.push(if chunk.len() > 1 {
            chars[2] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            chars[3] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes what [`TaskEvent::Artefact`] carries. Padding and length are
/// checked because the bytes are written to a file, not shown to a person.
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|c| **c == b'=').count();
        let mut n = 0u32;
        for c in chunk {
            n = (n << 6) | u32::from(sextet(*c)?);
        }
        let full = n.to_be_bytes();
        out.extend_from_slice(&full[1..4 - pad]);
    }
    Some(out)
}

fn sextet(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        b'=' => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artefact_names_are_reduced_to_a_file_name() {
        assert_eq!(artefact_name("shot.png").as_deref(), Some("shot.png"));
        assert_eq!(
            artefact_name("../etc/passwd").as_deref(),
            Some("..-etc-passwd")
        );
        assert_eq!(artefact_name("a b.png").as_deref(), Some("a-b.png"));
        assert_eq!(artefact_name(".."), None);
        assert_eq!(artefact_name(""), None);
        assert_eq!(artefact_name(&"x".repeat(129)), None);
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(encode_base64(b"Man"), "TWFu");
        assert_eq!(encode_base64(b"Ma"), "TWE=");
        assert_eq!(encode_base64(b"M"), "TQ==");
    }

    #[test]
    fn base64_round_trips_and_rejects_junk() {
        assert_eq!(decode_base64("TWFu").unwrap(), b"Man");
        assert_eq!(decode_base64("TWE=").unwrap(), b"Ma");
        assert_eq!(decode_base64("TQ==").unwrap(), b"M");
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("TWF"), None);
        assert_eq!(decode_base64("TW!="), None);
        for bytes in [b"Man".as_slice(), b"Ma", b"M", b""] {
            assert_eq!(decode_base64(&encode_base64(bytes)).unwrap(), bytes);
        }
    }
}
