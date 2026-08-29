mod backlog;
mod config;
mod http;
mod render;
mod run;
mod upgrade;

use clap::{Parser, Subcommand};
use http::Client;
use lgtm_protocol::{
    BatchSource, CiState, Executor, Review, StoredEvent, Task, TaskKind, TaskStatus, WorkerStatus,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lgtm", version)]
struct Cli {
    #[arg(
        long,
        env = "LGTM_ORCHESTRATOR",
        default_value = "http://127.0.0.1:4750",
        global = true
    )]
    orchestrator: String,
    /// Required by every subcommand; checked manually so the missing-token
    /// message can be a plain sentence instead of clap's usage dump.
    #[arg(long, env = "LGTM_TOKEN", global = true)]
    token: Option<String>,
    /// Extra PEM root certificate to trust, for an orchestrator serving a
    /// self-signed TLS cert.
    #[arg(long, env = "LGTM_CA", global = true)]
    ca: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the orchestrator (and a local worker) on this machine
    Serve {
        #[arg(long, default_value = "0.0.0.0:4750")]
        bind: String,
        #[arg(long, env = "LGTM_DATA_DIR")]
        data_dir: Option<PathBuf>,
        /// PEM certificate; requires `--tls-key` too. Plain HTTP if neither
        /// is given.
        #[arg(long)]
        tls_cert: Option<PathBuf>,
        /// PEM private key; requires `--tls-cert` too.
        #[arg(long)]
        tls_key: Option<PathBuf>,
        /// Command that brings up an ephemeral worker when the queue needs
        /// one, run through `sh -c`.
        #[arg(long, env = "LGTM_PROVISION")]
        provision: Option<String>,
        /// Ceiling on connected ephemeral workers.
        #[arg(long, default_value_t = 4)]
        provision_max: u32,
        /// Where a provisioned worker should connect back to. Defaults to
        /// this orchestrator's bind address.
        #[arg(long, env = "LGTM_PUBLIC_URL")]
        public_url: Option<String>,
        /// Don't run a worker inside this process. Tasks then only run on
        /// machines that joined with `lgtm worker`.
        #[arg(long)]
        no_worker: bool,
    },
    /// Join this machine to an orchestrator as a worker.
    Worker {
        /// Orchestrator WebSocket base, ws:// or wss://. An http(s) URL is
        /// accepted and converted.
        url: String,
        #[arg(long, env = "LGTM_WORKER_NAME")]
        name: Option<String>,
        /// Maximum tasks to run at once.
        #[arg(long, env = "LGTM_SLOTS")]
        slots: Option<u32>,
        /// Exit once `--max-tasks` runs have ended; for disposable machines.
        #[arg(long, env = "LGTM_EPHEMERAL")]
        ephemeral: bool,
        /// Runs to accept before exiting. Only read with `--ephemeral`.
        #[arg(long, env = "LGTM_MAX_TASKS", default_value_t = 1)]
        max_tasks: u32,
        /// Where mirrors and worktrees live.
        #[arg(long, env = "LGTM_DATA_DIR")]
        data_dir: Option<PathBuf>,
    },
    /// List connected workers
    Workers,
    /// Run a prompt as a task and stream its output
    Run {
        #[arg(long)]
        on: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, default_value = "main")]
        base: String,
        #[arg(long, default_value = "claude", value_parser = parse_executor)]
        agent: Executor,
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
        #[arg(long)]
        on: Option<String>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, default_value = "main")]
        base: String,
        #[arg(long, default_value = "claude", value_parser = parse_executor)]
        agent: Executor,
        goal: String,
    },
    /// List tasks
    Tasks,
    /// Print a task, its events, checks, and review
    Show { id: String },
    /// Print a task's rendered output
    Logs { id: String },
    /// Print a task's diff
    Diff { id: String },
    /// Approve a task: push its branch, or create a plan's tasks
    Approve { id: String },
    /// Reject a task and discard its branch
    Reject { id: String },
    /// Cancel a queued or running task
    Cancel { id: String },
    /// Merge a task's pull request
    Merge { id: String },
    /// Send a follow-up to a task awaiting review, then resume streaming.
    Tell { id: String, message: String },
    /// Import a backlog of issues as tasks, or inspect a past import.
    Backlog {
        #[command(subcommand)]
        command: BacklogCommand,
    },
    /// Replace this binary with the latest release
    Upgrade {
        /// Install a specific release tag instead of the latest.
        #[arg(long)]
        version: Option<String>,
    },
}

#[derive(Subcommand)]
enum BacklogCommand {
    /// Import every open issue labeled `--label` in a GitHub repository.
    Github {
        /// `OWNER/REPO`.
        repo: String,
        #[arg(long)]
        label: String,
        /// Have the agent propose a plan for each issue instead of a diff.
        #[arg(long)]
        plan: bool,
        /// Approve plan tasks in this batch without a person.
        #[arg(long)]
        approve_plans: bool,
        #[arg(long, default_value_t = 20)]
        max: u32,
        /// List the matching issues without creating a batch.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "main")]
        base: String,
        #[arg(long, default_value = "claude", value_parser = parse_executor)]
        agent: Executor,
        #[arg(long)]
        on: Option<String>,
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
        /// Have the agent propose a plan for each issue instead of a diff.
        #[arg(long)]
        plan: bool,
        /// Approve plan tasks in this batch without a person.
        #[arg(long)]
        approve_plans: bool,
        #[arg(long, default_value_t = 20)]
        max: u32,
        /// List the matching issues without creating a batch.
        #[arg(long)]
        dry_run: bool,
        #[arg(long, default_value = "main")]
        base: String,
        #[arg(long, default_value = "claude", value_parser = parse_executor)]
        agent: Executor,
        #[arg(long)]
        on: Option<String>,
    },
    /// List every batch imported so far.
    List,
    /// Show one batch's summary and its tasks.
    Status { id: String },
}

/// Body of `GET /api/tasks/:id`.
#[derive(Deserialize)]
struct TaskDetail {
    task: Task,
    events: Vec<StoredEvent>,
}

/// Body of `POST /api/tasks/:id/message`.
#[derive(Serialize)]
struct FollowUp<'a> {
    text: &'a str,
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

/// The wire form ("awaiting_review") rather than Rust's Debug form, so it
/// matches the JSON everywhere else in the CLI's output.
fn status_str(status: TaskStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The wire form ("success") rather than Rust's Debug form, matching
/// `status_str` above.
fn ci_str(state: CiState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// `#<pr-number> <mark>` for the `tasks` table's PR column, empty when the
/// task has no pull request. Mark is ✓/✗ for CI success/failure, … for
/// pending or missing CI info.
pub(crate) fn pr_cell(task: &Task) -> String {
    let Some(pr) = &task.pull_request else {
        return String::new();
    };
    let mark = match task.ci.as_ref().map(|ci| ci.state) {
        Some(CiState::Success) => "✓",
        Some(CiState::Failure) => "✗",
        Some(CiState::Pending) | None => "…",
    };
    format!("#{} {mark}", pr.number)
}

/// Reorders `tasks` for the `tasks` table so each child (`spec.parent`
/// `Some`) is listed right after its parent. Top-level tasks (and parents)
/// keep their existing relative order; a task's own children keep their
/// existing relative order too. A task whose declared parent isn't in the
/// list is treated as top-level.
pub(crate) fn order_tasks(tasks: Vec<Task>) -> Vec<Task> {
    let ids: std::collections::HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let mut children: std::collections::HashMap<String, Vec<Task>> =
        std::collections::HashMap::new();
    let mut top_level: Vec<Task> = Vec::new();
    for task in tasks {
        match &task.spec.parent {
            Some(parent_id) if ids.contains(parent_id.as_str()) => {
                children.entry(parent_id.clone()).or_default().push(task);
            }
            _ => top_level.push(task),
        }
    }
    let mut ordered = Vec::with_capacity(top_level.len());
    for task in top_level {
        let id = task.id.clone();
        ordered.push(task);
        if let Some(kids) = children.remove(&id) {
            ordered.extend(kids);
        }
    }
    ordered
}

/// STATUS cell for the `tasks` table: `display_status` only, not the raw
/// wire status. A queued, unassigned task with unmet dependencies shows
/// `blocked` instead of `queued`, so the table hints at why it isn't
/// running yet.
pub(crate) fn display_status(task: &Task, all: &[Task]) -> String {
    let has_unmet_deps = !task.spec.depends_on.is_empty()
        && !task.spec.depends_on.iter().all(|dep_id| {
            all.iter().any(|t| {
                &t.id == dep_id && matches!(t.status, TaskStatus::Approved | TaskStatus::Merged)
            })
        });
    if task.status == TaskStatus::Queued && task.worker.is_none() && has_unmet_deps {
        "blocked".to_string()
    } else {
        status_str(task.status)
    }
}

/// Prints the `tasks`-style table: `ID STATUS WORKER PR PROMPT`, children
/// ordered after their parent. Shared by `tasks` and `backlog status`.
pub(crate) fn print_task_table(tasks: Vec<Task>) {
    println!(
        "{:<10}{:<16}{:<16}{:<10}PROMPT",
        "ID", "STATUS", "WORKER", "PR"
    );
    for t in order_tasks(tasks.clone()) {
        let worker = t.worker.as_deref().unwrap_or("-");
        let prefix = if t.spec.parent.is_some() { "↳ " } else { "" };
        let prompt = format!("{prefix}{}", first_line_truncated(&t.spec.prompt, 60));
        let failed = t.result.as_ref().is_some_and(|r| {
            r.validation_failed() || r.review.as_ref().is_some_and(Review::has_blocking)
        });
        let status = display_status(&t, &tasks);
        let status = if failed { format!("{status}!") } else { status };
        let pr = pr_cell(&t);
        println!(
            "{:<10}{:<16}{:<16}{:<10}{}",
            t.id, status, worker, pr, prompt
        );
    }
}

pub(crate) fn first_line_truncated(s: &str, max: usize) -> String {
    let first = s.lines().next().unwrap_or("");
    if first.chars().count() > max {
        first.chars().take(max).collect()
    } else {
        first.to_string()
    }
}

/// Best-guess URL a provisioned worker can reach this orchestrator at, when
/// `--public-url` isn't given: `bind`'s scheme plus host, with `0.0.0.0`
/// (which a worker on another machine can't dial) swapped for the address
/// this machine advertises in its join line.
fn default_public_url(bind: &str, tls: bool, ip: &str) -> String {
    let scheme = if tls { "https" } else { "http" };
    let host = bind.replacen("0.0.0.0", ip, 1);
    format!("{scheme}://{host}")
}

/// `lgtm worker` takes the same URL a person would paste from a browser, so
/// an http(s) one becomes its ws(s) equivalent.
fn ws_url(url: &str) -> String {
    match url.split_once("://") {
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("https", rest)) => format!("wss://{rest}"),
        _ => url.to_string(),
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
}

fn default_repo() -> anyhow::Result<String> {
    let result = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output();
    match result {
        Ok(output) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => anyhow::bail!("pass --repo or run inside a git repository with an origin remote"),
    }
}

fn require_token(token: Option<String>, data_dir: &std::path::Path) -> String {
    match config::resolve_token(token, data_dir) {
        Some(t) => t,
        None => {
            eprintln!("no token: run `lgtm serve` on this machine, or pass --token");
            std::process::exit(2);
        }
    }
}

#[tokio::main]
async fn main() {
    // rustls needs a process-wide crypto provider before any TLS client is
    // built; ring is the only one in the graph, so install it once here.
    let _ = rustls::crypto::ring::default_provider().install_default();
    // A Windows upgrade can't delete the binary it's replacing while it's
    // running; the next run cleans up the leftover sibling.
    upgrade::cleanup_old_binary();
    let cli = Cli::parse();
    let code = match dispatch(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

async fn dispatch(cli: Cli) -> anyhow::Result<i32> {
    let Cli {
        orchestrator,
        token,
        ca,
        command,
    } = cli;

    if let Command::Serve {
        bind,
        data_dir,
        tls_cert,
        tls_key,
        provision,
        provision_max,
        public_url,
        no_worker,
    } = command
    {
        init_tracing();
        let bind_addr: std::net::SocketAddr = bind.parse()?;
        let data_dir = config::data_dir(data_dir);
        // Unlike every other subcommand, `serve` mints a token rather than
        // demanding one: it is the machine everyone else joins.
        let (token, source) = lgtm_orchestrator::token::resolve_or_create(token, &data_dir)?;
        if source == lgtm_orchestrator::token::TokenSource::Generated {
            tracing::info!(
                "generated token {token} (saved to {})",
                config::stored_token_path(&data_dir).display()
            );
        }
        let tls = match (tls_cert, tls_key) {
            (Some(cert), Some(key)) => Some((cert, key)),
            (None, None) => None,
            _ => anyhow::bail!("pass both --tls-cert and --tls-key"),
        };
        // A specific bind address is the only one workers can dial.
        let ip = if bind_addr.ip().is_unspecified() {
            lgtm_orchestrator::local::advertised_ip()
        } else {
            bind_addr.ip().to_string()
        };
        let provision = provision.map(|command| lgtm_orchestrator::ProvisionOptions {
            command,
            max: provision_max,
            public_url: public_url.unwrap_or_else(|| default_public_url(&bind, tls.is_some(), &ip)),
        });
        let serve_opts = lgtm_orchestrator::ServeOptions {
            bind: bind_addr,
            token,
            data_dir,
            tls,
            provision,
        };
        eprintln!("{}", lgtm_orchestrator::local::join_line_for(&serve_opts));
        lgtm_orchestrator::local::serve_local(lgtm_orchestrator::local::LocalOptions {
            serve: serve_opts,
            worker: !no_worker,
            worker_name: lgtm_agent::default_name(),
            worker_slots: lgtm_agent::default_slots(),
        })
        .await?;
        return Ok(0);
    }

    if let Command::Worker {
        url,
        name,
        slots,
        ephemeral,
        max_tasks,
        data_dir,
    } = command
    {
        init_tracing();
        let data_dir = config::data_dir(data_dir);
        let token = require_token(token, &data_dir);
        lgtm_agent::run(lgtm_agent::WorkerOptions {
            orchestrator: ws_url(&url),
            token,
            name: name.unwrap_or_else(lgtm_agent::default_name),
            data_dir,
            slots: slots.unwrap_or_else(lgtm_agent::default_slots),
            ephemeral,
            max_tasks,
            ca,
        })
        .await?;
        return Ok(0);
    }

    if let Command::Upgrade { version } = command {
        return upgrade::run(version).await;
    }

    let token = require_token(token, &config::data_dir(None));
    let client = Client::new(orchestrator.clone(), token.clone(), ca.as_deref())?;

    match command {
        Command::Serve { .. } | Command::Worker { .. } | Command::Upgrade { .. } => {
            unreachable!("handled above")
        }
        Command::Workers => {
            let workers: Vec<WorkerStatus> = client.get("/api/workers").await?;
            println!(
                "{:<16}{:<8}{:<8}{:<16}{:<10}SLOTS",
                "NAME", "OS", "ARCH", "EXECUTORS", "KIND"
            );
            for w in workers {
                let executors = w
                    .info
                    .executors
                    .iter()
                    .map(|e| e.binary())
                    .collect::<Vec<_>>()
                    .join(",");
                let slots = format!("{}/{}", w.running.len(), w.info.slots);
                let kind = if w.info.ephemeral {
                    "ephemeral"
                } else {
                    "fixed"
                };
                println!(
                    "{:<16}{:<8}{:<8}{:<16}{:<10}{}",
                    w.info.name, w.info.os, w.info.arch, executors, kind, slots
                );
            }
            Ok(0)
        }
        Command::Run {
            on,
            repo,
            base,
            agent,
            issue,
            linear,
            prompt,
        } => {
            if issue.is_some() && linear.is_some() {
                anyhow::bail!("pass only one of --issue or --linear");
            }
            let (issue, prompt) = match (issue, prompt) {
                (Some(issue), prompt) => (Some(issue), prompt),
                (None, Some(prompt)) => match prompt.strip_prefix("github:") {
                    Some(rest) => (Some(rest.to_string()), None),
                    None => (None, Some(prompt)),
                },
                (None, None) => (None, None),
            };
            if issue.is_none() && linear.is_none() && prompt.is_none() {
                anyhow::bail!("pass a prompt, --issue, or --linear");
            }
            match (issue, linear) {
                (Some(issue), _) => {
                    run::run_from_issue(
                        &client,
                        &orchestrator,
                        &token,
                        ca.as_deref(),
                        issue,
                        base,
                        agent,
                        on,
                    )
                    .await
                }
                (None, Some(linear)) => {
                    let repo = match repo {
                        Some(r) => r,
                        None => default_repo()?,
                    };
                    run::run_from_linear(
                        &client,
                        &orchestrator,
                        &token,
                        ca.as_deref(),
                        linear,
                        repo,
                        base,
                        agent,
                        on,
                    )
                    .await
                }
                (None, None) => {
                    let repo = match repo {
                        Some(r) => r,
                        None => default_repo()?,
                    };
                    run::run(
                        &client,
                        &orchestrator,
                        &token,
                        ca.as_deref(),
                        repo,
                        base,
                        prompt.expect("checked above: issue, linear, or prompt is present"),
                        agent,
                        on,
                        TaskKind::Run,
                    )
                    .await
                }
            }
        }
        Command::Plan {
            on,
            repo,
            base,
            agent,
            goal,
        } => {
            let repo = match repo {
                Some(r) => r,
                None => default_repo()?,
            };
            run::run(
                &client,
                &orchestrator,
                &token,
                ca.as_deref(),
                repo,
                base,
                goal,
                agent,
                on,
                TaskKind::Plan,
            )
            .await
        }
        Command::Tasks => {
            let tasks: Vec<Task> = client.get("/api/tasks").await?;
            print_task_table(tasks);
            Ok(0)
        }
        Command::Show { id } => {
            let detail: TaskDetail = client.get(&format!("/api/tasks/{id}")).await?;
            println!("{}", serde_json::to_string_pretty(&detail.task)?);
            if let Some(pr) = &detail.task.pull_request {
                println!("pr: {}", pr.url);
            }
            if let Some(ci) = &detail.task.ci {
                println!("ci: {} {}", ci_str(ci.state), ci.url);
            }
            if let Some(linear) = &detail.task.spec.linear {
                println!("linear: {}", linear.url);
            }
            for e in detail.events {
                println!("{} {}", e.at, serde_json::to_string(&e.event)?);
            }
            if let Some(result) = &detail.task.result {
                render::print_validation(&result.validation, &mut std::io::stdout())?;
                if let Some(review) = &result.review {
                    render::print_review(review, &mut std::io::stdout())?;
                }
                render::print_cost(result.cost_usd, &mut std::io::stdout())?;
                if let Some(plan) = &result.plan {
                    render::print_plan(plan, &mut std::io::stdout())?;
                }
            }
            Ok(0)
        }
        Command::Logs { id } => {
            let detail: TaskDetail = client.get(&format!("/api/tasks/{id}")).await?;
            let mut stdout = std::io::stdout();
            for e in detail.events {
                render::render(&e.event, &mut stdout)?;
            }
            Ok(0)
        }
        Command::Diff { id } => {
            let detail: TaskDetail = client.get(&format!("/api/tasks/{id}")).await?;
            match detail.task.result {
                Some(result) => {
                    print!("{}", result.diff);
                    Ok(0)
                }
                None => {
                    eprintln!("no diff yet (status: {})", status_str(detail.task.status));
                    Ok(1)
                }
            }
        }
        Command::Approve { id } => {
            let task: Task = client
                .post(&format!("/api/tasks/{id}/approve"), None::<&()>)
                .await?;
            println!("{}", serde_json::to_string_pretty(&task)?);
            Ok(0)
        }
        Command::Reject { id } => {
            let task: Task = client
                .post(&format!("/api/tasks/{id}/reject"), None::<&()>)
                .await?;
            println!("{}", serde_json::to_string_pretty(&task)?);
            Ok(0)
        }
        Command::Cancel { id } => {
            let task: Task = client
                .post(&format!("/api/tasks/{id}/cancel"), None::<&()>)
                .await?;
            println!("{}", serde_json::to_string_pretty(&task)?);
            Ok(0)
        }
        Command::Merge { id } => {
            let task: Task = client
                .post(&format!("/api/tasks/{id}/merge"), None::<&()>)
                .await?;
            println!("{}", serde_json::to_string_pretty(&task)?);
            Ok(0)
        }
        Command::Tell { id, message } => {
            // Events already delivered by a prior `run`/`tell` shouldn't replay.
            let detail: TaskDetail = client.get(&format!("/api/tasks/{id}")).await?;
            let from = detail.events.len();
            let _: Task = client
                .post(
                    &format!("/api/tasks/{id}/message"),
                    Some(&FollowUp { text: &message }),
                )
                .await?;
            eprintln!("task {id} → follow-up sent");
            run::stream(&orchestrator, &token, ca.as_deref(), &id, from).await
        }
        Command::Backlog { command } => match command {
            BacklogCommand::Github {
                repo,
                label,
                plan,
                approve_plans,
                max,
                dry_run,
                base,
                agent,
                on,
            } => {
                let Some((owner, name)) = repo.split_once('/') else {
                    anyhow::bail!("expected OWNER/REPO, got '{repo}'");
                };
                let source = BatchSource::GithubLabel {
                    owner: owner.to_string(),
                    repo: name.to_string(),
                    label,
                };
                backlog::create(
                    &client,
                    source,
                    None,
                    base,
                    agent,
                    on,
                    plan,
                    approve_plans,
                    max,
                    dry_run,
                )
                .await
            }
            BacklogCommand::Linear {
                team,
                state,
                repo,
                plan,
                approve_plans,
                max,
                dry_run,
                base,
                agent,
                on,
            } => {
                let source = BatchSource::Linear { team, state };
                backlog::create(
                    &client,
                    source,
                    Some(repo),
                    base,
                    agent,
                    on,
                    plan,
                    approve_plans,
                    max,
                    dry_run,
                )
                .await
            }
            BacklogCommand::List => backlog::list(&client).await,
            BacklogCommand::Status { id } => backlog::status(&client, &id).await,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{CiStatus, PullRequest, TaskSpec};

    #[test]
    fn default_public_url_swaps_unreachable_bind_host() {
        assert_eq!(
            default_public_url("0.0.0.0:4750", false, "127.0.0.1"),
            "http://127.0.0.1:4750"
        );
    }

    #[test]
    fn default_public_url_uses_https_scheme_under_tls() {
        assert_eq!(
            default_public_url("0.0.0.0:4750", true, "127.0.0.1"),
            "https://127.0.0.1:4750"
        );
    }

    #[test]
    fn default_public_url_keeps_a_reachable_host() {
        assert_eq!(
            default_public_url("10.0.0.5:4750", false, "100.64.0.1"),
            "http://10.0.0.5:4750"
        );
    }

    fn sample_task(pull_request: Option<PullRequest>, ci: Option<CiStatus>) -> Task {
        Task {
            id: "0123abcd".into(),
            spec: TaskSpec {
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                base_branch: "main".into(),
                prompt: "add a /health endpoint".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: vec![],
                batch: None,
            },
            status: TaskStatus::Approved,
            worker: None,
            created_at: 1,
            result: None,
            error: None,
            pull_request,
            ci,
        }
    }

    /// A minimal task for `order_tasks`/`display_status` tests, where only
    /// id, status, worker, parent, and dependencies matter.
    fn task(
        id: &str,
        status: TaskStatus,
        worker: Option<&str>,
        parent: Option<&str>,
        depends_on: &[&str],
    ) -> Task {
        let mut t = sample_task(None, None);
        t.id = id.into();
        t.status = status;
        t.worker = worker.map(String::from);
        t.spec.parent = parent.map(String::from);
        t.spec.depends_on = depends_on.iter().map(|s| s.to_string()).collect();
        t
    }

    #[test]
    fn pr_cell_empty_without_pull_request() {
        assert_eq!(pr_cell(&sample_task(None, None)), "");
    }

    #[test]
    fn pr_cell_pending_without_ci() {
        let pr = PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        };
        assert_eq!(pr_cell(&sample_task(Some(pr), None)), "#12 …");
    }

    #[test]
    fn pr_cell_marks_ci_success() {
        let pr = PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        };
        let ci = CiStatus {
            state: CiState::Success,
            url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
        };
        assert_eq!(pr_cell(&sample_task(Some(pr), Some(ci))), "#12 ✓");
    }

    #[test]
    fn pr_cell_marks_ci_failure() {
        let pr = PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        };
        let ci = CiStatus {
            state: CiState::Failure,
            url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
        };
        assert_eq!(pr_cell(&sample_task(Some(pr), Some(ci))), "#12 ✗");
    }

    #[test]
    fn order_tasks_places_children_after_parent() {
        let p = task("p", TaskStatus::Queued, None, None, &[]);
        let q = task("q", TaskStatus::Queued, None, None, &[]);
        let c1 = task("c1", TaskStatus::Queued, None, Some("p"), &[]);
        let c2 = task("c2", TaskStatus::Queued, None, Some("p"), &[]);
        let ordered = order_tasks(vec![p, q, c1, c2]);
        let ids: Vec<&str> = ordered.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["p", "c1", "c2", "q"]);
    }

    #[test]
    fn display_status_blocks_queued_task_with_unmet_dependency() {
        let dep = task("d", TaskStatus::Queued, None, None, &[]);
        let t = task("t", TaskStatus::Queued, None, None, &["d"]);
        assert_eq!(display_status(&t, &[dep, t.clone()]), "blocked");
    }

    #[test]
    fn display_status_queued_once_dependency_is_approved() {
        let dep = task("d", TaskStatus::Approved, None, None, &[]);
        let t = task("t", TaskStatus::Queued, None, None, &["d"]);
        assert_eq!(display_status(&t, &[dep, t.clone()]), "queued");
    }
}
