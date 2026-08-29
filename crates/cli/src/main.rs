mod backlog;
mod cli;
mod render;
mod run;
mod serve;
mod table;
mod upgrade;

use std::path::Path;

use clap::Parser;
use lgtm_client::{Client, FromLinear};
use lgtm_orchestrator::token::{data_dir, resolve_token};
use lgtm_protocol::{BatchSource, TaskKind, TaskSpec};

use crate::cli::{BacklogCommand, Cli, Command, Target};
use crate::table::{ci_str, print_task_table, status_str};

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

fn require_token(token: Option<String>, data_dir: &Path) -> String {
    match resolve_token(token, data_dir) {
        Some(t) => t,
        None => {
            eprintln!("no token: run `lgtm serve` on this machine, or pass --token");
            std::process::exit(2);
        }
    }
}

/// A refused connection means the orchestrator isn't there, which is worth
/// saying plainly instead of passing reqwest's wording through.
fn describe(err: &anyhow::Error, orchestrator: &str) -> String {
    let refused = err
        .downcast_ref::<reqwest::Error>()
        .is_some_and(reqwest::Error::is_connect);
    if refused {
        return format!(
            "cannot reach {orchestrator}: run `lgtm serve` first, or pass --orchestrator"
        );
    }
    format!("{err:#}")
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
    let orchestrator = cli.orchestrator.clone();
    let code = match dispatch(cli).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {}", describe(&e, &orchestrator));
            1
        }
    };
    std::process::exit(code);
}

fn client(orchestrator: &str, token: String, ca: Option<&Path>) -> anyhow::Result<Client> {
    let Some(path) = ca else {
        return Ok(Client::new(orchestrator, token));
    };
    let pem =
        std::fs::read(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    Client::with_ca(orchestrator, token, &pem)
}

async fn dispatch(cli: Cli) -> anyhow::Result<i32> {
    let command = match cli.command {
        Command::Serve(args) => return serve::serve(args, cli.token).await,
        Command::Worker(args) => return serve::worker(args, cli.token, cli.ca).await,
        Command::Upgrade { version } => return upgrade::run(version).await,
        command => command,
    };
    let token = require_token(cli.token, &data_dir(None));
    let client = client(&cli.orchestrator, token, cli.ca.as_deref())?;
    run_command(&client, command).await
}

/// Every command that talks to a running orchestrator.
async fn run_command(client: &Client, command: Command) -> anyhow::Result<i32> {
    match command {
        Command::Serve(_) | Command::Worker(_) | Command::Upgrade { .. } => {
            unreachable!("handled by dispatch")
        }
        Command::Workers => workers(client).await,
        Command::Run {
            target,
            issue,
            linear,
            prompt,
        } => run(client, target, (issue, linear, prompt)).await,
        Command::Plan { target, goal } => {
            let spec = target.spec(goal, TaskKind::Plan)?;
            run::announce_and_stream(client, client.create_task(&spec).await?).await
        }
        Command::Tasks => {
            print_task_table(client.tasks().await?);
            Ok(0)
        }
        Command::Backlog { command } => backlog_command(client, command).await,
        other => task_command(client, other).await,
    }
}

/// The commands that name one task.
async fn task_command(client: &Client, command: Command) -> anyhow::Result<i32> {
    match command {
        Command::Show { id } => show(client, &id).await,
        Command::Logs { id } => {
            let mut stdout = std::io::stdout();
            for e in client.task(&id).await?.events {
                render::render(&e.event, &mut stdout)?;
            }
            Ok(0)
        }
        Command::Diff { id } => {
            let detail = client.task(&id).await?;
            let Some(result) = detail.task.result else {
                eprintln!("no diff yet (status: {})", status_str(detail.task.status));
                return Ok(1);
            };
            print!("{}", result.diff);
            Ok(0)
        }
        Command::Approve { id } => print_json(client.approve(&id).await?),
        Command::Reject { id } => print_json(client.reject(&id).await?),
        Command::Cancel { id } => print_json(client.cancel(&id).await?),
        Command::Merge { id } => print_json(client.merge(&id).await?),
        Command::Tell { id, message } => {
            // Events already delivered by a prior `run`/`tell` shouldn't replay.
            let from = client.task(&id).await?.events.len();
            client.tell(&id, &message).await?;
            eprintln!("task {id} → follow-up sent");
            run::stream(client, &id, from).await
        }
        _ => unreachable!("handled by run_command"),
    }
}

fn print_json(value: impl serde::Serialize) -> anyhow::Result<i32> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(0)
}

async fn workers(client: &Client) -> anyhow::Result<i32> {
    println!(
        "{:<16}{:<8}{:<8}{:<16}{:<10}SLOTS",
        "NAME", "OS", "ARCH", "EXECUTORS", "KIND"
    );
    for w in client.workers().await? {
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

impl Target {
    fn spec(self, prompt: String, kind: TaskKind) -> anyhow::Result<TaskSpec> {
        Ok(TaskSpec {
            repository: self.repo.map_or_else(default_repo, Ok)?,
            base_branch: self.base,
            prompt,
            executor: self.agent,
            worker: self.on,
            issue: None,
            linear: None,
            kind,
            parent: None,
            depends_on: vec![],
            batch: None,
            sandbox: self.sandbox,
        })
    }
}

/// `lgtm run`'s three sources of work: a GitHub issue (`--issue` or a
/// `github:` prompt), a Linear issue, or a plain prompt.
async fn run(
    client: &Client,
    target: Target,
    (issue, linear, prompt): (Option<String>, Option<String>, Option<String>),
) -> anyhow::Result<i32> {
    if issue.is_some() && linear.is_some() {
        anyhow::bail!("pass only one of --issue or --linear");
    }
    let (issue, prompt) = match prompt.as_deref().and_then(|p| p.strip_prefix("github:")) {
        Some(rest) if issue.is_none() => (Some(rest.to_string()), None),
        _ => (issue, prompt),
    };
    let task = if let Some(issue) = issue {
        client
            .create_task_from_issue(
                &issue,
                &target.base,
                target.agent,
                target.on.as_deref(),
                target.sandbox,
            )
            .await?
    } else if let Some(linear) = linear {
        let repo = target.repo.map_or_else(default_repo, Ok)?;
        let body = FromLinear {
            issue: &linear,
            repository: &repo,
            base_branch: &target.base,
            executor: target.agent,
            worker: target.on.as_deref(),
            sandbox: target.sandbox,
        };
        client.create_task_from_linear(&body).await?
    } else if let Some(prompt) = prompt {
        client
            .create_task(&target.spec(prompt, TaskKind::Run)?)
            .await?
    } else {
        anyhow::bail!("pass a prompt, --issue, or --linear");
    };
    run::announce_and_stream(client, task).await
}

async fn show(client: &Client, id: &str) -> anyhow::Result<i32> {
    let detail = client.task(id).await?;
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
    let mut stdout = std::io::stdout();
    render::print_executions(&detail.task.executions, &mut stdout)?;
    if let Some(result) = &detail.task.result {
        render::print_validation(&result.validation, &mut stdout)?;
        if let Some(review) = &result.review {
            render::print_review(review, &mut stdout)?;
        }
        render::print_cost(result.cost_usd, &mut stdout)?;
        if let Some(plan) = &result.plan {
            render::print_plan(plan, &mut stdout)?;
        }
    }
    Ok(0)
}

async fn backlog_command(client: &Client, command: BacklogCommand) -> anyhow::Result<i32> {
    match command {
        BacklogCommand::Github { repo, label, flags } => {
            let Some((owner, name)) = repo.split_once('/') else {
                anyhow::bail!("expected OWNER/REPO, got '{repo}'");
            };
            let source = BatchSource::GithubLabel {
                owner: owner.to_string(),
                repo: name.to_string(),
                label,
            };
            backlog::create(client, source, None, flags).await
        }
        BacklogCommand::Linear {
            team,
            state,
            repo,
            flags,
        } => {
            let source = BatchSource::Linear { team, state };
            backlog::create(client, source, Some(repo), flags).await
        }
        BacklogCommand::List => backlog::list(client).await,
        BacklogCommand::Status { id } => backlog::status(client, &id).await,
    }
}
