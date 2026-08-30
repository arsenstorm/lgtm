//! The `lgtm` command line, as clap sees it.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use lgtm_orchestrator::Choice;
use lgtm_protocol::{DependsOn, Executor, SandboxProfile, TaskId};

#[derive(Parser)]
#[command(name = "lgtm", version)]
pub struct Cli {
    #[arg(
        long,
        env = "LGTM_ORCHESTRATOR",
        default_value = "http://127.0.0.1:4750",
        global = true
    )]
    pub orchestrator: String,
    /// Required by every subcommand; checked manually so the missing-token
    /// message can be a plain sentence instead of clap's usage dump.
    #[arg(long, env = "LGTM_TOKEN", global = true)]
    pub token: Option<String>,
    /// Extra PEM root certificate to trust, for an orchestrator serving a
    /// self-signed TLS cert.
    #[arg(long, env = "LGTM_CA", global = true)]
    pub ca: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the orchestrator (and a local runner) on this machine
    Serve(ServeArgs),
    /// Join this machine to an orchestrator as a runner.
    #[command(alias = "worker")]
    Runner(RunnerArgs),
    /// List connected runners
    #[command(alias = "workers")]
    Runners,
    /// Run a prompt as a task and stream its output
    Run {
        #[command(flatten)]
        target: Target,
        /// GitHub issue to work from, e.g. an issue URL or `owner/repo#123`.
        #[arg(long)]
        issue: Option<String>,
        /// Linear issue to work from, e.g. `ENG-123` or a linear.app issue URL.
        #[arg(long)]
        linear: Option<String>,
        prompt: Option<String>,
    },
    /// Have the agent read the repository and propose a plan (a set of
    /// dependent steps) instead of making a change.
    Plan {
        #[command(flatten)]
        target: Target,
        goal: String,
    },
    /// State an outcome to work toward, and run its first task
    Goal {
        #[command(flatten)]
        target: Target,
        /// Propose a plan first instead of running the objective as one task.
        #[arg(long)]
        plan: bool,
        objective: String,
    },
    /// List goals
    Goals,
    /// List tasks
    Tasks,
    /// Throughput, duration, and cost over a window of created tasks
    Stats {
        /// Window in days, default 7.
        #[arg(long, default_value_t = 7)]
        days: u32,
    },
    /// Print a task, its events, checks, and review
    Show { id: String },
    /// List a plan's versions: a goal's plan tasks, or one plan task.
    Plans {
        /// A goal id or a plan task id.
        id: String,
    },
    /// Print a task's rendered output
    Logs { id: String },
    /// Print a task's diff
    Diff { id: String },
    /// Answer why a commit was introduced, from LGTM's own records
    Why { sha: String },
    /// Approve a task: push its branch, or create a plan's tasks
    Approve { id: String },
    /// Reject a task and discard its branch
    Reject { id: String },
    /// Cancel a queued or running task
    Cancel { id: String },
    /// Merge a task's pull request
    Merge { id: String },
    /// Queue a task that ended badly as a fresh attempt, then stream it
    Retry {
        id: String,
        /// Runner to run it on this time.
        #[arg(long)]
        on: Option<String>,
        /// Executor to use this time.
        #[arg(long, value_parser = parse_executor)]
        agent: Option<Executor>,
    },
    /// Attach a shell to the task's worktree on its runner. Ctrl-] detaches
    /// and leaves the shell running.
    Terminal {
        id: String,
        /// Kill the task's shell instead of attaching to it.
        #[arg(long)]
        close: bool,
    },
    /// Send a follow-up to a task awaiting review, then resume streaming.
    Tell { id: String, message: String },
    /// Grant a host an agent asked for; it applies to the task's next run.
    Allow { id: String, host: String },
    /// Show the working notes an agent kept for a task
    Pad {
        id: String,
        /// Replace the notes with this text (use - to read stdin).
        #[arg(long)]
        set: Option<String>,
    },
    /// Import a backlog of issues as tasks, or inspect a past import.
    Backlog {
        #[command(subcommand)]
        command: BacklogCommand,
    },
    /// Record, list, and forget facts every agent run is told.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Note work to do, and promote a note into a task when it's ready.
    Todo {
        #[command(subcommand)]
        command: TodoCommand,
    },
    /// Serve this task's memories, todos, and scratchpad to an agent over
    /// MCP on stdio. Started by the runner, not by a person.
    Mcp,
    /// Replace this binary with the latest release
    Upgrade {
        /// Install a specific release tag instead of the latest.
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Args)]
pub struct ServeArgs {
    #[arg(long, default_value = "0.0.0.0:4750")]
    pub bind: String,
    #[arg(long, env = "LGTM_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
    /// PEM certificate; requires `--tls-key` too. Plain HTTP if neither
    /// is given.
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,
    /// PEM private key; requires `--tls-cert` too.
    #[arg(long)]
    pub tls_key: Option<PathBuf>,
    /// Command that brings up an ephemeral runner when the queue needs
    /// one, run through `sh -c`.
    #[arg(long, env = "LGTM_PROVISION")]
    pub provision: Option<String>,
    /// Ceiling on connected ephemeral runners.
    #[arg(long, default_value_t = 4)]
    pub provision_max: u32,
    /// Where a provisioned runner should connect back to. Defaults to
    /// this orchestrator's bind address.
    #[arg(long, env = "LGTM_PUBLIC_URL")]
    pub public_url: Option<String>,
    /// Don't run a runner inside this process. Tasks then only run on
    /// machines that joined with `lgtm runner`.
    #[arg(long, alias = "no-worker")]
    pub no_runner: bool,
    /// POST every event a person would want to see to this URL.
    #[arg(long, env = "LGTM_WEBHOOK")]
    pub webhook: Option<String>,
    /// Let this model drive a goal each time one of its tasks ends:
    /// `claude`, `codex`, or `auto` for whichever this machine has. Off when
    /// not given.
    #[arg(long, env = "LGTM_ORCHESTRATE", value_parser = parse_orchestrate)]
    pub orchestrate: Option<Choice>,
    /// Model to run a task of a kind on when its spec names none, e.g.
    /// `plan=opus`. Repeatable. `LGTM_MODELS="plan=opus,run=sonnet"` when
    /// the flag is absent.
    #[arg(long = "model-for", value_delimiter = ',')]
    pub model_for: Vec<String>,
}

#[derive(Args)]
pub struct RunnerArgs {
    /// Orchestrator WebSocket base, ws:// or wss://. An http(s) URL is
    /// accepted and converted.
    pub url: String,
    #[arg(long, env = "LGTM_RUNNER_NAME")]
    pub name: Option<String>,
    /// Maximum tasks to run at once.
    #[arg(long, env = "LGTM_SLOTS")]
    pub slots: Option<u32>,
    /// Exit once `--max-tasks` runs have ended; for disposable machines.
    #[arg(long, env = "LGTM_EPHEMERAL")]
    pub ephemeral: bool,
    /// Runs to accept before exiting. Only read with `--ephemeral`.
    #[arg(long, env = "LGTM_MAX_TASKS", default_value_t = 1)]
    pub max_tasks: u32,
    /// Where mirrors and worktrees live.
    #[arg(long, env = "LGTM_DATA_DIR")]
    pub data_dir: Option<PathBuf>,
}

/// Where a task runs and on what: shared by `run` and `plan`.
#[derive(Args)]
pub struct Target {
    #[arg(long)]
    pub on: Option<String>,
    #[arg(long)]
    pub repo: Option<String>,
    #[arg(long, default_value = "main")]
    pub base: String,
    #[arg(long, default_value = "claude", value_parser = parse_executor)]
    pub agent: Executor,
    /// off, standard, or strict; defaults to the repository's config.
    #[arg(long, value_parser = parse_sandbox)]
    pub sandbox: Option<SandboxProfile>,
    /// A capability the runner must have, e.g. docker or os:windows. Repeatable.
    #[arg(long = "require")]
    pub requirements: Vec<String>,
    /// Harness for the review pass; defaults to the repository's `[policy]
    /// review_executor`, then auto.
    #[arg(long, value_parser = parse_executor)]
    pub review_with: Option<Executor>,
    /// Model for the harness to run, e.g. opus or gpt-5-codex. Defaults to
    /// the harness's own default.
    #[arg(long)]
    pub model: Option<String>,
    /// A task id this one must wait on. Repeatable.
    #[arg(long = "after")]
    pub after: Vec<TaskId>,
    /// What every `--after` id must have reached before this task starts.
    #[arg(long, default_value = "approved", value_parser = parse_depends_on)]
    pub after_condition: DependsOn,
    /// Chat session to file this task under.
    #[arg(long)]
    pub session: Option<String>,
}

#[derive(Subcommand)]
pub enum BacklogCommand {
    /// Import every open issue labeled `--label` in a GitHub repository.
    Github {
        /// `OWNER/REPO`.
        repo: String,
        #[arg(long)]
        label: String,
        #[command(flatten)]
        flags: BatchFlags,
    },
    /// Import every issue in the named Linear team and workflow state.
    Linear {
        #[arg(long)]
        team: String,
        #[arg(long)]
        state: String,
        /// Git URL the tasks clone from. Required: unlike `run --linear`,
        /// there is no origin-remote fallback.
        #[arg(long)]
        repo: String,
        #[command(flatten)]
        flags: BatchFlags,
    },
    /// List every batch imported so far.
    List,
    /// Show one batch's summary and its tasks.
    Status { id: String },
}

#[derive(Subcommand)]
pub enum MemoryCommand {
    /// Record a fact every agent run in the repository is told.
    Add {
        /// Git URL; omit for every repository.
        #[arg(long)]
        repo: Option<String>,
        content: String,
    },
    /// List recorded memories.
    List {
        #[arg(long)]
        repo: Option<String>,
        /// Only proposals still awaiting approval.
        #[arg(long)]
        pending: bool,
    },
    /// Approve a memory an agent proposed, so later runs are told it.
    Approve { id: String },
    /// Forget one memory.
    Rm { id: String },
}

#[derive(Subcommand)]
pub enum TodoCommand {
    /// Note work to do.
    Add {
        /// Git URL; omit for one not tied to a repository.
        #[arg(long)]
        repo: Option<String>,
        title: String,
        /// Longer text below the title.
        #[arg(long)]
        description: Option<String>,
    },
    /// List todos.
    List {
        #[arg(long)]
        repo: Option<String>,
    },
    /// Mark a todo done.
    Done { id: String },
    /// Turn a todo into a task and stream its output.
    Promote {
        id: String,
        #[command(flatten)]
        target: Target,
    },
    /// Delete a todo.
    Rm { id: String },
}

/// How a batch is imported, whichever source it comes from.
#[derive(Args)]
pub struct BatchFlags {
    /// Have the agent propose a plan for each issue instead of a diff.
    #[arg(long)]
    pub plan: bool,
    /// Approve plan tasks in this batch without a person.
    #[arg(long)]
    pub approve_plans: bool,
    #[arg(long, default_value_t = 20)]
    pub max: u32,
    /// List the matching issues without creating a batch.
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long, default_value = "main")]
    pub base: String,
    #[arg(long, default_value = "claude", value_parser = parse_executor)]
    pub agent: Executor,
    #[arg(long)]
    pub on: Option<String>,
    /// off, standard, or strict; defaults to the repository's config.
    #[arg(long, value_parser = parse_sandbox)]
    pub sandbox: Option<SandboxProfile>,
    /// A capability the runner must have, e.g. docker or os:windows. Repeatable.
    #[arg(long = "require")]
    pub requirements: Vec<String>,
    /// Harness for the review pass; defaults to the repository's `[policy]
    /// review_executor`, then auto.
    #[arg(long, value_parser = parse_executor)]
    pub review_with: Option<Executor>,
    /// Model for the harness to run, e.g. opus or gpt-5-codex. Defaults to
    /// the harness's own default.
    #[arg(long)]
    pub model: Option<String>,
}

fn parse_orchestrate(s: &str) -> Result<Choice, String> {
    match s {
        "auto" => Ok(Choice::Auto),
        other => parse_executor(other).map(Choice::One),
    }
}

fn parse_executor(s: &str) -> Result<Executor, String> {
    match s {
        "claude" => Ok(Executor::Claude),
        "codex" => Ok(Executor::Codex),
        other => Err(format!(
            "invalid agent '{other}', expected 'claude' or 'codex'"
        )),
    }
}

fn parse_sandbox(s: &str) -> Result<SandboxProfile, String> {
    SandboxProfile::parse(s).ok_or_else(|| {
        format!("invalid sandbox '{s}', expected 'off', 'standard', 'strict' or 'custom'")
    })
}

fn parse_depends_on(s: &str) -> Result<DependsOn, String> {
    match s {
        "approved" => Ok(DependsOn::Approved),
        "completed" => Ok(DependsOn::Completed),
        "merged" => Ok(DependsOn::Merged),
        other => Err(format!(
            "invalid after-condition '{other}', expected 'approved', 'completed' or 'merged'"
        )),
    }
}
