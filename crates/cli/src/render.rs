//! Turns a `TaskEvent` into the lines a human watches scroll by.
//!
//! Agent stdout is `claude -p --output-format stream-json`: one JSON object
//! per line. We only surface the parts a human cares about (assistant text,
//! tool calls, failed results) and stay silent on the rest (system/user
//! bookkeeping, rate-limit pings, successful results already implied by
//! `Completed`).

use lgtm_protocol::{
    Execution, ExecutionStatus, OutputStream, Plan, Review, Severity, TaskEvent, ValidationResult,
};
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
            if let Some(plan) = &result.plan {
                return writeln!(out, "plan: {} steps", plan.steps.len());
            }
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
        TaskEvent::Retry { attempt, reason } => writeln!(out, "retry {attempt}: {reason}"),
        TaskEvent::AutoApproved => writeln!(out, "approved by policy"),
        TaskEvent::AutoMerged => writeln!(out, "merged by policy"),
        TaskEvent::Failed { error } => writeln!(out, "failed: {error}"),
        TaskEvent::Cancelled => writeln!(out, "cancelled"),
        TaskEvent::Pushed { branch, .. } => writeln!(out, "pushed {branch}"),
        TaskEvent::Discarded => writeln!(out, "discarded"),
    }
}

/// Renders one line per attempt. Silent for a task that was attempted once,
/// where the task's own status and result already say everything.
pub fn print_executions(execs: &[Execution], out: &mut impl Write) -> std::io::Result<()> {
    if execs.len() < 2 {
        return Ok(());
    }
    writeln!(out)?;
    for exec in execs {
        let status = match exec.status {
            ExecutionStatus::Running => "running",
            ExecutionStatus::Completed => "completed",
            ExecutionStatus::Failed => "failed",
            ExecutionStatus::Cancelled => "cancelled",
        };
        let secs = exec
            .finished_at
            .unwrap_or(exec.started_at)
            .saturating_sub(exec.started_at)
            / 1000;
        writeln!(
            out,
            "attempt {}: {status} on {} ({}) {}m{}s",
            exec.attempt,
            exec.worker,
            exec.executor.binary(),
            secs / 60,
            secs % 60,
        )?;
    }
    Ok(())
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

/// Renders each finding after a `Completed` event, one line per finding. A
/// blank line precedes them. No-op when the review has no findings (the
/// reviewer agent found nothing to flag).
pub fn print_review(review: &Review, out: &mut impl Write) -> std::io::Result<()> {
    if review.findings.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    for finding in &review.findings {
        let mark = match finding.severity {
            Severity::Blocking => '✖',
            Severity::Warning => '⚠',
        };
        let location = match (finding.file.is_empty(), finding.line) {
            (true, None) => String::new(),
            (true, Some(line)) => format!("{line} "),
            (false, Some(line)) => format!("{}:{line} ", finding.file),
            (false, None) => format!("{} ", finding.file),
        };
        writeln!(out, "{mark} {location}{}", finding.message)?;
    }
    Ok(())
}

/// Renders the task's total agent cost after a `Completed` event. No-op
/// when the cost is zero (not reported, or a free local model).
pub fn print_cost(cost: f64, out: &mut impl Write) -> std::io::Result<()> {
    if cost > 0.0 {
        writeln!(out, "cost: ${cost:.2}")?;
    }
    Ok(())
}

/// Renders a plan's steps, one line per step, 1-based, with a dependency
/// note underneath a step that has any.
pub fn print_plan(plan: &Plan, out: &mut impl Write) -> std::io::Result<()> {
    for (i, step) in plan.steps.iter().enumerate() {
        writeln!(out, "{}. {}  {}", i + 1, step.key, step.title)?;
        if !step.depends_on.is_empty() {
            writeln!(out, "  (after: {})", step.depends_on.join(", "))?;
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
    use lgtm_protocol::{Finding, OutputStream, TaskEvent};

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

    #[test]
    fn print_review_is_silent_without_findings() {
        let mut out = Vec::new();
        print_review(&Review::default(), &mut out).unwrap();
        assert_eq!(out, b"");
    }

    #[test]
    fn print_review_formats_both_severities_and_optional_parts() {
        let review = Review {
            findings: vec![
                Finding {
                    severity: Severity::Blocking,
                    file: "src/a.rs".into(),
                    line: Some(3),
                    message: "unwrap on user input".into(),
                },
                Finding {
                    severity: Severity::Warning,
                    file: "src/b.rs".into(),
                    line: None,
                    message: "unused import".into(),
                },
                Finding {
                    severity: Severity::Warning,
                    file: String::new(),
                    line: Some(9),
                    message: "line without a file".into(),
                },
                Finding {
                    severity: Severity::Blocking,
                    file: String::new(),
                    line: None,
                    message: "no location at all".into(),
                },
            ],
        };
        let mut out = Vec::new();
        print_review(&review, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\n\
             ✖ src/a.rs:3 unwrap on user input\n\
             ⚠ src/b.rs unused import\n\
             ⚠ 9 line without a file\n\
             ✖ no location at all\n"
        );
    }

    fn execution(attempt: u32, status: ExecutionStatus, finished_at: Option<u64>) -> Execution {
        Execution {
            attempt,
            worker: "w1".into(),
            executor: lgtm_protocol::Executor::Claude,
            started_at: 1_000,
            finished_at,
            status,
            error: None,
            cost_usd: 0.0,
            validation: Vec::new(),
        }
    }

    #[test]
    fn print_executions_hides_a_single_attempt() {
        let mut out = Vec::new();
        print_executions(
            &[execution(1, ExecutionStatus::Completed, Some(2_000))],
            &mut out,
        )
        .unwrap();
        assert_eq!(out, b"");
    }

    #[test]
    fn print_executions_lists_every_attempt_with_its_duration() {
        let execs = vec![
            execution(1, ExecutionStatus::Failed, Some(91_000)),
            execution(2, ExecutionStatus::Running, None),
        ];
        let mut out = Vec::new();
        print_executions(&execs, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\n\
             attempt 1: failed on w1 (claude) 1m30s\n\
             attempt 2: running on w1 (claude) 0m0s\n"
        );
    }

    #[test]
    fn print_cost_hides_zero() {
        let mut out = Vec::new();
        print_cost(0.0, &mut out).unwrap();
        assert_eq!(out, b"");
    }

    #[test]
    fn print_cost_shows_positive_amount() {
        let mut out = Vec::new();
        print_cost(0.42, &mut out).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "cost: $0.42\n");
    }
}
