//! Turns a `TaskEvent` into the lines a human watches scroll by.
//!
//! Agent stdout is `claude -p --output-format stream-json`: one JSON object
//! per line. We only surface the parts a human cares about (assistant text,
//! tool calls, failed results) and stay silent on the rest (system/user
//! bookkeeping, rate-limit pings, successful results already implied by
//! `Completed`).

use lgtm_protocol::{OutputStream, TaskEvent, ValidationResult};
use serde_json::Value;
use std::io::Write;

const TOOL_DETAIL_MAX: usize = 100;

pub fn render(event: &TaskEvent, out: &mut impl Write) -> std::io::Result<()> {
    match event {
        TaskEvent::Started => writeln!(out, "agent started"),
        TaskEvent::Message { text } => writeln!(out, "> {text}"),
        TaskEvent::Output {
            stream: OutputStream::Stderr,
            line,
        } => writeln!(out, "! {line}"),
        TaskEvent::Output {
            stream: OutputStream::Stdout,
            line,
        } => render_stdout(line, out),
        TaskEvent::Completed { result } => {
            let total = result.validation.len();
            if total == 0 {
                return writeln!(
                    out,
                    "completed: {} files changed",
                    result.changed_files.len()
                );
            }
            let passed = result.validation.iter().filter(|v| v.ok).count();
            writeln!(
                out,
                "completed: {} files changed, {passed}/{total} checks passed",
                result.changed_files.len()
            )
        }
        TaskEvent::Failed { error } => writeln!(out, "failed: {error}"),
        TaskEvent::Cancelled => writeln!(out, "cancelled"),
        TaskEvent::Pushed { branch } => writeln!(out, "pushed {branch}"),
        TaskEvent::Discarded => writeln!(out, "discarded"),
    }
}

/// Renders each check's pass/fail line after a `Completed` event, with the
/// output tail of a failing check indented underneath it. No-op when there
/// are no checks (repo has no `.lgtm/config.toml` validation configured).
pub fn print_validation(results: &[ValidationResult], out: &mut impl Write) -> std::io::Result<()> {
    if results.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    for result in results {
        if result.ok {
            writeln!(out, "✓ {}", result.name)?;
        } else {
            writeln!(out, "✗ {}", result.name)?;
            for line in result.output_tail.lines() {
                writeln!(out, "    {line}")?;
            }
        }
    }
    Ok(())
}

fn render_stdout(line: &str, out: &mut impl Write) -> std::io::Result<()> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return writeln!(out, "{line}");
    };
    match value.get("type").and_then(Value::as_str) {
        Some("assistant") => render_assistant(&value, out),
        Some("result") => render_result(&value, out),
        _ => Ok(()),
    }
}

fn render_assistant(value: &Value, out: &mut impl Write) -> std::io::Result<()> {
    let Some(blocks) = value.pointer("/message/content").and_then(Value::as_array) else {
        return Ok(());
    };
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                if !text.is_empty() {
                    writeln!(out, "{text}")?;
                }
            }
            Some("tool_use") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                let detail = block.get("input").map(tool_detail).unwrap_or_default();
                if detail.is_empty() {
                    writeln!(out, "▸ {name}")?;
                } else {
                    writeln!(out, "▸ {name} {detail}")?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn render_result(value: &Value, out: &mut impl Write) -> std::io::Result<()> {
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !is_error {
        return Ok(());
    }
    let result = value.get("result").and_then(Value::as_str).unwrap_or("");
    writeln!(out, "! {result}")
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
    use lgtm_protocol::{OutputStream, TaskEvent};

    fn render_line(line: &str) -> String {
        let event = TaskEvent::Output {
            stream: OutputStream::Stdout,
            line: line.to_string(),
        };
        let mut out = Vec::new();
        render(&event, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn system_init_is_silent() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"a","tools":["Task","Bash"]}"#;
        assert_eq!(render_line(line), "");
    }

    #[test]
    fn assistant_text_is_printed() {
        let line = r#"{"type":"assistant","message":{"model":"claude-x","id":"m","type":"message","role":"assistant","content":[{"type":"text","text":"Hi."}],"stop_reason":null},"parent_tool_use_id":null,"session_id":"a"}"#;
        assert_eq!(render_line(line), "Hi.\n");
    }

    #[test]
    fn tool_use_shows_command() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls -la","description":"List files"}}]},"session_id":"a"}"#;
        assert_eq!(render_line(line), "▸ Bash ls -la\n");
    }

    #[test]
    fn rate_limit_event_is_silent() {
        let line = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#;
        assert_eq!(render_line(line), "");
    }

    #[test]
    fn successful_result_is_silent() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":1,"result":"Hi.","session_id":"a"}"#;
        assert_eq!(render_line(line), "");
    }

    #[test]
    fn non_json_line_is_echoed() {
        assert_eq!(render_line("plain text output"), "plain text output\n");
    }

    #[test]
    fn message_event_is_quoted() {
        let event = TaskEvent::Message {
            text: "use the existing helper".into(),
        };
        let mut out = Vec::new();
        render(&event, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "> use the existing helper\n"
        );
    }

    #[test]
    fn print_validation_marks_failures_with_output_tail() {
        let results = vec![
            ValidationResult {
                name: "test".into(),
                command: "bun test".into(),
                ok: true,
                output_tail: String::new(),
            },
            ValidationResult {
                name: "lint".into(),
                command: "bun lint".into(),
                ok: false,
                output_tail: "line1\nline2".into(),
            },
        ];
        let mut out = Vec::new();
        print_validation(&results, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\n✓ test\n✗ lint\n    line1\n    line2\n"
        );
    }
}
