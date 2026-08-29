//! One task's agent work: the run and its retries, the fix-the-checks loop,
//! and the review. Every run of the executor for a task goes through here, so
//! one cost total and one cancel receiver cover all of them.

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;

use anyhow::{Context, Result};
use lgtm_protocol::{
    Executor, OutputStream, Policy, Review, Task, TaskEvent, TaskKind, TaskResult, ValidationResult,
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
    cost_buffer, cost_total, final_text, tail_buffer, tail_lines, text_buffer, Cost, Pump, Sinks,
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
    /// Where the run's session id replaces the task's recorded one.
    session: Option<PathBuf>,
}

/// A finished run. `None` from a run means the task was cancelled.
struct Finish {
    status: ExitStatus,
    stderr_tail: String,
}

/// One task's worktree, cancel receiver, and cost total, shared by every run
/// of the executor for that task.
pub struct Run<'a> {
    task: &'a Task,
    worktree: &'a Path,
    ctx: &'a Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
    cost: Cost,
}

impl<'a> Run<'a> {
    pub fn new(
        task: &'a Task,
        worktree: &'a Path,
        ctx: &'a Arc<Ctx>,
        cancel: oneshot::Receiver<()>,
    ) -> Self {
        Run {
            task,
            worktree,
            ctx,
            cancel,
            cost: cost_buffer(),
        }
    }
}

/// The agent run for a task, and everything the repository's policy adds to it.
pub async fn execute(mut run: Run<'_>, prompt: &str, resume: Option<String>) -> Result<()> {
    let policy = load_policy(run.worktree);
    if run.task.spec.kind == TaskKind::Plan {
        return run.plan(prompt, policy).await;
    }
    let Some(finish) = run.attempts(prompt, resume, policy.retry).await? else {
        return run.cancelled();
    };
    if !finish.status.success() {
        return run.failed(&finish);
    }
    let Some(mut result) = run.commit_and_fix(prompt, policy.fix_checks).await? else {
        return run.cancelled();
    };
    if policy.review && !result.diff.is_empty() {
        let Some(review) = run.review(&result.diff).await? else {
            return run.cancelled();
        };
        result.review = Some(review);
    }
    result.policy = Some(policy_of(policy));
    result.cost_usd = cost_total(&run.cost);
    run.ctx.emit(&run.task.id, TaskEvent::Completed { result });
    Ok(())
}

impl Run<'_> {
    fn cancelled(&self) -> Result<()> {
        self.ctx.emit(&self.task.id, TaskEvent::Cancelled);
        Ok(())
    }

    fn failed(&self, finish: &Finish) -> Result<()> {
        let error = format!(
            "{} exited with {}\n{}",
            self.binary(),
            finish.status,
            finish.stderr_tail
        );
        self.ctx.fail(&self.task.id, error);
        Ok(())
    }

    fn binary(&self) -> &'static str {
        self.task.spec.executor.binary()
    }

    fn session_path(&self) -> PathBuf {
        session_path(&self.ctx.data_dir, &self.task.id)
    }

    fn branch(&self) -> String {
        branch_name(&self.task.id)
    }

    /// A plan run leaves nothing behind to retry into, so it runs once.
    async fn plan(&mut self, prompt: &str, policy: PolicyConfig) -> Result<()> {
        let answer = text_buffer();
        let opts = RunOpts {
            prompt,
            resume: None,
            permission: "default",
            answer: Some(answer.clone()),
            session: Some(self.session_path()),
        };
        let Some(finish) = self.agent_run(opts).await? else {
            return self.cancelled();
        };
        if !finish.status.success() {
            return self.failed(&finish);
        }
        let event = planned(
            &self.branch(),
            &final_text(&answer),
            policy,
            cost_total(&self.cost),
        );
        self.ctx.emit(&self.task.id, event);
        Ok(())
    }

    async fn attempts(
        &mut self,
        prompt: &str,
        mut session: Option<String>,
        retries: u32,
    ) -> Result<Option<Finish>> {
        let mut attempt = 0;
        loop {
            let opts = RunOpts {
                prompt,
                resume: session.take(),
                permission: "acceptEdits",
                answer: None,
                session: Some(self.session_path()),
            };
            let Some(finish) = self.agent_run(opts).await? else {
                return Ok(None);
            };
            if finish.status.success() || attempt >= retries {
                return Ok(Some(finish));
            }
            attempt += 1;
            let reason = format!("{} exited with {}", self.binary(), finish.status);
            self.ctx
                .emit(&self.task.id, TaskEvent::Retry { attempt, reason });
        }
    }

    async fn commit_and_fix(
        &mut self,
        prompt: &str,
        fix_checks: u32,
    ) -> Result<Option<TaskResult>> {
        let base = &self.task.spec.base_branch;
        let branch = self.branch();
        let mut result = commit(prompt, base, &branch, self.worktree).await?;
        let checks = load_validation(self.worktree);
        result.validation = run_validation(self.worktree, &checks).await;

        let mut fixes = 0;
        while fixes < fix_checks && result.validation_failed() {
            fixes += 1;
            let failed: Vec<&ValidationResult> =
                result.validation.iter().filter(|check| !check.ok).collect();
            let reason = format!("checks failed: {}", failed_names(&failed));
            self.ctx.emit(
                &self.task.id,
                TaskEvent::Retry {
                    attempt: fixes,
                    reason,
                },
            );
            let opts = RunOpts {
                prompt: &fix_prompt(&failed),
                resume: recorded_session(self.ctx, &self.task.id).await,
                permission: "acceptEdits",
                answer: None,
                session: Some(self.session_path()),
            };
            if self.agent_run(opts).await?.is_none() {
                return Ok(None);
            }
            // Whatever the fix run exited with, judge it by the checks themselves.
            result = commit(FIX_MESSAGE, base, &branch, self.worktree).await?;
            result.validation = run_validation(self.worktree, &checks).await;
        }
        Ok(Some(result))
    }

    async fn review(&mut self, diff: &str) -> Result<Option<Review>> {
        let answer = text_buffer();
        let opts = RunOpts {
            prompt: &review_prompt(&self.task.spec.prompt, diff),
            resume: None,
            permission: "default",
            answer: Some(answer.clone()),
            session: None,
        };
        let Some(finish) = self.agent_run(opts).await? else {
            return Ok(None);
        };
        Ok(Some(if finish.status.success() {
            parse_review(&final_text(&answer))
        } else {
            review_warning(format!("reviewer exited with {}", finish.status))
        }))
    }

    /// Spawns the executor, forwards its output, and waits. `Ok(None)` means the
    /// task was cancelled: the child is killed and nothing else has happened.
    async fn agent_run(&mut self, opts: RunOpts<'_>) -> Result<Option<Finish>> {
        let path = which::which(self.binary())
            .with_context(|| format!("{} not found on PATH", self.binary()))?;
        let mut child = self
            .command(&path, &opts)
            .spawn()
            .with_context(|| format!("spawn {}", path.display()))?;
        self.ctx.emit(&self.task.id, TaskEvent::Started);

        let stderr_tail = tail_buffer();
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        let pump_out = tokio::spawn(
            self.pump(
                OutputStream::Stdout,
                Sinks {
                    session: opts.session,
                    text: opts.answer,
                    cost: Some(self.cost.clone()),
                    ..Sinks::default()
                },
            )
            .run(stdout),
        );
        let pump_err = tokio::spawn(
            self.pump(
                OutputStream::Stderr,
                Sinks {
                    tail: Some(stderr_tail.clone()),
                    ..Sinks::default()
                },
            )
            .run(stderr),
        );

        let Some(status) = self.wait_or_kill(&mut child).await? else {
            return Ok(None);
        };
        let _ = tokio::join!(pump_out, pump_err);
        Ok(Some(Finish {
            status,
            stderr_tail: tail(&tail_lines(&stderr_tail)),
        }))
    }

    /// The exit status, or `None` when the task was cancelled first and the
    /// child killed.
    async fn wait_or_kill(
        &mut self,
        child: &mut tokio::process::Child,
    ) -> Result<Option<ExitStatus>> {
        let waited = tokio::select! {
            status = child.wait() => Some(status),
            _ = &mut self.cancel => None,
        };
        let Some(status) = waited else {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(None);
        };
        Ok(Some(status?))
    }

    fn pump(&self, stream: OutputStream, sinks: Sinks) -> Pump {
        Pump {
            ctx: self.ctx.clone(),
            task_id: self.task.id.clone(),
            stream,
            sinks,
        }
    }

    fn command(&self, path: &Path, opts: &RunOpts<'_>) -> Command {
        let mut cmd = Command::new(path);
        match self.task.spec.executor {
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
        cmd.current_dir(self.worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }
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
