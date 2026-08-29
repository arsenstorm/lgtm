//! Turns a `TaskEvent` into the lines a human watches scroll by.
//!
//! What the agent did arrives as its own events (`Progress`, `Command`,
//! `FileChanged`, `Validating`), so agent stdout is echoed only when it is
//! not stream-json, plus the failed `result` line. The rest of that stream is
//! bookkeeping no human wants to read.

use lgtm_agent::codex_error;
use lgtm_protocol::{
    Execution, ExecutionStatus, OutputStream, Plan, PlanVersion, Review, Severity, Stats,
    TaskEvent, ValidationResult,
};
use serde_json::Value;
use std::io::Write;

use crate::table::wire_str;

pub fn render(event: &TaskEvent, out: &mut impl Write) -> std::io::Result<()> {
    match event {
        TaskEvent::Started { model: Some(m) } => writeln!(out, "agent started ({m})"),
        TaskEvent::Started { .. } => writeln!(out, "agent started"),
        TaskEvent::Message { text } => writeln!(out, "> {text}"),
        TaskEvent::Output {
            stream: OutputStream::Stderr,
            line,
        } => writeln!(out, "! {line}"),
        TaskEvent::Output {
            stream: OutputStream::Stdout,
            line,
        } => render_stdout(line, out),
        TaskEvent::Command { command } => writeln!(out, "$ {command}"),
        TaskEvent::FileChanged { path } => writeln!(out, "~ {path}"),
        TaskEvent::Progress { text } => writeln!(out, "{text}"),
        TaskEvent::Scratchpad { .. } => writeln!(out, "notes updated"),
        TaskEvent::Validating { names } => {
            writeln!(out, "running checks: {}", names.join(", "))
        }
        TaskEvent::NetworkDenied { host } => writeln!(out, "network denied: {host}"),
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
        TaskEvent::Requeued { worker, executor } => writeln!(
            out,
            "requeued on {} ({})",
            worker.as_deref().unwrap_or("any worker"),
            executor.binary()
        ),
        TaskEvent::PolicyDecision {
            action,
            allowed,
            reasons,
        } => {
            if *allowed {
                writeln!(out, "policy: auto-{action} ({})", reasons.join(", "))
            } else {
                writeln!(out, "policy: no auto-{action}: {}", reasons.join("; "))
            }
        }
        TaskEvent::Orchestrated {
            action,
            reason,
            applied,
            note,
        } => {
            if *applied {
                writeln!(out, "orchestrator: {action} — {reason}")
            } else {
                writeln!(
                    out,
                    "orchestrator wanted {action} ({reason}); not applied: {note}"
                )
            }
        }
        TaskEvent::AutoApproved => writeln!(out, "approved by policy"),
        TaskEvent::AutoMerged => writeln!(out, "merged by policy"),
        TaskEvent::Failed { error } => writeln!(out, "failed: {error}"),
        TaskEvent::TimedOut { secs } => writeln!(out, "timed out after {secs}s"),
        TaskEvent::RunnerLost => writeln!(out, "runner lost"),
        TaskEvent::Cancelled => writeln!(out, "cancelled"),
        TaskEvent::Conflicted { base, files } => {
            writeln!(out, "conflicted with {base}: {}", files.join(", "))
        }
        TaskEvent::Pushed { branch, .. } => writeln!(out, "pushed {branch}"),
        TaskEvent::Discarded => writeln!(out, "discarded"),
    }
}

/// `{m}m{s}s`, the duration format used wherever we print one.
fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{}m{}s", secs / 60, secs % 60)
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
        let ms = exec
            .finished_at
            .unwrap_or(exec.started_at)
            .saturating_sub(exec.started_at);
        let model = exec
            .model
            .as_deref()
            .map_or(String::new(), |m| format!(" [{m}]"));
        writeln!(
            out,
            "attempt {}: {status} on {} ({}){model} {}",
            exec.attempt,
            exec.worker,
            exec.executor.binary(),
            duration(ms),
        )?;
    }
    Ok(())
}

/// `lgtm stats`: throughput, duration, and cost over the window.
pub fn print_stats(stats: &Stats, out: &mut impl Write) -> std::io::Result<()> {
    let done = stats.approved + stats.merged;
    let dropped = stats.cancelled + stats.rejected;
    let open = stats.running + stats.queued + stats.awaiting_review;
    writeln!(
        out,
        "{:<13}{}   ({done} done, {} failed, {dropped} dropped, {open} open)",
        "tasks", stats.tasks, stats.failed,
    )?;
    writeln!(
        out,
        "{:<13}{}",
        "median run",
        duration(stats.median_execution_ms)
    )?;
    writeln!(
        out,
        "{:<13}{}",
        "median queue",
        duration(stats.median_queue_ms)
    )?;
    writeln!(out, "{:<13}{}", "retried", stats.retried_tasks)?;
    writeln!(out, "{:<13}${:.2}", "cost", stats.cost_usd)?;
    for entry in &stats.by_executor {
        writeln!(
            out,
            "{:<13}{} attempts, {} completed, {} failed",
            entry.executor.binary(),
            entry.attempts,
            entry.completed,
            entry.failed,
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
    if let Some(executor) = review.executor {
        writeln!(out, "reviewed by {}", executor.binary())?;
    }
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

/// Renders each version: a `v{n}  {status}  {created_at}` header, then its
/// steps (via `print_plan`) indented two spaces, blank line between versions.
pub fn print_plan_versions(versions: &[PlanVersion], out: &mut impl Write) -> std::io::Result<()> {
    for (i, version) in versions.iter().enumerate() {
        if i > 0 {
            writeln!(out)?;
        }
        writeln!(
            out,
            "v{}  {}  {}",
            version.version,
            wire_str(version.status),
            version.created_at
        )?;
        let mut steps = Vec::new();
        print_plan(&version.plan, &mut steps)?;
        for line in String::from_utf8_lossy(&steps).lines() {
            writeln!(out, "  {line}")?;
        }
    }
    Ok(())
}

fn render_stdout(line: &str, out: &mut impl Write) -> std::io::Result<()> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return writeln!(out, "{line}");
    };
    match value.get("type").and_then(Value::as_str) {
        Some("result") => render_result(&value, out),
        Some("item.completed") | Some("turn.failed") => match codex_error(&value) {
            Some(message) => writeln!(out, "! {message}"),
            None => Ok(()),
        },
        _ => Ok(()),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Finding, OutputStream, TaskEvent};

    fn rendered(event: &TaskEvent) -> String {
        let mut out = Vec::new();
        render(event, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn started_names_the_requested_model_when_one_was_asked_for() {
        assert_eq!(
            rendered(&TaskEvent::Started {
                model: Some("opus".into())
            }),
            "agent started (opus)\n"
        );
        assert_eq!(
            rendered(&TaskEvent::Started { model: None }),
            "agent started\n"
        );
    }

    #[test]
    fn policy_decisions_read_as_a_sentence() {
        assert_eq!(
            rendered(&TaskEvent::PolicyDecision {
                action: "approve".into(),
                allowed: true,
                reasons: vec!["checks passed".into(), "12 lines".into()],
            }),
            "policy: auto-approve (checks passed, 12 lines)\n"
        );
        assert_eq!(
            rendered(&TaskEvent::PolicyDecision {
                action: "merge".into(),
                allowed: false,
                reasons: vec!["ci failure".into()],
            }),
            "policy: no auto-merge: ci failure\n"
        );
    }

    fn render_line(line: &str) -> String {
        rendered(&TaskEvent::Output {
            stream: OutputStream::Stdout,
            line: line.to_string(),
        })
    }

    #[test]
    fn a_codex_error_prints_like_a_failed_result() {
        let item = r#"{"type":"item.completed","item":{"type":"error","message":"boom"}}"#;
        assert_eq!(render_line(item), "! boom\n");
        let turn = r#"{"type":"turn.failed","error":{"message":"usage limit"}}"#;
        assert_eq!(render_line(turn), "! usage limit\n");
        let said = r#"{"type":"item.completed","item":{"type":"agent_message","text":"hi"}}"#;
        assert_eq!(render_line(said), "");
    }

    #[test]
    fn system_init_is_silent() {
        let line = r#"{"type":"system","subtype":"init","cwd":"/x","session_id":"a","tools":["Task","Bash"]}"#;
        assert_eq!(render_line(line), "");
    }

    /// The runner sends the same content as `Progress` and `Command`, so
    /// echoing the raw line would print all of it twice.
    #[test]
    fn assistant_stdout_is_silent() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hi."},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls -la"}}]},"session_id":"a"}"#;
        assert_eq!(render_line(line), "");
    }

    #[test]
    fn structured_events_each_have_their_own_line() {
        assert_eq!(
            rendered(&TaskEvent::Progress {
                text: "Looking.".into()
            }),
            "Looking.\n"
        );
        assert_eq!(
            rendered(&TaskEvent::Command {
                command: "ls -la".into()
            }),
            "$ ls -la\n"
        );
        assert_eq!(
            rendered(&TaskEvent::FileChanged {
                path: "src/a.rs".into()
            }),
            "~ src/a.rs\n"
        );
        assert_eq!(
            rendered(&TaskEvent::Validating {
                names: vec!["test".into(), "lint".into()]
            }),
            "running checks: test, lint\n"
        );
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
            executor: Some(lgtm_protocol::Executor::Codex),
        };
        let mut out = Vec::new();
        print_review(&review, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\n\
             reviewed by codex\n\
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
            model: None,
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
    fn print_executions_names_the_model_when_one_ran() {
        let mut exec = execution(1, ExecutionStatus::Completed, Some(2_000));
        exec.model = Some("opus".into());
        let mut out = Vec::new();
        print_executions(
            &[exec, execution(2, ExecutionStatus::Running, None)],
            &mut out,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\n\
             attempt 1: completed on w1 (claude) [opus] 0m1s\n\
             attempt 2: running on w1 (claude) 0m0s\n"
        );
    }

    #[test]
    fn print_stats_renders_every_field() {
        let stats = Stats {
            since: 0,
            tasks: 5,
            queued: 1,
            running: 1,
            awaiting_review: 1,
            approved: 1,
            merged: 1,
            failed: 0,
            cancelled: 0,
            rejected: 0,
            median_execution_ms: 90_000,
            median_queue_ms: 5_000,
            retried_tasks: 2,
            cost_usd: 12.5,
            by_executor: vec![
                lgtm_protocol::ExecutorStats {
                    executor: lgtm_protocol::Executor::Claude,
                    attempts: 3,
                    completed: 2,
                    failed: 1,
                },
                lgtm_protocol::ExecutorStats {
                    executor: lgtm_protocol::Executor::Codex,
                    attempts: 1,
                    completed: 1,
                    failed: 0,
                },
            ],
        };
        let mut out = Vec::new();
        print_stats(&stats, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "tasks        5   (2 done, 0 failed, 0 dropped, 3 open)\n\
             median run   1m30s\n\
             median queue 0m5s\n\
             retried      2\n\
             cost         $12.50\n\
             claude       3 attempts, 2 completed, 1 failed\n\
             codex        1 attempts, 1 completed, 0 failed\n"
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
