//! One task's agent work: the run and its retries, the fix-the-checks loop,
//! and the review. Every run of the executor for a task goes through here, so
//! one cost total and one cancel receiver cover all of them.

use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lgtm_protocol::{
    Authorship, Executor, OutputStream, Policy, ReasoningEffort, Review, SandboxProfile, SkillRef,
    Task, TaskEvent, TaskKind, TaskResult, ValidationResult,
};
use serde_json::json;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::artefacts;
use crate::connection::Ctx;
use crate::git::{base64_encode, branch_name, commit, mirror_path, session_path, SCRATCHPAD};
use crate::plan::extract_plan;
use crate::policy::{
    effective_sandbox, failed_names, fix_prompt, load_policy, parse_review, review_prompt,
    review_warning, reviewer, CustomPaths, Limits, NetworkPolicy, PolicyConfig,
};
use crate::proc::{
    cost_buffer, cost_total, final_text, tail_buffer, tail_lines, text_buffer, Cost, Pump, Sinks,
    Tail, Text,
};
use crate::proxy;
use crate::sandbox::{self, Network};
use crate::validate::{load_validation, run_validation, tail};

/// Commit subject for the follow-up run that fixed the checks.
const FIX_MESSAGE: &str = "fix failing checks";

/// Time an interrupted agent gets to exit on its own before the kill a plain
/// cancel goes straight to.
const INTERRUPT_GRACE: Duration = Duration::from_secs(10);

/// Told to a `Run` as the last paragraph of its prompt.
const NOTES: &str = "\n\nKeep your working notes in .lgtm/scratchpad.md, or with the `scratchpad_write` tool: findings, open questions, decisions, and the files that matter. Whoever continues this task reads them first. Use `memory_propose` for a fact the next run should know, and `todo_create` for work you noticed but did not do. Use `skill_propose` for a procedure worth reusing, as a whole SKILL.md. Put screenshots or generated files for the reviewer in .lgtm/artefacts/.";

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

/// Puts the task's notes back in the worktree, so a retry on another runner
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
    /// The harness for this spawn: the task's own for every pass but review,
    /// which may run under the other one.
    executor: Executor,
    /// `None` runs the harness's own default model.
    model: Option<&'a str>,
    /// `None` runs the model's own default reasoning level.
    reasoning_effort: Option<ReasoningEffort>,
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
    /// The hosts an allowlisted run may reach, `None` when the repository
    /// asked for no allowlist.
    allowed_hosts: Option<Vec<String>>,
    /// What the sandbox enforces for the run that is about to start: a port
    /// only once that run's proxy is listening.
    network: Network,
    /// What the run may consume, known with the timeout and the profile.
    limits: Limits,
    /// The repository's `[sandbox]` path exceptions; only applied when
    /// `sandbox` is `Custom`.
    paths: CustomPaths,
    /// The notes as last seen, so an unchanged scratchpad sends nothing.
    notes: String,
    /// Name and content hash of every artefact sent, so a file that survives
    /// a follow-up run is not sent again.
    artefacts: Vec<(String, u64)>,
    /// Whose names go on the commit; the orchestrator resolved them, because
    /// the credentials they come from never leave it.
    authorship: Authorship,
    /// What was put in the worktree for this task, reported on every `Started`.
    pub(crate) skills: Vec<SkillRef>,
}

/// The allowlist proxy serving one run: the task that accepts on it, and the
/// hosts it refused.
struct Proxy {
    task: JoinHandle<()>,
    denied: mpsc::UnboundedReceiver<String>,
}

impl<'a> Run<'a> {
    pub fn new(
        task: &'a Task,
        worktree: &'a Path,
        ctx: &'a Arc<Ctx>,
        cancel: oneshot::Receiver<()>,
        authorship: Authorship,
    ) -> Self {
        Run {
            task,
            worktree,
            ctx,
            cancel,
            cost: cost_buffer(),
            timeout: Duration::from_secs(3600),
            sandbox: SandboxProfile::default(),
            allowed_hosts: None,
            network: Network::Unrestricted,
            limits: Limits::default(),
            paths: CustomPaths::default(),
            notes: task.scratchpad.clone(),
            artefacts: Vec::new(),
            authorship,
            skills: Vec::new(),
        }
    }
}

/// The agent run for a task, and everything the repository's policy adds to it.
pub async fn execute(mut run: Run<'_>, prompt: &str, resume: Option<String>) -> Result<()> {
    let policy = load_policy(run.worktree);
    let available = crate::detect_executors();
    run.timeout = Duration::from_secs(policy.timeout_secs);
    run.sandbox = effective_sandbox(&run.task.spec, &policy);
    run.limits = policy.limits;
    run.paths = policy.paths.clone();
    // An allowlist stays blocked until its proxy is listening: a run that
    // cannot be restricted must not run unrestricted instead.
    (run.allowed_hosts, run.network) = match &policy.network {
        NetworkPolicy::Unrestricted => (None, Network::Unrestricted),
        NetworkPolicy::None => (None, Network::Blocked),
        NetworkPolicy::Allowlist(hosts) => (
            Some(with_task_hosts(hosts, &run.task.spec.allowed_hosts)),
            Network::Blocked,
        ),
    };
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
        match run.review(&result.diff, &policy, &available).await? {
            Ok(review) => result.review = Some(review),
            Err(stop) => return run.stopped(stop),
        }
    }
    result.policy = Some(policy_of(&policy));
    result.cost_usd = cost_total(&run.cost);
    run.ctx.emit(&run.task.id, TaskEvent::Completed { result });
    Ok(())
}

impl<'a> Run<'a> {
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

    // Tied to the task's own lifetime, not `&self`'s: `RunOpts` borrows it
    // across an `&mut self` call in `agent_run`.
    fn model(&self) -> Option<&'a str> {
        self.task.spec.model.as_deref()
    }

    fn reasoning_effort(&self) -> Option<ReasoningEffort> {
        self.task.spec.reasoning_effort
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
            executor: self.task.spec.executor,
            model: self.model(),
            reasoning_effort: self.reasoning_effort(),
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
                executor: self.task.spec.executor,
                model: self.model(),
                reasoning_effort: self.reasoning_effort(),
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
        let mut result = commit(prompt, base, &branch, self.worktree, &self.authorship).await?;
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
                executor: self.task.spec.executor,
                model: self.model(),
                reasoning_effort: self.reasoning_effort(),
            };
            match self.agent_run(opts).await? {
                Ran::Finished(_) => {}
                stop => return Ok(Err(stop)),
            }
            // Whatever the fix run exited with, judge it by the checks themselves.
            result = commit(FIX_MESSAGE, base, &branch, self.worktree, &self.authorship).await?;
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

    async fn review(
        &mut self,
        diff: &str,
        policy: &PolicyConfig,
        available: &[Executor],
    ) -> Result<Result<Review, Ran>> {
        let used = reviewer(&self.task.spec, policy, available);
        let uses_task_model = used == self.task.spec.executor;
        let answer = text_buffer();
        let opts = RunOpts {
            prompt: &review_prompt(&self.task.spec.prompt, diff),
            resume: None,
            edits: false,
            answer: Some(answer.clone()),
            session: None,
            executor: used,
            model: uses_task_model.then(|| self.model()).flatten(),
            reasoning_effort: uses_task_model.then(|| self.reasoning_effort()).flatten(),
        };
        let finish = match self.agent_run(opts).await? {
            Ran::Finished(finish) => finish,
            stop => return Ok(Err(stop)),
        };
        let mut review = if finish.status.success() {
            parse_review(&final_text(&answer))
        } else {
            review_warning(format!("reviewer exited with {}", finish.status))
        };
        review.executor = Some(used);
        Ok(Ok(review))
    }

    /// Spawns the executor, forwards its output, and waits. A `Ran` that is not
    /// `Finished` means the child was killed and nothing else has happened.
    async fn agent_run(&mut self, opts: RunOpts<'_>) -> Result<Ran> {
        let binary = opts.executor.binary();
        let path = which::which(binary).with_context(|| format!("{binary} not found on PATH"))?;
        let proxy = self.start_proxy().await;
        let mut child = self
            .command(&path, &opts)
            .spawn()
            .with_context(|| format!("spawn {}", path.display()))?;
        let _confined = sandbox::confine_child(&child, &self.limits);
        self.ctx.emit(
            &self.task.id,
            TaskEvent::Started {
                model: self.task.spec.model.clone(),
                skills: self.skills.clone(),
            },
        );

        let stderr_tail = tail_buffer();
        let (pump_out, pump_err) = self.spawn_pumps(&mut child, opts, &stderr_tail)?;
        let waited = self.wait_or_kill(&mut child).await?;
        // Also for a cancel or a timeout: that is when the notes matter most.
        self.after_run().await;
        self.send_artefacts().await;
        self.close_proxy(proxy);
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

    /// Each run gets its own proxy, so the port a run was told to use dies
    /// with it. A proxy that will not bind leaves the run blocked.
    async fn start_proxy(&mut self) -> Option<Proxy> {
        let hosts = self.allowed_hosts.clone()?;
        self.network = Network::Blocked;
        let (sender, denied) = mpsc::unbounded_channel();
        match proxy::serve(hosts, sender).await {
            Ok((addr, task)) => {
                self.network = Network::Proxy(addr.port());
                Some(Proxy { task, denied })
            }
            Err(err) => {
                tracing::warn!("network allowlist proxy: {err}; the run gets no network");
                None
            }
        }
    }

    /// One event per host, however many times the run tried it: the point is
    /// which hosts the allowlist is missing, not how often.
    fn close_proxy(&mut self, proxy: Option<Proxy>) {
        let Some(mut proxy) = proxy else {
            return;
        };
        proxy.task.abort();
        let mut seen: Vec<String> = Vec::new();
        while let Ok(host) = proxy.denied.try_recv() {
            if seen.contains(&host) {
                continue;
            }
            seen.push(host.clone());
            self.ctx
                .emit(&self.task.id, TaskEvent::NetworkDenied { host });
        }
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

    /// Publishes the files the run left for the reviewer.
    async fn send_artefacts(&mut self) {
        for (name, bytes) in artefacts::collect(self.worktree).await {
            if !artefacts::changed(&mut self.artefacts, &name, &bytes) {
                continue;
            }
            self.ctx.emit(
                &self.task.id,
                TaskEvent::Artefact {
                    size: bytes.len(),
                    bytes_base64: base64_encode(&bytes),
                    name,
                },
            );
        }
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
        let executor = opts.executor;
        let out = self.pump(
            OutputStream::Stdout,
            Sinks {
                session: opts.session,
                text: opts.answer,
                cost: Some(self.cost.clone()),
                ..Sinks::default()
            },
            executor,
        );
        let err = self.pump(
            OutputStream::Stderr,
            Sinks {
                tail: Some(stderr_tail.clone()),
                ..Sinks::default()
            },
            executor,
        );
        Ok((tokio::spawn(out.run(stdout)), tokio::spawn(err.run(stderr))))
    }

    /// The exit status, or why there is none: the task was cancelled or
    /// interrupted, or the run outlived the policy's timeout. Either way the
    /// child is killed, unless an interrupt already made it exit on its own.
    async fn wait_or_kill(&mut self, child: &mut tokio::process::Child) -> Result<Waited> {
        // Registered only for the span of this wait: an interrupt can only
        // ever reach a run that is actually being waited on.
        let (interrupt_tx, mut interrupt_rx) = oneshot::channel();
        self.ctx
            .interrupt
            .lock()
            .expect("interrupt map poisoned")
            .insert(self.task.id.clone(), interrupt_tx);
        let interrupted;
        let waited = tokio::select! {
            status = child.wait() => {
                self.clear_interrupt();
                return Ok(Waited::Exited(status?));
            }
            _ = &mut self.cancel => { interrupted = false; Waited::Cancelled }
            _ = &mut interrupt_rx => { interrupted = true; Waited::Cancelled }
            _ = tokio::time::sleep(self.timeout) => { interrupted = false; Waited::TimedOut }
        };
        self.clear_interrupt();
        if interrupted {
            send_sigint(child);
            // Grace period before the same kill a plain cancel goes straight to.
            if tokio::time::timeout(INTERRUPT_GRACE, child.wait())
                .await
                .is_ok()
            {
                return Ok(waited);
            }
        }
        let _ = child.start_kill();
        let _ = child.wait().await;
        if matches!(waited, Waited::TimedOut) {
            let secs = self.timeout.as_secs();
            self.ctx.emit(&self.task.id, TaskEvent::TimedOut { secs });
        }
        Ok(waited)
    }

    fn clear_interrupt(&self) {
        self.ctx
            .interrupt
            .lock()
            .expect("interrupt map poisoned")
            .remove(&self.task.id);
    }

    fn pump(&self, stream: OutputStream, sinks: Sinks, executor: Executor) -> Pump {
        Pump {
            ctx: self.ctx.clone(),
            task_id: self.task.id.clone(),
            stream,
            executor,
            sinks,
        }
    }

    fn wrapped(&self, path: &Path, opts: &RunOpts<'_>) -> sandbox::Wrapped {
        let mirror = mirror_path(&self.ctx.data_dir, &self.task.spec.repository);
        let home = sandbox::home_dir(self.worktree);
        let paths = sandbox::Paths {
            worktree: self.worktree,
            mirror: &mirror,
            home: &home,
        };
        sandbox::wrap(
            self.sandbox,
            &paths,
            self.network,
            &self.limits,
            &self.paths,
            path,
            &args(opts),
        )
    }

    fn command(&self, path: &Path, opts: &RunOpts<'_>) -> Command {
        let wrapped = self.wrapped(path, opts);
        let mut cmd = Command::new(wrapped.program);
        cmd.args(wrapped.args);
        if self.sandbox != SandboxProfile::Off {
            cmd.env_clear()
                .envs(std::env::vars().filter(|(name, _)| sandbox::keep_env(name)));
        }
        // After the inherited variables: the run's own proxy is not one the
        // runner's shell gets to override.
        cmd.envs(self.mcp_env())
            .envs(sandbox::network_env(self.network))
            .current_dir(self.worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }
}

impl Run<'_> {
    /// What `lgtm mcp` reads to answer for this run.
    fn mcp_env(&self) -> [(&'static str, String); 4] {
        [
            ("LGTM_ORCHESTRATOR", http_url(&self.ctx.orchestrator)),
            ("LGTM_TOKEN", self.ctx.token.clone()),
            ("LGTM_TASK_ID", self.task.id.clone()),
            ("LGTM_REPOSITORY", self.task.spec.repository.clone()),
        ]
    }
}

/// SIGINT to the child's process group, so its subprocesses see it too, not
/// just the immediate one. Windows has no such signal, so an interrupted run
/// there just falls straight through to the kill in `wait_or_kill`.
#[cfg(unix)]
fn send_sigint(child: &tokio::process::Child) {
    let Some(pid) = child.id() else { return };
    let _ = std::process::Command::new("kill")
        .args(["-INT", "--", &format!("-{pid}")])
        .status();
}

#[cfg(not(unix))]
fn send_sigint(_child: &tokio::process::Child) {}

fn args(opts: &RunOpts<'_>) -> Vec<String> {
    // A runner whose own path is unknowable can still run the agent; it
    // just runs it without the LGTM tools.
    let exe = std::env::current_exe().ok();
    match opts.executor {
        Executor::Claude => claude_args(opts, exe.as_deref()),
        Executor::Codex => codex_args(opts, exe.as_deref()),
    }
}

/// The runner dials the orchestrator over a WebSocket; the MCP server talks
/// to the same host over HTTP.
fn http_url(ws: &str) -> String {
    match ws.split_once("://") {
        Some(("ws", rest)) => format!("http://{rest}"),
        Some(("wss", rest)) => format!("https://{rest}"),
        _ => ws.to_string(),
    }
}

/// The repository's allowlist plus whatever a person has granted this task,
/// deduplicated: a host allowed twice must still be one proxy rule.
fn with_task_hosts(repo_hosts: &[String], task_hosts: &[String]) -> Vec<String> {
    let mut hosts = repo_hosts.to_vec();
    for host in task_hosts {
        if !hosts.contains(host) {
            hosts.push(host.clone());
        }
    }
    hosts
}

fn capped(content: &str) -> String {
    let mut end = NOTES_MAX.min(content.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_string()
}

fn claude_args(opts: &RunOpts<'_>, exe: Option<&Path>) -> Vec<String> {
    let mut args = vec!["-p".to_string(), opts.prompt.to_string()];
    if let Some(session) = opts.resume.as_ref() {
        args.extend(["--resume".to_string(), session.clone()]);
    }
    if let Some(model) = opts.model {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if let Some(effort) = opts.reasoning_effort {
        args.extend(["--effort".to_string(), effort.as_str().to_string()]);
    }
    args.extend([
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--permission-mode".to_string(),
        if opts.edits { "acceptEdits" } else { "default" }.to_string(),
    ]);
    if let Some(exe) = exe {
        args.extend([
            "--mcp-config".to_string(),
            mcp_config(exe),
            // Nothing prompts an unattended run, so the LGTM tools are
            // pre-approved; every other tool still goes by permission mode.
            "--allowedTools".to_string(),
            "mcp__lgtm__*".to_string(),
        ]);
    }
    args
}

/// The MCP server is this same binary: the runner and `lgtm` are one command.
fn mcp_config(exe: &Path) -> String {
    json!({ "mcpServers": { "lgtm": { "command": exe.display().to_string(), "args": ["mcp"] } } })
        .to_string()
}

/// `codex exec resume` takes no `--sandbox`, so the mode goes through `-c`,
/// which both forms accept. Codex has no `--full-auto` any more; the editing
/// mode it stood for is `workspace-write`.
fn codex_args(opts: &RunOpts<'_>, exe: Option<&Path>) -> Vec<String> {
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
    ]);
    if let Some(exe) = exe {
        args.extend([
            "-c".to_string(),
            format!("mcp_servers.lgtm.command=\"{}\"", exe.display()),
            "-c".to_string(),
            "mcp_servers.lgtm.args=[\"mcp\"]".to_string(),
        ]);
    }
    if let Some(model) = opts.model {
        args.extend(["-m".to_string(), model.to_string()]);
    }
    if let Some(effort) = opts.reasoning_effort {
        args.extend([
            "-c".to_string(),
            format!("model_reasoning_effort=\"{}\"", effort.as_str()),
        ]);
    }
    args.push(opts.prompt.to_string());
    args
}

fn policy_of(policy: &PolicyConfig) -> Policy {
    Policy {
        auto_approve: policy.auto_approve,
        auto_merge: policy.auto_merge,
        max_diff_lines: policy.max_diff_lines,
        protected_files: policy.protected_files.clone(),
        budget_per_task_usd: policy.budget_per_task_usd,
        reassign: policy.reassign,
        budget_daily_usd: policy.budget_daily_usd,
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
        assert!(run.contains("`scratchpad_write`"));
        assert!(run.contains("`memory_propose`"));
        assert!(run.contains("`todo_create`"));
        assert_eq!(with_notes("do the thing", TaskKind::Plan), "do the thing");
    }

    #[test]
    fn the_orchestrator_socket_becomes_an_http_url() {
        assert_eq!(http_url("ws://127.0.0.1:4750"), "http://127.0.0.1:4750");
        assert_eq!(http_url("wss://example.com"), "https://example.com");
        assert_eq!(http_url("http://example.com"), "http://example.com");
    }

    #[test]
    fn task_hosts_extend_the_repository_allowlist_without_duplicating() {
        let repo = vec!["github.com".to_string()];
        let task = vec!["github.com".to_string(), "registry.internal".to_string()];
        assert_eq!(
            with_task_hosts(&repo, &task),
            vec!["github.com".to_string(), "registry.internal".to_string()]
        );
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
            executor: Executor::Claude,
            model: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn a_fresh_codex_run_is_json_and_sandboxed_by_its_edit_rights() {
        assert_eq!(
            codex_args(&opts(None, true), None),
            [
                "exec",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "do the thing"
            ]
        );
        assert_eq!(
            codex_args(&opts(None, false), None)[3],
            "sandbox_mode=\"read-only\""
        );
    }

    #[test]
    fn a_codex_follow_up_resumes_the_thread() {
        assert_eq!(
            codex_args(&opts(Some("01a04eb1"), true), None),
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
        let editing = claude_args(&opts(Some("abc-123"), true), None);
        assert!(editing.ends_with(&["--permission-mode".to_string(), "acceptEdits".to_string()]));
        assert!(editing[2..4] == ["--resume".to_string(), "abc-123".to_string()]);
        let reviewing = claude_args(&opts(None, false), None);
        assert!(reviewing.ends_with(&["--permission-mode".to_string(), "default".to_string()]));
    }

    #[test]
    fn both_executors_register_this_binary_as_the_lgtm_mcp_server() {
        let exe = Path::new("/usr/local/bin/lgtm");
        let claude = claude_args(&opts(None, true), Some(exe));
        assert_eq!(claude[claude.len() - 4], "--mcp-config");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&claude[claude.len() - 3]).unwrap(),
            json!({"mcpServers": {"lgtm": {"command": "/usr/local/bin/lgtm", "args": ["mcp"]}}})
        );
        assert_eq!(
            &claude[claude.len() - 2..],
            ["--allowedTools", "mcp__lgtm__*"]
        );
        let codex = codex_args(&opts(None, true), Some(exe));
        assert_eq!(
            &codex[codex.len() - 5..],
            [
                "-c",
                "mcp_servers.lgtm.command=\"/usr/local/bin/lgtm\"",
                "-c",
                "mcp_servers.lgtm.args=[\"mcp\"]",
                "do the thing"
            ]
        );
    }

    #[test]
    fn a_requested_model_becomes_the_harness_flag() {
        let mut with_model = opts(None, true);
        with_model.model = Some("opus");
        assert!(claude_args(&with_model, None).contains(&"--model".to_string()));
        assert!(claude_args(&with_model, None).contains(&"opus".to_string()));
        assert_eq!(
            codex_args(&with_model, None),
            [
                "exec",
                "--json",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "-m",
                "opus",
                "do the thing"
            ]
        );
        assert!(!claude_args(&opts(None, true), None).contains(&"--model".to_string()));
        assert!(!codex_args(&opts(None, true), None).contains(&"-m".to_string()));
    }

    #[test]
    fn requested_reasoning_becomes_each_harness_configuration() {
        let mut with_reasoning = opts(None, true);
        with_reasoning.reasoning_effort = Some(ReasoningEffort::High);

        let claude = claude_args(&with_reasoning, None);
        assert!(claude.windows(2).any(|pair| pair == ["--effort", "high"]));

        let codex = codex_args(&with_reasoning, None);
        assert!(codex
            .windows(2)
            .any(|pair| pair == ["-c", "model_reasoning_effort=\"high\""]));
    }
}
