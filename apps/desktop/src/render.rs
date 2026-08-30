//! Turns a `TaskEvent` into the lines the Activity tab shows.
//!
//! Same rules as the CLI's `render`, returning lines instead of writing them,
//! so each line can carry a kind the UI colours by. What the agent did comes
//! in as its own events, so raw stdout is only shown when it is not
//! stream-json, plus the failed `result` line.

use lgtm_agent::codex_error;
use lgtm_protocol::{OutputStream, TaskEvent};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Tool,
    Stderr,
    Message,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

impl Line {
    fn new(kind: Kind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

pub fn render(event: &TaskEvent) -> Vec<Line> {
    match event {
        TaskEvent::Started => vec![Line::new(Kind::Status, "agent started")],
        TaskEvent::Message { text } => vec![Line::new(Kind::Message, format!("> {text}"))],
        TaskEvent::Output {
            stream: OutputStream::Stderr,
            line,
        } => vec![Line::new(Kind::Stderr, format!("! {line}"))],
        TaskEvent::Output {
            stream: OutputStream::Stdout,
            line,
        } => render_stdout(line),
        TaskEvent::Command { command } => vec![Line::new(Kind::Tool, format!("$ {command}"))],
        TaskEvent::FileChanged { path } => vec![Line::new(Kind::Tool, format!("~ {path}"))],
        TaskEvent::Progress { text } => vec![Line::new(Kind::Text, text.clone())],
        TaskEvent::Scratchpad { .. } => vec![Line::new(Kind::Status, "notes updated")],
        TaskEvent::Validating { names } => vec![Line::new(
            Kind::Status,
            format!("running checks: {}", names.join(", ")),
        )],
        TaskEvent::Completed { result } => {
            let changed = result.changed_files.len();
            let total = result.validation.len();
            if total == 0 {
                return vec![Line::new(
                    Kind::Status,
                    format!("completed: {changed} files changed"),
                )];
            }
            let passed = result.validation.iter().filter(|v| v.ok).count();
            vec![Line::new(
                Kind::Status,
                format!("completed: {changed} files changed, {passed}/{total} checks passed"),
            )]
        }
        TaskEvent::Retry { attempt, reason } => vec![Line::new(
            Kind::Status,
            format!("retry {attempt}: {reason}"),
        )],
        TaskEvent::Requeued { worker, executor } => vec![Line::new(
            Kind::Status,
            format!(
                "requeued on {} ({})",
                worker.as_deref().unwrap_or("any worker"),
                executor.binary()
            ),
        )],
        TaskEvent::PolicyDecision {
            action,
            allowed,
            reasons,
        } => vec![Line::new(
            Kind::Status,
            policy_decision(action, *allowed, reasons),
        )],
        TaskEvent::Orchestrated {
            action,
            reason,
            applied,
            note,
        } => vec![Line::new(
            Kind::Status,
            orchestrated(action, reason, *applied, note),
        )],
        TaskEvent::AutoApproved => vec![Line::new(Kind::Status, "approved by policy")],
        TaskEvent::AutoMerged => vec![Line::new(Kind::Status, "merged by policy")],
        TaskEvent::Failed { error } => vec![Line::new(Kind::Status, format!("failed: {error}"))],
        TaskEvent::TimedOut { secs } => {
            vec![Line::new(Kind::Status, format!("timed out after {secs}s"))]
        }
        TaskEvent::RunnerLost => vec![Line::new(Kind::Status, "runner lost")],
        TaskEvent::Cancelled => vec![Line::new(Kind::Status, "cancelled")],
        TaskEvent::Conflicted { base, files } => vec![Line::new(
            Kind::Status,
            format!("conflicted with {base}: {}", files.join(", ")),
        )],
        TaskEvent::Pushed { branch, .. } => {
            vec![Line::new(Kind::Status, format!("pushed {branch}"))]
        }
        TaskEvent::Discarded => vec![Line::new(Kind::Status, "discarded")],
    }
}

fn orchestrated(action: &str, reason: &str, applied: bool, note: &str) -> String {
    if applied {
        format!("orchestrator: {action} — {reason}")
    } else {
        format!("orchestrator wanted {action} ({reason}); not applied: {note}")
    }
}

fn policy_decision(action: &str, allowed: bool, reasons: &[String]) -> String {
    if allowed {
        format!("policy: auto-{action} ({})", reasons.join(", "))
    } else {
        format!("policy: no auto-{action}: {}", reasons.join("; "))
    }
}

fn render_stdout(line: &str) -> Vec<Line> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return vec![Line::new(Kind::Text, line)];
    };
    match value.get("type").and_then(Value::as_str) {
        Some("result") => render_result(&value),
        Some("item.completed") | Some("turn.failed") => codex_error(&value)
            .map(|message| Line::new(Kind::Stderr, format!("! {message}")))
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
}

fn render_result(value: &Value) -> Vec<Line> {
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !is_error {
        return Vec::new();
    }
    let result = value.get("result").and_then(Value::as_str).unwrap_or("");
    vec![Line::new(Kind::Stderr, format!("! {result}"))]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdout(line: &str) -> Vec<Line> {
        render(&TaskEvent::Output {
            stream: OutputStream::Stdout,
            line: line.to_string(),
        })
    }

    #[test]
    fn system_init_is_silent() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"a","tools":["Task","Bash"]}"#;
        assert!(stdout(line).is_empty());
    }

    /// The runner sends the same content as `Progress` and `Command`, so
    /// rendering the raw line would show all of it twice.
    #[test]
    fn assistant_stdout_is_silent() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hi."},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls -la"}}]},"session_id":"a"}"#;
        assert!(stdout(line).is_empty());
    }

    #[test]
    fn structured_events_each_have_their_own_line() {
        assert_eq!(
            render(&TaskEvent::Progress {
                text: "Looking.".into()
            }),
            vec![Line::new(Kind::Text, "Looking.")]
        );
        assert_eq!(
            render(&TaskEvent::Command {
                command: "ls -la".into()
            }),
            vec![Line::new(Kind::Tool, "$ ls -la")]
        );
        assert_eq!(
            render(&TaskEvent::FileChanged {
                path: "src/a.rs".into()
            }),
            vec![Line::new(Kind::Tool, "~ src/a.rs")]
        );
        assert_eq!(
            render(&TaskEvent::Validating {
                names: vec!["test".into(), "lint".into()]
            }),
            vec![Line::new(Kind::Status, "running checks: test, lint")]
        );
    }

    #[test]
    fn policy_decisions_read_as_a_sentence() {
        assert_eq!(
            render(&TaskEvent::PolicyDecision {
                action: "approve".into(),
                allowed: true,
                reasons: vec!["checks passed".into(), "12 lines".into()],
            }),
            vec![Line::new(
                Kind::Status,
                "policy: auto-approve (checks passed, 12 lines)"
            )]
        );
        assert_eq!(
            render(&TaskEvent::PolicyDecision {
                action: "merge".into(),
                allowed: false,
                reasons: vec!["ci failure".into()],
            }),
            vec![Line::new(Kind::Status, "policy: no auto-merge: ci failure")]
        );
    }

    #[test]
    fn rate_limit_event_is_silent() {
        let line = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#;
        assert!(stdout(line).is_empty());
    }

    #[test]
    fn successful_result_is_silent() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"result":"Hi.","session_id":"a"}"#;
        assert!(stdout(line).is_empty());
    }

    #[test]
    fn non_json_line_is_echoed() {
        assert_eq!(
            stdout("plain text output"),
            vec![Line::new(Kind::Text, "plain text output")]
        );
    }

    #[test]
    fn message_event_is_quoted() {
        let event = TaskEvent::Message {
            text: "use the existing helper".into(),
        };
        assert_eq!(
            render(&event),
            vec![Line::new(Kind::Message, "> use the existing helper")]
        );
    }

    #[test]
    fn stderr_is_prefixed() {
        let event = TaskEvent::Output {
            stream: OutputStream::Stderr,
            line: "boom".into(),
        };
        assert_eq!(render(&event), vec![Line::new(Kind::Stderr, "! boom")]);
    }

    #[test]
    fn retry_shows_attempt_and_reason() {
        let event = TaskEvent::Retry {
            attempt: 2,
            reason: "checks failed".into(),
        };
        assert_eq!(
            render(&event),
            vec![Line::new(Kind::Status, "retry 2: checks failed")]
        );
    }
}
