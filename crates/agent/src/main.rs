//! lgtm worker agent: connects out to the orchestrator and runs coding tasks.

mod connection;
mod git;
mod runner;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    tracing::info!(
        "worker {name} in {} executors {executors:?}",
        data_dir.display()
    );

    let info = WorkerInfo {
        name,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        executors,
    };

    // One channel for the process lifetime: events emitted while disconnected
    // wait here and flush on the next connection.
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = Arc::new(Ctx {
        data_dir,
        tx,
        running: Mutex::new(HashMap::new()),
        mirrors: Mutex::new(HashMap::new()),
    });

    connection::run(&args.orchestrator, &token, &info, ctx, rx).await;
    Ok(())
}

fn default_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "worker".to_string())
}
