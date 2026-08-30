//! Turns a `TaskEvent` into the lines the Activity tab shows.
//!
//! Same rules as the CLI's `render`, returning lines instead of writing them,
//! so each line can carry a kind the UI colours by.

use lgtm_protocol::{OutputStream, TaskEvent};
use serde_json::Value;

const TOOL_DETAIL_MAX: usize = 100;

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
        TaskEvent::AutoApproved => vec![Line::new(Kind::Status, "approved by policy")],
        TaskEvent::AutoMerged => vec![Line::new(Kind::Status, "merged by policy")],
        TaskEvent::Failed { error } => vec![Line::new(Kind::Status, format!("failed: {error}"))],
        TaskEvent::TimedOut { secs } => {
            vec![Line::new(Kind::Status, format!("timed out after {secs}s"))]
        }
        TaskEvent::RunnerLost => vec![Line::new(Kind::Status, "runner lost")],
        TaskEvent::Cancelled => vec![Line::new(Kind::Status, "cancelled")],
        TaskEvent::Pushed { branch, .. } => {
            vec![Line::new(Kind::Status, format!("pushed {branch}"))]
        }
        TaskEvent::Discarded => vec![Line::new(Kind::Status, "discarded")],
    }
}

fn render_stdout(line: &str) -> Vec<Line> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return vec![Line::new(Kind::Text, line)];
    };
    match value.get("type").and_then(Value::as_str) {
        Some("assistant") => render_assistant(&value),
        Some("result") => render_result(&value),
        _ => Vec::new(),
    }
}

fn render_assistant(value: &Value) -> Vec<Line> {
    let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };
    blocks.iter().filter_map(assistant_line).collect()
}

fn assistant_line(block: &Value) -> Option<Line> {
    let text = |key: &str| block.get(key).and_then(Value::as_str).unwrap_or("");
    match text("type") {
        "text" if !text("text").is_empty() => Some(Line::new(Kind::Text, text("text"))),
        "tool_use" => {
            let name = text("name");
            let detail = block.get("input").map(tool_detail).unwrap_or_default();
            let line = if detail.is_empty() {
                format!("▸ {name}")
            } else {
                format!("▸ {name} {detail}")
            };
            Some(Line::new(Kind::Tool, line))
        }
        _ => None,
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

/// First present of command/file_path/pattern/description, truncated.
fn tool_detail(input: &Value) -> String {
    let raw = input
        .get("command")
        .or_else(|| input.get("file_path"))
        .or_else(|| input.get("pattern"))
        .or_else(|| input.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if raw.chars().count() > TOOL_DETAIL_MAX {
        raw.chars().take(TOOL_DETAIL_MAX).collect()
    } else {
        raw.to_string()
    }
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

    #[test]
    fn assistant_text_is_a_text_line() {
        let line = r#"{"type":"assistant","message":{"model":"claude-x","id":"m","type":"message","role":"assistant","content":[{"type":"text","text":"Hi."}],"stop_reason":null},"parent_tool_use_id":null,"session_id":"a"}"#;
        assert_eq!(stdout(line), vec![Line::new(Kind::Text, "Hi.")]);
    }

    #[test]
    fn tool_use_shows_command() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls -la","description":"List files"}}]},"session_id":"a"}"#;
        assert_eq!(stdout(line), vec![Line::new(Kind::Tool, "▸ Bash ls -la")]);
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
