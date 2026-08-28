mod http;
mod render;
mod run;

use clap::{Parser, Subcommand};
use http::Client;
use lgtm_protocol::{CiState, Executor, StoredEvent, Task, TaskStatus, WorkerStatus};
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

fn first_line_truncated(s: &str, max: usize) -> String {
    let first = s.lines().next().unwrap_or("");
    if first.chars().count() > max {
        first.chars().take(max).collect()
    } else {
        first.to_string()
    }
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
        command,
    } = cli;

    if let Command::Serve { bind, data_dir } = command {
        let token = require_token(token);
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info".into()),
            )
            .init();
        let bind_addr: std::net::SocketAddr = bind.parse()?;
        let data_dir = data_dir.unwrap_or_else(default_data_dir);
        lgtm_orchestrator::serve(bind_addr, token, data_dir).await?;
        return Ok(0);
    }

    let token = require_token(token);
    let client = Client::new(orchestrator.clone(), token.clone());

    match command {
        Command::Serve { .. } => unreachable!("handled above"),
        Command::Workers => {
            let workers: Vec<WorkerStatus> = client.get("/api/workers").await?;
            println!(
                "{:<16}{:<8}{:<8}{:<16}SLOTS",
                "NAME", "OS", "ARCH", "EXECUTORS"
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
                println!(
                    "{:<16}{:<8}{:<8}{:<16}{}",
                    w.info.name, w.info.os, w.info.arch, executors, slots
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
                    run::run_from_issue(&client, &orchestrator, &token, issue, base, agent, on)
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
                        repo,
                        base,
                        prompt.expect("checked above: issue, linear, or prompt is present"),
                        agent,
                        on,
                    )
                    .await
                }
            }
        }
        Command::Tasks => {
            let tasks: Vec<Task> = client.get("/api/tasks").await?;
            println!(
                "{:<10}{:<16}{:<16}{:<10}PROMPT",
                "ID", "STATUS", "WORKER", "PR"
            );
            for t in tasks {
                let worker = t.worker.as_deref().unwrap_or("-");
                let prompt = first_line_truncated(&t.spec.prompt, 60);
                let failed = t.result.as_ref().is_some_and(|r| r.validation_failed());
                let status = status_str(t.status);
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
            run::stream(&orchestrator, &token, &id, from).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{CiStatus, PullRequest, TaskSpec};

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
}
