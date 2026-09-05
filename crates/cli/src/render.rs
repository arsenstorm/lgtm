//! Turns a `TaskEvent` into the lines a human watches scroll by.
//!
//! What the agent did arrives as its own events (`Progress`, `Command`,
//! `FileChanged`, `Validating`), so agent stdout is echoed only when it is
//! not stream-json, plus the failed `result` line. The rest of that stream is
//! bookkeeping no human wants to read.

use lgtm_agent::codex_error;
use lgtm_protocol::{
    first_line_title, Execution, ExecutionStatus, OutputStream, Plan, PlanVersion, Provenance,
    Review, ReviewState, Severity, SkillRef, Stats, Task, TaskEvent, ValidationResult,
};
use serde_json::Value;
use std::io::Write;

use crate::table::wire_str;

/// The "agent started" line: the model in parens when one was requested,
/// then every skill it was handed, since both answer "what is this run
/// working with" before any output has arrived.
fn started_line(model: Option<&str>, skills: &[SkillRef]) -> String {
    let mut line = "agent started".to_string();
    if let Some(model) = model {
        line.push_str(&format!(" ({model})"));
    }
    if !skills.is_empty() {
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        line.push_str(&format!(", skills: {}", names.join(", ")));
    }
    line
}

pub fn render(event: &TaskEvent, out: &mut impl Write) -> std::io::Result<()> {
    match event {
        TaskEvent::Started { model, skills } => {
            writeln!(out, "{}", started_line(model.as_deref(), skills))
        }
        TaskEvent::Message { text, .. } => writeln!(out, "> {text}"),
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
        TaskEvent::Artefact { name, size, .. } => writeln!(out, "artefact: {name} ({size} bytes)"),
        TaskEvent::Validating { names } => {
            writeln!(out, "running checks: {}", names.join(", "))
        }
        TaskEvent::NetworkDenied { host } => writeln!(out, "network denied: {host}"),
        TaskEvent::PermissionRequested {
            kind,
            target,
            reason,
        } => writeln!(out, "permission requested: {kind} {target} — {reason}"),
        TaskEvent::HostAllowed { host } => writeln!(out, "allowed host {host}"),
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
        TaskEvent::Requeued { runner, executor } => writeln!(
            out,
            "requeued on {} ({})",
            runner.as_deref().unwrap_or("any runner"),
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
        TaskEvent::PrReviewed { state, url } => {
            writeln!(out, "pr review: {} {url}", review_word(*state))
        }
    }
}

fn review_word(state: ReviewState) -> &'static str {
    match state {
        ReviewState::Approved => "approved",
        ReviewState::ChangesRequested => "changes requested",
    }
}

/// `{m}m{s}s`, the duration format used wherever we print one.
fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{}m{}s", secs / 60, secs % 60)
}

/// The header for `lgtm show`: what was asked, where it ran, what it touched.
/// A field still at its default is left out — it tells the reader nothing, and
/// the point of this is to be read.
pub fn print_task(task: &Task, out: &mut impl Write) -> std::io::Result<()> {
    writeln!(out, "{}  {}", task.id, wire_str(task.status))?;
    let prompt = task.spec.prompt.trim();
    if !prompt.is_empty() {
        writeln!(out, "{prompt}")?;
    }
    writeln!(out)?;
    writeln!(out, "{:<12}{}", "repository", task.spec.repository)?;
    writeln!(out, "{:<12}{}", "base", task.spec.base_branch)?;
    writeln!(out, "{:<12}{}", "executor", task.spec.executor.binary())?;
    if let Some(model) = &task.spec.model {
        writeln!(out, "{:<12}{model}", "model")?;
    }
    if let Some(runner) = &task.runner {
        writeln!(out, "{:<12}{runner}", "runner")?;
    }
    if let Some(sandbox) = task.spec.sandbox {
        writeln!(out, "{:<12}{}", "sandbox", sandbox.as_str())?;
    }
    if let Some(result) = &task.result {
        writeln!(out, "{:<12}{}", "branch", result.branch)?;
        if !result.changed_files.is_empty() {
            writeln!(out, "{:<12}{}", "files", result.changed_files.join(", "))?;
        }
    }
    if let Some(error) = &task.error {
        writeln!(out, "{:<12}{error}", "error")?;
    }
    Ok(())
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
            exec.runner,
            exec.executor.binary(),
            duration(ms),
        )?;
    }
    Ok(())
}

/// `lgtm stats`: throughput, duration, and cost over the window. The budget
/// line only appears when some repository in view declared one.
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
    if let Some(budget) = stats.budget_daily_usd {
        writeln!(
            out,
            "{:<13}${:.2} of ${:.2} today",
            "budget", stats.spent_today, budget
        )?;
    }
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
    for entry in &stats.by_runner {
        writeln!(
            out,
            "{:<13}{} attempts, {} failed, median {}",
            entry.runner,
            entry.attempts,
            entry.failed,
            duration(entry.median_ms),
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

/// Coarse enough that clock skew of a few seconds never shows: "just now",
/// then whole minutes, hours, or days.
fn relative_age(at_ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let secs = now.saturating_sub(at_ms) / 1000;
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86_400),
    }
}

/// `lgtm why <sha>`: everything LGTM's own records say about why a commit
/// exists. `sha` is what the caller asked about, not stored on `Provenance`
/// itself — the API already spends it as the route's path parameter.
pub fn print_provenance(
    sha: &str,
    provenance: &Provenance,
    out: &mut impl Write,
) -> std::io::Result<()> {
    writeln!(out, "commit   {sha}")?;
    writeln!(
        out,
        "task     {}  {}",
        provenance.task.id,
        first_line_title(&provenance.task.spec.prompt)
    )?;
    if let Some(goal) = &provenance.goal {
        writeln!(out, "goal     {}  {}", goal.id, goal.objective)?;
    }
    if let Some(plan) = &provenance.plan {
        writeln!(out, "plan     v{}  {}", plan.version, wire_str(plan.status))?;
    }
    let model = provenance
        .task
        .spec
        .model
        .as_deref()
        .map_or(String::new(), |m| format!(" {m}"));
    writeln!(
        out,
        "runner   {}  {}{model}",
        provenance.task.runner.as_deref().unwrap_or("-"),
        provenance.task.spec.executor.binary()
    )?;
    if let Some(review) = &provenance.review {
        let blocking = review
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Blocking)
            .count();
        let by = review
            .executor
            .map_or(String::new(), |e| format!(" by {}", e.binary()));
        writeln!(
            out,
            "review   {} findings ({blocking} blocking){by}",
            review.findings.len()
        )?;
    }
    for decision in &provenance.decisions {
        let mut line = Vec::new();
        render(&decision.event, &mut line)?;
        write!(out, "policy   {}", String::from_utf8_lossy(&line))?;
    }
    if let Some(approval) = &provenance.approval {
        let by = match approval.event {
            TaskEvent::AutoApproved => "by policy",
            _ => "by a person",
        };
        writeln!(out, "approved {by} at {}", relative_age(approval.at))?;
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
                model: Some("opus".into()),
                skills: Vec::new(),
            }),
            "agent started (opus)\n"
        );
        assert_eq!(
            rendered(&TaskEvent::Started {
                model: None,
                skills: Vec::new(),
            }),
            "agent started\n"
        );
    }

    #[test]
    fn started_lists_the_skills_it_was_handed() {
        assert_eq!(
            rendered(&TaskEvent::Started {
                model: Some("opus".into()),
                skills: vec![
                    SkillRef {
                        name: "review".into(),
                        revision: 1,
                    },
                    SkillRef {
                        name: "commit".into(),
                        revision: 1,
                    },
                ],
            }),
            "agent started (opus), skills: review, commit\n"
        );
        assert_eq!(
            rendered(&TaskEvent::Started {
                model: None,
                skills: vec![SkillRef {
                    name: "review".into(),
                    revision: 1,
                }],
            }),
            "agent started, skills: review\n"
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
        assert_eq!(
            rendered(&TaskEvent::PermissionRequested {
                kind: "network".into(),
                target: "registry.internal".into(),
                reason: "install a private package".into(),
            }),
            "permission requested: network registry.internal — install a private package\n"
        );
        assert_eq!(
            rendered(&TaskEvent::HostAllowed {
                host: "registry.internal".into()
            }),
            "allowed host registry.internal\n"
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
    fn pr_reviewed_names_the_state_and_the_url() {
        assert_eq!(
            rendered(&TaskEvent::PrReviewed {
                state: lgtm_protocol::ReviewState::Approved,
                url: "https://github.com/o/r/pull/1#pullrequestreview-1".into(),
            }),
            "pr review: approved https://github.com/o/r/pull/1#pullrequestreview-1\n"
        );
        assert_eq!(
            rendered(&TaskEvent::PrReviewed {
                state: lgtm_protocol::ReviewState::ChangesRequested,
                url: "https://github.com/o/r/pull/1#pullrequestreview-2".into(),
            }),
            "pr review: changes requested https://github.com/o/r/pull/1#pullrequestreview-2\n"
        );
    }

    #[test]
    fn message_event_is_quoted() {
        let event = TaskEvent::Message {
            text: "use the existing helper".into(),
            by: None,
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
            runner: "w1".into(),
            executor: lgtm_protocol::Executor::Claude,
            model: None,
            started_at: 1_000,
            finished_at,
            status,
            error: None,
            cost_usd: 0.0,
            validation: Vec::new(),
            artefacts: Vec::new(),
            skills: Vec::new(),
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
            by_runner: vec![lgtm_protocol::RunnerStats {
                runner: "w1".into(),
                attempts: 3,
                failed: 1,
                median_ms: 90_000,
            }],
            budget_daily_usd: None,
            spent_today: 0.0,
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
             codex        1 attempts, 1 completed, 0 failed\n\
             w1           3 attempts, 1 failed, median 1m30s\n"
        );
    }

    #[test]
    fn print_stats_shows_the_budget_line_only_when_one_was_declared() {
        let stats = Stats {
            budget_daily_usd: Some(100.0),
            spent_today: 42.5,
            ..Stats::default()
        };
        let mut out = Vec::new();
        print_stats(&stats, &mut out).unwrap();
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("budget       $42.50 of $100.00 today\n"));
    }

    #[test]
    fn print_task_shows_the_prompt_and_where_it_ran() {
        let mut out = Vec::new();
        print_task(&provenance_task(None), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("abc12345  approved\n"));
        assert!(text.contains("add a /health endpoint"));
        assert!(text.contains("executor    codex"));
        assert!(text.contains("runner      w1"));
    }

    #[test]
    fn print_task_leaves_out_what_was_never_set() {
        let mut out = Vec::new();
        print_task(&provenance_task(None), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        // No result, error, model or sandbox on this task: naming them with
        // nothing after the label is worse than not naming them.
        for absent in ["model", "sandbox", "branch", "files", "error"] {
            assert!(!text.contains(absent), "{absent} should not be printed");
        }
    }

    #[test]
    fn print_task_names_the_branch_and_the_files_it_changed() {
        let mut task = provenance_task(Some("opus"));
        task.spec.sandbox = Some(lgtm_protocol::SandboxProfile::Off);
        task.result = Some(lgtm_protocol::TaskResult {
            branch: "lgtm/abc12345".into(),
            diff: String::new(),
            changed_files: vec!["src/a.rs".into(), "src/b.rs".into()],
            validation: Vec::new(),
            plan: None,
            review: None,
            policy: None,
            cost_usd: 0.0,
        });
        let mut out = Vec::new();
        print_task(&task, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("model       opus"));
        assert!(text.contains("sandbox     off"));
        assert!(text.contains("branch      lgtm/abc12345"));
        assert!(text.contains("files       src/a.rs, src/b.rs"));
    }

    #[test]
    fn print_task_shows_why_a_task_failed() {
        let mut task = provenance_task(None);
        task.status = lgtm_protocol::TaskStatus::Failed;
        task.error = Some("failed to write commit object".into());
        let mut out = Vec::new();
        print_task(&task, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("abc12345  failed\n"));
        assert!(text.contains("error       failed to write commit object"));
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

    fn provenance_task(model: Option<&str>) -> lgtm_protocol::Task {
        lgtm_protocol::Task {
            id: "abc12345".into(),
            title: None,
            spec: lgtm_protocol::TaskSpec {
                repository: "https://example.com/repo.git".into(),
                base_branch: "main".into(),
                prompt: "add a /health endpoint\n\ndetails".into(),
                executor: lgtm_protocol::Executor::Codex,
                runner: None,
                issue: None,
                linear: None,
                kind: lgtm_protocol::TaskKind::Run,
                parent: None,
                depends_on: Vec::new(),
                depends_on_condition: Default::default(),
                batch: None,
                sandbox: None,
                requirements: Vec::new(),
                goal: None,
                review_executor: None,
                model: model.map(str::to_string),
                reasoning_effort: None,
                allowed_hosts: Vec::new(),
                session: None,
                created_by: None,
            },
            status: lgtm_protocol::TaskStatus::Approved,
            runner: Some("w1".into()),
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            pr_review: None,
            executions: Vec::new(),
            scratchpad: String::new(),
            files: Vec::new(),
            workspace: None,
            created_by: None,
            archived: false,
        }
    }

    #[test]
    fn print_provenance_renders_every_section() {
        let approved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - 3_600_000;
        let provenance = Provenance {
            task: provenance_task(Some("gpt-5-codex")),
            goal: Some(lgtm_protocol::Goal {
                id: "g1".into(),
                objective: "ship health checks".into(),
                repository: "https://example.com/repo.git".into(),
                created_at: 0,
                attention: None,
                workspace: None,
                created_by: None,
            }),
            plan: Some(lgtm_protocol::PlanVersion {
                task: "plan1".into(),
                goal: Some("g1".into()),
                version: 2,
                status: lgtm_protocol::PlanStatus::Approved,
                created_at: 0,
                plan: Plan { steps: Vec::new() },
            }),
            review: Some(Review {
                findings: vec![Finding {
                    severity: Severity::Blocking,
                    file: "a.rs".into(),
                    line: None,
                    message: "nit".into(),
                }],
                executor: Some(lgtm_protocol::Executor::Codex),
            }),
            decisions: vec![lgtm_protocol::StoredEvent {
                at: 0,
                event: TaskEvent::PolicyDecision {
                    action: "approve".into(),
                    allowed: true,
                    reasons: vec!["checks passed".into()],
                },
            }],
            approval: Some(lgtm_protocol::StoredEvent {
                at: approved_at,
                event: TaskEvent::AutoApproved,
            }),
        };
        let mut out = Vec::new();
        print_provenance("abc1234", &provenance, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "commit   abc1234\n\
             task     abc12345  add a /health endpoint\n\
             goal     g1  ship health checks\n\
             plan     v2  approved\n\
             runner   w1  codex gpt-5-codex\n\
             review   1 findings (1 blocking) by codex\n\
             policy   policy: auto-approve (checks passed)\n\
             approved by policy at 1h ago\n"
        );
    }

    #[test]
    fn print_provenance_hides_absent_sections_and_names_a_person() {
        let provenance = Provenance {
            task: provenance_task(None),
            goal: None,
            plan: None,
            review: None,
            decisions: Vec::new(),
            approval: Some(lgtm_protocol::StoredEvent {
                at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
                event: TaskEvent::Pushed {
                    branch: "lgtm/abc12345".into(),
                    sha: "abc1234".into(),
                },
            }),
        };
        let mut out = Vec::new();
        print_provenance("abc1234", &provenance, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "commit   abc1234\n\
             task     abc12345  add a /health endpoint\n\
             runner   w1  codex\n\
             approved by a person at just now\n"
        );
    }
}
