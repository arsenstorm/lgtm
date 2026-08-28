mod http;
mod render;
mod run;

use clap::{Parser, Subcommand};
use http::Client;
use lgtm_protocol::{
    CiState, Executor, Review, StoredEvent, Task, TaskKind, TaskStatus, WorkerStatus,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lgtm")]
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
    },
    Workers,
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
    Tasks,
    Show {
        id: String,
    },
    Logs {
        id: String,
    },
    Diff {
        id: String,
    },
    Approve {
        id: String,
    },
    Reject {
        id: String,
    },
    Cancel {
        id: String,
    },
    Merge {
        id: String,
    },
    /// Send a follow-up to a task awaiting review, then resume streaming.
    Tell {
        id: String,
        message: String,
    },
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
fn pr_cell(task: &Task) -> String {
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
fn order_tasks(tasks: Vec<Task>) -> Vec<Task> {
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
fn display_status(task: &Task, all: &[Task]) -> String {
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

fn first_line_truncated(s: &str, max: usize) -> String {
    let first = s.lines().next().unwrap_or("");
    if first.chars().count() > max {
        first.chars().take(max).collect()
    } else {
        first.to_string()
    }
}

/// Best-guess URL a provisioned worker can reach this orchestrator at, when
/// `--public-url` isn't given: `bind`'s scheme plus host, with `0.0.0.0`
/// (which a worker on another machine can't dial) swapped for `127.0.0.1`.
fn default_public_url(bind: &str, tls: bool) -> String {
    let scheme = if tls { "https" } else { "http" };
    let host = bind.replacen("0.0.0.0", "127.0.0.1", 1);
    format!("{scheme}://{host}")
}

fn default_data_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .expect("HOME or USERPROFILE must be set");
    PathBuf::from(home).join(".lgtm")
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

fn require_token(token: Option<String>) -> String {
    match token {
        Some(t) => t,
        None => {
            eprintln!("set LGTM_TOKEN or pass --token");
            std::process::exit(2);
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match dispatch(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
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
    } = command
    {
        let token = require_token(token);
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .init();
        let bind_addr: std::net::SocketAddr = bind.parse()?;
        let data_dir = data_dir.unwrap_or_else(default_data_dir);
        let tls = match (tls_cert, tls_key) {
            (Some(cert), Some(key)) => Some((cert, key)),
            (None, None) => None,
            _ => anyhow::bail!("pass both --tls-cert and --tls-key"),
        };
        let provision = provision.map(|command| lgtm_orchestrator::ProvisionOptions {
            command,
            max: provision_max,
            public_url: public_url.unwrap_or_else(|| default_public_url(&bind, tls.is_some())),
        });
        lgtm_orchestrator::serve(lgtm_orchestrator::ServeOptions {
            bind: bind_addr,
            token,
            data_dir,
            tls,
            provision,
        })
        .await?;
        return Ok(0);
    }

    let token = require_token(token);
    let client = Client::new(orchestrator.clone(), token.clone(), ca.as_deref())?;

    match command {
        Command::Serve { .. } => unreachable!("handled above"),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{CiStatus, PullRequest, TaskSpec};

    #[test]
    fn default_public_url_swaps_unreachable_bind_host() {
        assert_eq!(
            default_public_url("0.0.0.0:4750", false),
            "http://127.0.0.1:4750"
        );
    }

    #[test]
    fn default_public_url_uses_https_scheme_under_tls() {
        assert_eq!(
            default_public_url("0.0.0.0:4750", true),
            "https://127.0.0.1:4750"
        );
    }

    #[test]
    fn default_public_url_keeps_a_reachable_host() {
        assert_eq!(
            default_public_url("10.0.0.5:4750", false),
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
