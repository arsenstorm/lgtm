//! One task's agent work: the run and its retries, the fix-the-checks loop,
//! and the review. Every run of the executor for a task goes through here, so
//! one cost total and one cancel receiver cover all of them.

use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result};
use lgtm_protocol::{
    Executor, OutputStream, Policy, Task, TaskEvent, TaskKind, TaskResult, ValidationResult,
};
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::connection::Ctx;
use crate::git::{branch_name, commit, session_path};
use crate::plan::extract_plan;
use crate::policy::{
    failed_names, fix_prompt, load_policy, parse_review, review_prompt, review_warning,
    PolicyConfig,
};
use crate::proc::{
    cost_buffer, cost_total, final_text, pump, tail_buffer, tail_lines, text_buffer, Cost, Sinks,
    Text,
};
use crate::validate::{load_validation, run_validation, tail};

/// Commit subject for the follow-up run that fixed the checks.
const FIX_MESSAGE: &str = "fix failing checks";

/// What one spawn of the executor is asked to do.
struct RunOpts<'a> {
    prompt: &'a str,
    /// Agent session to continue, when the run belongs to an earlier one.
    resume: Option<String>,
    /// Claude's `--permission-mode`; the reviewer must not edit.
    permission: &'a str,
    /// Set to collect the agent's final answer.
    answer: Option<Text>,
    /// Whether the run's session id replaces the task's recorded one.
    record_session: bool,
}

/// A finished run. `None` from a run means the task was cancelled.
struct Finish {
    status: ExitStatus,
    stderr_tail: String,
}

/// The agent run for a task, and everything the repository's policy adds to it.
pub async fn execute(
    task: &Task,
    prompt: &str,
    resume: Option<String>,
    worktree: &Path,
    ctx: &Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
) -> Result<()> {
    let planning = task.spec.kind == TaskKind::Plan;
    let policy = load_policy(worktree);
    let cost = cost_buffer();
    let mut cancel = cancel;
    let answer = planning.then(text_buffer);
    let branch = branch_name(&task.id);
    let binary = task.spec.executor.binary();

    let mut attempt = 0;
    let mut session = resume.filter(|_| !planning);
    let finish = loop {
        let opts = RunOpts {
            prompt,
            resume: session.take(),
            permission: if planning { "default" } else { "acceptEdits" },
            answer: answer.clone(),
            record_session: true,
        };
        let Some(finish) = agent_run(task, worktree, ctx, &cost, &mut cancel, opts).await? else {
            return cancelled(task, ctx);
        };
        // A plan run leaves nothing behind to retry into.
        if finish.status.success() || planning || attempt >= policy.retry {
            break finish;
        }
        attempt += 1;
        ctx.emit(
            &task.id,
            TaskEvent::Retry {
                attempt,
                reason: format!("{binary} exited with {}", finish.status),
            },
        );
    };

    if !finish.status.success() {
        ctx.emit(
            &task.id,
            TaskEvent::Failed {
                error: format!(
                    "{binary} exited with {}\n{}",
                    finish.status, finish.stderr_tail
                ),
            },
        );
        return Ok(());
    }

    if let Some(answer) = answer {
        let text = final_text(&answer);
        ctx.emit(&task.id, planned(&branch, &text, policy, cost_total(&cost)));
        return Ok(());
    }

    let mut result = commit(prompt, &task.spec.base_branch, &branch, worktree).await?;
    let checks = load_validation(worktree);
    result.validation = run_validation(worktree, &checks).await;

    let mut fixes = 0;
    while fixes < policy.fix_checks && result.validation_failed() {
        fixes += 1;
        let failed: Vec<&ValidationResult> =
            result.validation.iter().filter(|check| !check.ok).collect();
        ctx.emit(
            &task.id,
            TaskEvent::Retry {
                attempt: fixes,
                reason: format!("checks failed: {}", failed_names(&failed)),
            },
        );
        let opts = RunOpts {
            prompt: &fix_prompt(&failed),
            resume: recorded_session(ctx, &task.id).await,
            permission: "acceptEdits",
            answer: None,
            record_session: true,
        };
        if agent_run(task, worktree, ctx, &cost, &mut cancel, opts)
            .await?
            .is_none()
        {
            return cancelled(task, ctx);
        }
        // Whatever the fix run exited with, judge it by the checks themselves.
        result = commit(FIX_MESSAGE, &task.spec.base_branch, &branch, worktree).await?;
        result.validation = run_validation(worktree, &checks).await;
    }

    if policy.review && !result.diff.is_empty() {
        let answer = text_buffer();
        let opts = RunOpts {
            prompt: &review_prompt(&task.spec.prompt, &result.diff),
            resume: None,
            permission: "default",
            answer: Some(answer.clone()),
            record_session: false,
        };
        let Some(finish) = agent_run(task, worktree, ctx, &cost, &mut cancel, opts).await? else {
            return cancelled(task, ctx);
        };
        result.review = Some(if finish.status.success() {
            parse_review(&final_text(&answer))
        } else {
            review_warning(format!("reviewer exited with {}", finish.status))
        });
    }

    result.policy = Some(policy_of(policy));
    result.cost_usd = cost_total(&cost);
    ctx.emit(&task.id, TaskEvent::Completed { result });
    Ok(())
}

fn cancelled(task: &Task, ctx: &Arc<Ctx>) -> Result<()> {
    ctx.emit(&task.id, TaskEvent::Cancelled);
    Ok(())
}

fn policy_of(policy: PolicyConfig) -> Policy {
    Policy {
        auto_approve: policy.auto_approve,
        auto_merge: policy.auto_merge,
    }
}

/// Session id of the task's last run, when one was recorded.
pub async fn recorded_session(ctx: &Arc<Ctx>, task_id: &str) -> Option<String> {
    tokio::fs::read_to_string(session_path(&ctx.data_dir, task_id))
        .await
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

/// Spawns the executor, forwards its output, and waits. `Ok(None)` means the
/// task was cancelled: the child is killed and nothing else has happened.
async fn agent_run(
    task: &Task,
    worktree: &Path,
    ctx: &Arc<Ctx>,
    cost: &Cost,
    cancel: &mut oneshot::Receiver<()>,
    opts: RunOpts<'_>,
) -> Result<Option<Finish>> {
    let binary = task.spec.executor.binary();
    let path = which::which(binary).with_context(|| format!("{binary} not found on PATH"))?;
    let mut cmd = Command::new(&path);
    match task.spec.executor {
        Executor::Claude => {
            cmd.args(["-p", opts.prompt]);
            if let Some(session) = opts.resume.as_ref() {
                cmd.args(["--resume", session]);
            }
            cmd.args([
                "--output-format",
                "stream-json",
                "--verbose",
                "--permission-mode",
                opts.permission,
            ]);
        }
        Executor::Codex => {
            cmd.args(["exec", opts.prompt]);
        }
    };
    let mut child = cmd
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {}", path.display()))?;
    ctx.emit(&task.id, TaskEvent::Started);

    let stderr_tail = tail_buffer();
    let stdout = child.stdout.take().context("no stdout")?;
    let stderr = child.stderr.take().context("no stderr")?;
    let pump_out = tokio::spawn(pump(
        stdout,
        OutputStream::Stdout,
        ctx.clone(),
        task.id.clone(),
        Sinks {
            session: opts
                .record_session
                .then(|| session_path(&ctx.data_dir, &task.id)),
            text: opts.answer,
            cost: Some(cost.clone()),
            ..Sinks::default()
        },
    ));
    let pump_err = tokio::spawn(pump(
        stderr,
        OutputStream::Stderr,
        ctx.clone(),
        task.id.clone(),
        Sinks {
            tail: Some(stderr_tail.clone()),
            ..Sinks::default()
        },
    ));

    let waited = tokio::select! {
        status = child.wait() => Some(status),
        _ = &mut *cancel => None,
    };
    let Some(status) = waited else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Ok(None);
    };
    let status = status?;
    let _ = tokio::join!(pump_out, pump_err);
    Ok(Some(Finish {
        status,
        stderr_tail: tail(&tail_lines(&stderr_tail)),
    }))
}

/// A plan run leaves no diff, so its result carries only the parsed plan.
fn planned(branch: &str, text: &str, policy: PolicyConfig, cost_usd: f64) -> TaskEvent {
    match extract_plan(text) {
        Ok(plan) => TaskEvent::Completed {
            result: TaskResult {
                branch: branch.to_string(),
                diff: String::new(),
                changed_files: Vec::new(),
                validation: Vec::new(),
                plan: Some(plan),
                review: None,
                policy: Some(policy_of(policy)),
                cost_usd,
            },
        },
        Err(err) => TaskEvent::Failed {
            error: format!("{err:#}"),
        },
    }
}
