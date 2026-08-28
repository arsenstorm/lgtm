//! lgtm worker agent: connects out to the orchestrator and runs coding tasks.

mod automation;
mod connection;
mod git;
mod plan;
mod policy;
mod proc;
mod runner;
mod validate;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use lgtm_protocol::{Executor, WorkerInfo};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

use crate::connection::Ctx;

#[derive(Parser)]
#[command(name = "lgtm-agent", about = "lgtm worker agent")]
struct Args {
    /// Orchestrator WebSocket base, ws:// or wss://.
    #[arg(long, env = "LGTM_ORCHESTRATOR", default_value = "ws://127.0.0.1:4750")]
    orchestrator: String,
    /// Worker name reported to the orchestrator.
    #[arg(long, env = "LGTM_WORKER_NAME")]
    name: Option<String>,
    #[arg(long, env = "LGTM_TOKEN")]
    token: Option<String>,
    /// Where mirrors and worktrees live.
    #[arg(long, env = "LGTM_DATA_DIR")]
    data_dir: Option<PathBuf>,
    /// Maximum tasks to run at once.
    #[arg(long, env = "LGTM_SLOTS")]
    slots: Option<u32>,
    /// Exit once `--max-tasks` runs have ended; for disposable machines.
    #[arg(long, env = "LGTM_EPHEMERAL")]
    ephemeral: bool,
    /// Runs to accept before exiting. Only read with `--ephemeral`.
    #[arg(long, env = "LGTM_MAX_TASKS", default_value_t = 1)]
    max_tasks: u32,
    /// Extra CA certificate (PEM) to trust for `wss://`.
    #[arg(long, env = "LGTM_CA")]
    ca: Option<PathBuf>,
}

fn default_slots() -> u32 {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    (cpus / 4).max(1)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let token = args.token.context("set LGTM_TOKEN or pass --token")?;
    let name = args.name.unwrap_or_else(default_name);
    let data_dir = match args.data_dir {
        Some(dir) => dir,
        None => dirs::home_dir().context("no home directory")?.join(".lgtm"),
    };

    let executors: Vec<Executor> = [Executor::Claude, Executor::Codex]
        .into_iter()
        .filter(|e| which::which(e.binary()).is_ok())
        .collect();
    let slots = args.slots.unwrap_or_else(default_slots);
    tracing::info!(
        "worker {name} in {} executors {executors:?} slots {slots}",
        data_dir.display()
    );

    let info = WorkerInfo {
        name,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        executors,
        slots,
        ephemeral: args.ephemeral,
    };

    let connector = match &args.ca {
        Some(path) => {
            let pem = std::fs::read(path).with_context(|| format!("read CA {}", path.display()))?;
            let count = connection::load_roots(&pem)?;
            tracing::info!(
                "trusting {count} extra CA certificate(s) from {}",
                path.display()
            );
            Some(connection::ca_connector(&pem)?)
        }
        None => None,
    };

    // One channel for the process lifetime: events emitted while disconnected
    // wait here and flush on the next connection.
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = Arc::new(Ctx::new(data_dir, tx, args.ephemeral, args.max_tasks));

    tokio::spawn(shutdown(ctx.clone()));

    connection::run(&args.orchestrator, &token, &info, connector, ctx, rx).await;
    Ok(())
}

/// The process is killed without unwinding, so `kill_on_drop` never runs.
/// Cancel every task by hand and give the runners a moment to kill their children.
async fn shutdown(ctx: Arc<Ctx>) {
    terminated().await;
    tracing::info!("shutting down, cancelling running tasks");
    let senders: Vec<_> = ctx
        .running
        .lock()
        .expect("running map poisoned")
        .drain()
        .map(|(_, sender)| sender)
        .collect();
    for sender in senders {
        let _ = sender.send(());
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    std::process::exit(0);
}

#[cfg(unix)]
async fn terminated() {
    use tokio::signal::unix::{signal, SignalKind};
    let Ok(mut term) = signal(SignalKind::terminate()) else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn terminated() {
    let _ = tokio::signal::ctrl_c().await;
}

fn default_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "worker".to_string())
}
