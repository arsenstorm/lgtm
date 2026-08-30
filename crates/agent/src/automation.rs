//! One task's agent work: the run and its retries, the fix-the-checks loop,
//! and the review. Every run of the executor for a task goes through here, so
//! one cost total and one cancel receiver cover all of them.

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lgtm_protocol::{
    Executor, OutputStream, Policy, Review, SandboxProfile, Task, TaskEvent, TaskKind, TaskResult,
    ValidationResult,
};
use tokio::process::Command;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::connection::Ctx;
use crate::git::{branch_name, commit, mirror_path, session_path, SCRATCHPAD};
use crate::plan::extract_plan;
use crate::policy::{
    effective_sandbox, failed_names, fix_prompt, load_policy, parse_review, review_prompt,
    review_warning, PolicyConfig,
};
use crate::proc::{
    cost_buffer, cost_total, final_text, tail_buffer, tail_lines, text_buffer, Cost, Pump, Sinks,
    Tail, Text,
};
use crate::sandbox;
use crate::validate::{load_validation, run_validation, tail};

/// Commit subject for the follow-up run that fixed the checks.
const FIX_MESSAGE: &str = "fix failing checks";

/// Told to a `Run` as the last paragraph of its prompt.
const NOTES: &str = "\n\nKeep your working notes in .lgtm/scratchpad.md: findings, open questions, decisions, and the files that matter. Whoever continues this task reads that file first.";

/// The notes travel over the socket and are stored with the task, so a file
/// that ran away is truncated rather than carried.
const NOTES_MAX: usize = 64 * 1024;

/// A plan produces no diff and no follow-up runs, so it has nothing to hand on.
pub fn with_notes(prompt: &str, kind: TaskKind) -> String {
    match kind {
        TaskKind::Plan => prompt.to_string(),
        TaskKind::Run => format!("{prompt}{NOTES}"),
    }
}

/// Puts the task's notes back in the worktree, so a retry on another worker
/// starts from what the last run wrote down.
pub async fn restore_notes(worktree: &Path, notes: &str) -> Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    let path = worktree.join(SCRATCHPAD);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(&path, notes)
        .await
        .with_context(|| format!("write {}", path.display()))
}

/// What one spawn of the executor is asked to do.
struct RunOpts<'a> {
    prompt: &'a str,
    /// Agent session to continue, when the run belongs to an earlier one.
    resume: Option<String>,
    /// Whether the run may change files; the reviewer must not.
    edits: bool,
    /// Set to collect the agent's final answer.
    answer: Option<Text>,
    /// Where the run's session id replaces the task's recorded one.
    session: Option<PathBuf>,
}

/// A finished run.
struct Finish {
    status: ExitStatus,
    stderr_tail: String,
}

/// How waiting on the executor ended.
enum Waited {
    Exited(ExitStatus),
    Cancelled,
    TimedOut,
}

/// `Waited` once the run's output has been collected. Both stop the task:
/// a cancel still needs its event, a timeout has already sent one.
enum Ran {
    Finished(Finish),
    Cancelled,
    TimedOut,
}

/// One task's worktree, cancel receiver, and cost total, shared by every run
/// of the executor for that task.
pub struct Run<'a> {
    task: &'a Task,
    worktree: &'a Path,
    ctx: &'a Arc<Ctx>,
    cancel: oneshot::Receiver<()>,
    cost: Cost,
    /// The repository's `timeout_secs`, known only once `execute` has read it.
    timeout: Duration,
    /// The profile every run of this task is confined by, known at the same
    /// point as the timeout.
    sandbox: SandboxProfile,
    /// The notes as last seen, so an unchanged scratchpad sends nothing.
    notes: String,
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
            timeout: Duration::from_secs(3600),
            sandbox: SandboxProfile::default(),
            notes: task.scratchpad.clone(),
        }
    }
}

/// The agent run for a task, and everything the repository's policy adds to it.
pub async fn execute(mut run: Run<'_>, prompt: &str, resume: Option<String>) -> Result<()> {
    let policy = load_policy(run.worktree);
    run.timeout = Duration::from_secs(policy.timeout_secs);
    run.sandbox = effective_sandbox(&run.task.spec, &policy);
    tracing::info!(profile = run.sandbox.as_str(), "sandbox profile");
    if run.task.spec.kind == TaskKind::Plan {
        return run.plan(prompt, &policy).await;
    }
    let finish = match run.attempts(prompt, resume, policy.retry).await? {
        Ran::Finished(finish) => finish,
        stop => return run.stopped(stop),
    };
    if !finish.status.success() {
        return run.failed(&finish);
    }
    let mut result = match run.commit_and_fix(prompt, policy.fix_checks).await? {
        Ok(result) => result,
        Err(stop) => return run.stopped(stop),
    };
    if policy.review && !result.diff.is_empty() {
        match run.review(&result.diff).await? {
            Ok(review) => result.review = Some(review),
            Err(stop) => return run.stopped(stop),
        }
    }
    result.policy = Some(policy_of(&policy));
    result.cost_usd = cost_total(&run.cost);
    run.ctx.emit(&run.task.id, TaskEvent::Completed { result });
    Ok(())
}

impl Run<'_> {
    /// Ends the task after a run that produced no exit status.
    fn stopped(&self, stop: Ran) -> Result<()> {
        if matches!(stop, Ran::Cancelled) {
            self.ctx.emit(&self.task.id, TaskEvent::Cancelled);
        }
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
    async fn plan(&mut self, prompt: &str, policy: &PolicyConfig) -> Result<()> {
        let answer = text_buffer();
        let opts = RunOpts {
            prompt,
            resume: None,
            edits: false,
            answer: Some(answer.clone()),
            session: Some(self.session_path()),
        };
        let finish = match self.agent_run(opts).await? {
            Ran::Finished(finish) => finish,
            stop => return self.stopped(stop),
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
    ) -> Result<Ran> {
        let mut attempt = 0;
        loop {
            let opts = RunOpts {
                prompt,
                resume: session.take(),
                edits: true,
                answer: None,
                session: Some(self.session_path()),
            };
            let finish = match self.agent_run(opts).await? {
                Ran::Finished(finish) => finish,
                stop => return Ok(stop),
            };
            if finish.status.success() || attempt >= retries {
                return Ok(Ran::Finished(finish));
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
    ) -> Result<Result<TaskResult, Ran>> {
        let base = &self.task.spec.base_branch;
        let branch = self.branch();
        let mut result = commit(prompt, base, &branch, self.worktree).await?;
        let checks = load_validation(self.worktree);
        result.validation = self.validate(&checks).await;

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
                edits: true,
                answer: None,
                session: Some(self.session_path()),
            };
            match self.agent_run(opts).await? {
                Ran::Finished(_) => {}
                stop => return Ok(Err(stop)),
            }
            // Whatever the fix run exited with, judge it by the checks themselves.
            result = commit(FIX_MESSAGE, base, &branch, self.worktree).await?;
            result.validation = self.validate(&checks).await;
        }
        Ok(Ok(result))
    }

    /// Announces the checks before running them, so a reader knows why the
    /// task went quiet. Silent when the repository declares none.
    async fn validate(&self, checks: &[(String, String)]) -> Vec<ValidationResult> {
        let names: Vec<String> = checks.iter().map(|(name, _)| name.clone()).collect();
        if !names.is_empty() {
            self.ctx
                .emit(&self.task.id, TaskEvent::Validating { names });
        }
        run_validation(self.worktree, checks).await
    }

    async fn review(&mut self, diff: &str) -> Result<Result<Review, Ran>> {
        let answer = text_buffer();
        let opts = RunOpts {
            prompt: &review_prompt(&self.task.spec.prompt, diff),
            resume: None,
            edits: false,
            answer: Some(answer.clone()),
            session: None,
        };
        let finish = match self.agent_run(opts).await? {
            Ran::Finished(finish) => finish,
            stop => return Ok(Err(stop)),
        };
        Ok(Ok(if finish.status.success() {
            parse_review(&final_text(&answer))
        } else {
            review_warning(format!("reviewer exited with {}", finish.status))
        }))
    }

    /// Spawns the executor, forwards its output, and waits. A `Ran` that is not
    /// `Finished` means the child was killed and nothing else has happened.
    async fn agent_run(&mut self, opts: RunOpts<'_>) -> Result<Ran> {
        let path = which::which(self.binary())
            .with_context(|| format!("{} not found on PATH", self.binary()))?;
        let mut child = self
            .command(&path, &opts)
            .spawn()
            .with_context(|| format!("spawn {}", path.display()))?;
        self.ctx.emit(&self.task.id, TaskEvent::Started);

        let stderr_tail = tail_buffer();
        let (pump_out, pump_err) = self.spawn_pumps(&mut child, opts, &stderr_tail)?;
        let waited = self.wait_or_kill(&mut child).await?;
        // Also for a cancel or a timeout: that is when the notes matter most.
        self.after_run().await;
        let status = match waited {
            Waited::Exited(status) => status,
            Waited::Cancelled => return Ok(Ran::Cancelled),
            Waited::TimedOut => return Ok(Ran::TimedOut),
        };
        let _ = tokio::join!(pump_out, pump_err);
        Ok(Ran::Finished(Finish {
            status,
            stderr_tail: tail(&tail_lines(&stderr_tail)),
        }))
    }

    /// Publishes the scratchpad the run left behind. A run that wrote nothing
    /// leaves the notes the task already carries standing.
    async fn after_run(&mut self) {
        let Ok(content) = tokio::fs::read_to_string(self.worktree.join(SCRATCHPAD)).await else {
            return;
        };
        let content = capped(&content);
        if content.is_empty() || content == self.notes {
            return;
        }
        self.notes = content.clone();
        self.ctx
            .emit(&self.task.id, TaskEvent::Scratchpad { content });
    }

    /// Forwards stdout and stderr; stdout also feeds the answer, cost, and
    /// session sinks the run asked for, stderr the tail kept for failures.
    fn spawn_pumps(
        &self,
        child: &mut tokio::process::Child,
        opts: RunOpts<'_>,
        stderr_tail: &Tail,
    ) -> Result<(JoinHandle<()>, JoinHandle<()>)> {
        let stdout = child.stdout.take().context("no stdout")?;
        let stderr = child.stderr.take().context("no stderr")?;
        let out = self.pump(
            OutputStream::Stdout,
            Sinks {
                session: opts.session,
                text: opts.answer,
                cost: Some(self.cost.clone()),
                ..Sinks::default()
            },
        );
        let err = self.pump(
            OutputStream::Stderr,
            Sinks {
                tail: Some(stderr_tail.clone()),
                ..Sinks::default()
            },
        );
        Ok((tokio::spawn(out.run(stdout)), tokio::spawn(err.run(stderr))))
    }

    /// The exit status, or why there is none: the task was cancelled, or the
    /// run outlived the policy's timeout. Either way the child is killed.
    async fn wait_or_kill(&mut self, child: &mut tokio::process::Child) -> Result<Waited> {
        let waited = tokio::select! {
            status = child.wait() => return Ok(Waited::Exited(status?)),
            _ = &mut self.cancel => Waited::Cancelled,
            _ = tokio::time::sleep(self.timeout) => Waited::TimedOut,
        };
        let _ = child.start_kill();
        let _ = child.wait().await;
        if matches!(waited, Waited::TimedOut) {
            let secs = self.timeout.as_secs();
            self.ctx.emit(&self.task.id, TaskEvent::TimedOut { secs });
        }
        Ok(waited)
    }

    fn pump(&self, stream: OutputStream, sinks: Sinks) -> Pump {
        Pump {
            ctx: self.ctx.clone(),
            task_id: self.task.id.clone(),
            stream,
            executor: self.task.spec.executor,
            sinks,
        }
    }

    fn command(&self, path: &Path, opts: &RunOpts<'_>) -> Command {
        let mirror = mirror_path(&self.ctx.data_dir, &self.task.spec.repository);
        let home = sandbox::home_dir(self.worktree);
        let paths = sandbox::Paths {
            worktree: self.worktree,
            mirror: &mirror,
            home: &home,
        };
        let wrapped = sandbox::wrap(self.sandbox, &paths, path, &self.args(opts));
        let mut cmd = Command::new(wrapped.program);
        cmd.args(wrapped.args);
        if self.sandbox != SandboxProfile::Off {
            cmd.env_clear()
                .envs(std::env::vars().filter(|(name, _)| sandbox::keep_env(name)));
        }
        cmd.current_dir(self.worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }

    fn args(&self, opts: &RunOpts<'_>) -> Vec<String> {
        match self.task.spec.executor {
            Executor::Claude => claude_args(opts),
            Executor::Codex => codex_args(opts),
        }
    }
}

fn capped(content: &str) -> String {
    let mut end = NOTES_MAX.min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_string()
}

fn claude_args(opts: &RunOpts<'_>) -> Vec<String> {
    let mut args = vec!["-p".to_string(), opts.prompt.to_string()];
    if let Some(session) = opts.resume.as_ref() {
        args.extend(["--resume".to_string(), session.clone()]);
    }
    args.extend([
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--permission-mode".to_string(),
        if opts.edits { "acceptEdits" } else { "default" }.to_string(),
    ]);
    args
}

/// `codex exec resume` takes no `--sandbox`, so the mode goes through `-c`,
/// which both forms accept. Codex has no `--full-auto` any more; the editing
/// mode it stood for is `workspace-write`.
fn codex_args(opts: &RunOpts<'_>) -> Vec<String> {
    let mut args = vec!["exec".to_string()];
    if let Some(session) = opts.resume.as_ref() {
        args.extend(["resume".to_string(), session.clone()]);
    }
    let mode = if opts.edits {
        "workspace-write"
    } else {
        "read-only"
    };
    args.extend([
        "--json".to_string(),
        "-c".to_string(),
        format!("sandbox_mode=\"{mode}\""),
        opts.prompt.to_string(),
    ]);
    args
}

fn policy_of(policy: &PolicyConfig) -> Policy {
    Policy {
        auto_approve: policy.auto_approve,
        auto_merge: policy.auto_merge,
        max_diff_lines: policy.max_diff_lines,
        protected_files: policy.protected_files.clone(),
        budget_per_task_usd: policy.budget_per_task_usd,
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
fn planned(branch: &str, text: &str, policy: &PolicyConfig, cost_usd: f64) -> TaskEvent {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_run_is_told_about_the_scratchpad() {
        let run = with_notes("do the thing", TaskKind::Run);
        assert!(run.starts_with("do the thing\n\nKeep your working notes"));
        assert!(run.contains(".lgtm/scratchpad.md"));
        assert_eq!(with_notes("do the thing", TaskKind::Plan), "do the thing");
    }

    #[test]
    fn notes_are_capped_on_a_char_boundary() {
        assert_eq!(capped("short"), "short");
        let long = "é".repeat(NOTES_MAX);
        assert_eq!(capped(&long), "é".repeat(NOTES_MAX / 2));
    }

    fn opts(resume: Option<&str>, edits: bool) -> RunOpts<'static> {
        RunOpts {
            prompt: "do the thing",
            resume: resume.map(str::to_string),
            edits,
            answer: None,
            session: None,
        }
    }

    #[test]
    fn a_fresh_codex_run_is_json_and_sandboxed_by_its_edit_rights() {
        assert_eq!(
            codex_args(&opts(None, true)),
            [
                "exec",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "do the thing"
            ]
        );
        assert_eq!(
            codex_args(&opts(None, false))[3],
            "sandbox_mode=\"read-only\""
        );
    }

    #[test]
    fn a_codex_follow_up_resumes_the_thread() {
        assert_eq!(
            codex_args(&opts(Some("01a04eb1"), true)),
            [
                "exec",
                "resume",
                "01a04eb1",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "do the thing"
            ]
        );
    }

    #[test]
    fn claude_edit_rights_pick_the_permission_mode() {
        let editing = claude_args(&opts(Some("abc-123"), true));
        assert!(editing.ends_with(&["--permission-mode".to_string(), "acceptEdits".to_string()]));
        assert!(editing[2..4] == ["--resume".to_string(), "abc-123".to_string()]);
        let reviewing = claude_args(&opts(None, false));
        assert!(reviewing.ends_with(&["--permission-mode".to_string(), "default".to_string()]));
    }
}
