mod backlog;
mod cli;
mod mcp;
mod render;
mod run;
mod serve;
mod table;
mod terminal;
mod upgrade;

use std::path::Path;

use clap::Parser;
use lgtm_client::{Client, FromIssue, FromLinear, NewGoal, PromoteTodo};
use lgtm_orchestrator::token::{data_dir, resolve_token};
use lgtm_protocol::{BatchSource, TaskKind, TaskSpec};

use crate::cli::{BacklogCommand, Cli, Command, MemoryCommand, Target, TodoCommand};
use crate::table::{
    ci_str, print_goal_table, print_memory_table, print_task_table, print_todo_table, status_str,
};

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
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
        Command::Runner(args) => return serve::runner(args, cli.token, cli.ca).await,
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
        Command::Serve(_) | Command::Runner(_) | Command::Upgrade { .. } => {
            unreachable!("handled by dispatch")
        }
        Command::Runners => runners(client).await,
        Command::Mcp => mcp::serve(client).await,
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
        Command::Goal {
            target,
            plan,
            objective,
        } => goal(client, target, plan, objective).await,
        Command::Goals => {
            print_goal_table(client.goals().await?);
            Ok(0)
        }
        Command::Tasks => {
            print_task_table(client.tasks().await?);
            Ok(0)
        }
        Command::Plans { id } => plans(client, &id).await,
        Command::Stats { days } => {
            let since = now_ms().saturating_sub(u64::from(days) * 24 * 60 * 60 * 1000);
            let stats = client.stats(Some(since)).await?;
            render::print_stats(&stats, &mut std::io::stdout())?;
            Ok(0)
        }
        Command::Why { sha } => why(client, &sha).await,
        Command::Backlog { command } => backlog_command(client, command).await,
        Command::Memory { command } => memory_command(client, command).await,
        Command::Todo { command } => todo_command(client, command).await,
        other => task_command(client, other).await,
    }
}

/// The commands that name one task.
async fn task_command(client: &Client, command: Command) -> anyhow::Result<i32> {
    match command {
        Command::Show { id } => show(client, &id).await,
        Command::Terminal { id, close } => {
            if close {
                client.close_terminal(&id).await?;
                return Ok(0);
            }
            terminal::attach(client, &id).await
        }
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
        Command::Interrupt { id } => print_json(client.interrupt(&id).await?),
        Command::Merge { id } => print_json(client.merge(&id).await?),
        Command::Retry { id, on, agent } => {
            // The failure that made this worth retrying is already on record;
            // replaying it would end the stream before the new run started.
            let from = client.task(&id).await?.events.len();
            let into = lgtm_client::Retry {
                runner: on,
                executor: agent,
            };
            client.retry(&id, &into).await?;
            eprintln!("retrying {id}");
            run::stream(client, &id, from).await
        }
        Command::Pad { id, set } => pad(client, &id, set).await,
        Command::Allow { id, host } => {
            client.allow_host(&id, &host).await?;
            println!("allowed {host} for {id}; it applies to the next run");
            Ok(0)
        }
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

/// `--set -` reads the notes from stdin, so an editor or a pipe can write them.
async fn pad(client: &Client, id: &str, set: Option<String>) -> anyhow::Result<i32> {
    let Some(set) = set else {
        let notes = client.task(id).await?.task.scratchpad;
        match notes.trim_end() {
            "" => println!("no notes"),
            notes => println!("{notes}"),
        }
        return Ok(0);
    };
    let content = match set.as_str() {
        "-" => std::io::read_to_string(std::io::stdin())?,
        text => text.to_string(),
    };
    client.set_scratchpad(id, &content).await?;
    println!("notes updated");
    Ok(0)
}

fn print_json(value: impl serde::Serialize) -> anyhow::Result<i32> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(0)
}

async fn runners(client: &Client) -> anyhow::Result<i32> {
    println!(
        "{:<16}{:<8}{:<8}{:<16}{:<10}{:<8}CAPABILITIES",
        "NAME", "OS", "ARCH", "EXECUTORS", "KIND", "SLOTS"
    );
    for w in client.runners().await? {
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
        let capabilities = w.info.capabilities.join(" ");
        println!(
            "{:<16}{:<8}{:<8}{:<16}{:<10}{:<8}{}",
            w.info.name, w.info.os, w.info.arch, executors, kind, slots, capabilities
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
            runner: self.on,
            issue: None,
            linear: None,
            kind,
            parent: None,
            depends_on: self.after,
            depends_on_condition: self.after_condition,
            batch: None,
            sandbox: self.sandbox,
            requirements: self.requirements,
            goal: None,
            review_executor: self.review_with,
            model: self.model,
            allowed_hosts: Vec::new(),
            session: self.session,
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
        let body = FromIssue {
            issue: &issue,
            base_branch: &target.base,
            executor: target.agent,
            runner: target.on.as_deref(),
            sandbox: target.sandbox,
            requirements: target.requirements,
            review_executor: target.review_with,
            model: target.model,
        };
        client.create_task_from_issue(&body).await?
    } else if let Some(linear) = linear {
        let repo = target.repo.map_or_else(default_repo, Ok)?;
        let body = FromLinear {
            issue: &linear,
            repository: &repo,
            base_branch: &target.base,
            executor: target.agent,
            runner: target.on.as_deref(),
            sandbox: target.sandbox,
            requirements: target.requirements,
            review_executor: target.review_with,
            model: target.model,
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

/// Creates the goal, then follows its first task the way `run` does.
async fn goal(
    client: &Client,
    target: Target,
    plan: bool,
    objective: String,
) -> anyhow::Result<i32> {
    let body = NewGoal {
        objective,
        repository: target.repo.map_or_else(default_repo, Ok)?,
        base_branch: target.base,
        executor: target.agent,
        runner: target.on,
        plan,
    };
    let id = client.create_goal(&body).await?.goal.id;
    eprintln!("goal {id} created");
    let Some(task) = client.goal(&id).await?.tasks.into_iter().next() else {
        anyhow::bail!("goal {id} has no task");
    };
    run::announce_and_stream(client, task).await
}

/// `id` is a goal id, unless the orchestrator says it isn't one: an id could
/// also name a plan task directly.
async fn plans(client: &Client, id: &str) -> anyhow::Result<i32> {
    let versions = match client.goal_plans(id).await {
        Ok(versions) => versions,
        Err(err) if err.to_string().starts_with("404") => client.task_plans(id).await?,
        Err(err) => return Err(err),
    };
    render::print_plan_versions(&versions, &mut std::io::stdout())?;
    Ok(0)
}

async fn why(client: &Client, sha: &str) -> anyhow::Result<i32> {
    let provenance = client.provenance(sha).await?;
    render::print_provenance(sha, &provenance, &mut std::io::stdout())?;
    Ok(0)
}

async fn show(client: &Client, id: &str) -> anyhow::Result<i32> {
    let detail = client.task(id).await?;
    println!("{}", serde_json::to_string_pretty(&detail.task)?);
    for overlap in &detail.overlaps {
        println!(
            "overlaps with {}: {}",
            overlap.task,
            overlap.files.join(", ")
        );
    }
    for (target, reason) in lgtm_protocol::pending_requests(&detail.events, &detail.task.spec) {
        println!("requested: {target} — {reason}");
    }
    if !detail.task.spec.allowed_hosts.is_empty() {
        println!(
            "allowed hosts: {}",
            detail.task.spec.allowed_hosts.join(", ")
        );
    }
    if let Some(pr) = &detail.task.pull_request {
        println!("pr: {}", pr.url);
    }
    if let Some(ci) = &detail.task.ci {
        println!("ci: {} {}", ci_str(ci.state), ci.url);
    }
    if let Some(linear) = &detail.task.spec.linear {
        println!("linear: {}", linear.url);
    }
    let plan_version = lgtm_protocol::plan_versions(&detail.task, &detail.events)
        .into_iter()
        .next_back();
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
            if let Some(version) = &plan_version {
                println!(
                    "plan v{} ({})",
                    version.version,
                    table::wire_str(version.status)
                );
            }
            render::print_plan(plan, &mut stdout)?;
        }
    }
    Ok(0)
}

async fn memory_command(client: &Client, command: MemoryCommand) -> anyhow::Result<i32> {
    match command {
        MemoryCommand::Add { repo, content } => {
            let memory = client.create_memory(repo.as_deref(), &content).await?;
            println!("memory {} added", memory.id);
        }
        MemoryCommand::List { repo, pending } => {
            print_memory_table(&client.memories(repo.as_deref(), pending).await?);
        }
        MemoryCommand::Approve { id } => {
            client.approve_memory(&id).await?;
            println!("memory {id} approved");
        }
        MemoryCommand::Rm { id } => {
            client.delete_memory(&id).await?;
            println!("memory {id} removed");
        }
    }
    Ok(0)
}

async fn todo_command(client: &Client, command: TodoCommand) -> anyhow::Result<i32> {
    match command {
        TodoCommand::Add {
            repo,
            title,
            description,
        } => {
            let todo = client
                .create_todo(
                    repo.as_deref(),
                    &title,
                    description.as_deref().unwrap_or(""),
                )
                .await?;
            println!("todo {} added", todo.id);
        }
        TodoCommand::List { repo } => {
            print_todo_table(&client.todos(repo.as_deref()).await?);
        }
        TodoCommand::Done { id } => {
            client.finish_todo(&id).await?;
            println!("todo {id} done");
        }
        TodoCommand::Promote { id, target } => {
            let body = PromoteTodo {
                base_branch: target.base,
                executor: target.agent,
                runner: target.on,
            };
            let task = client.promote_todo(&id, &body).await?;
            eprintln!("task {} created from todo {id}", task.id);
            return run::announce_and_stream(client, task).await;
        }
        TodoCommand::Rm { id } => {
            client.delete_todo(&id).await?;
            println!("todo {id} removed");
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
