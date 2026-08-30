//! lgtm worker agent: connects out to the orchestrator and runs coding tasks.

mod automation;
mod connection;
mod git;
mod plan;
mod policy;
mod proc;
mod runner;
/// Public so the seatbelt and bubblewrap builders stay compiled, and tested,
/// on every platform.
pub mod sandbox;
mod validate;

/// Both renderers print a Codex failure the way they print Claude's, so the
/// one parser lives here rather than twice over.
pub use proc::codex_error;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lgtm_protocol::{Executor, WorkerInfo, WORKER_WS_PATH};
use tokio::sync::mpsc;
use tokio_tungstenite::Connector;

use crate::connection::Ctx;

/// Everything needed to start a worker; the caller (a CLI, usually) parses
/// flags and fills in defaults before calling [`run`].
#[derive(Clone, Debug)]
pub struct WorkerOptions {
    /// `ws://host:port` or `wss://host:port`, no path.
    pub orchestrator: String,
    pub token: String,
    pub name: String,
    pub data_dir: PathBuf,
    pub slots: u32,
    pub ephemeral: bool,
    pub max_tasks: u32,
    /// Extra CA to trust for wss.
    pub ca: Option<PathBuf>,
}

/// The default worker name: `COMPUTERNAME`, else `HOSTNAME`, else the system
/// hostname via the `hostname` command, else `"worker"`.
pub fn default_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            let output = std::process::Command::new("hostname").output().ok()?;
            output.status.success().then_some(())?;
            let name = String::from_utf8(output.stdout).ok()?;
            let name = name.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .unwrap_or_else(|| "worker".to_string())
}

/// `available_parallelism / 4`, minimum 1.
pub fn default_slots() -> u32 {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    (cpus / 4).max(1)
}

/// What this machine can run a task with: its os and arch, and every
/// toolchain binary a task might require that is on PATH.
pub fn detect_capabilities() -> Vec<String> {
    let toolchains = [
        "node",
        "bun",
        "npm",
        "pnpm",
        "cargo",
        "python3",
        "go",
        "docker",
        "gh",
        "java",
        "xcodebuild",
    ];
    let mut tags = vec![
        format!("os:{}", std::env::consts::OS),
        format!("arch:{}", std::env::consts::ARCH),
    ];
    tags.extend(
        toolchains
            .into_iter()
            .filter(|bin| which::which(bin).is_ok())
            .map(String::from),
    );
    tags
}

/// Runs the worker until it exits on purpose (ephemeral done -> `Ok(())`) or
/// the connector/CA fails to build (`Err`).
pub async fn run(opts: WorkerOptions) -> Result<()> {
    // ring and aws-lc-rs can both be linked; rustls will not guess.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let executors: Vec<Executor> = [Executor::Claude, Executor::Codex]
        .into_iter()
        .filter(|e| which::which(e.binary()).is_ok())
        .collect();
    let capabilities = detect_capabilities();
    tracing::info!(
        "worker {} in {} executors {executors:?} slots {} capabilities {capabilities:?}",
        opts.name,
        opts.data_dir.display(),
        opts.slots
    );

    let info = WorkerInfo {
        name: opts.name,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        executors,
        slots: opts.slots,
        ephemeral: opts.ephemeral,
        capabilities,
    };

    let link = connection::Link {
        url: format!("{}{WORKER_WS_PATH}", opts.orchestrator),
        token: opts.token.clone(),
        info,
        connector: opts.ca.as_deref().map(load_connector).transpose()?,
    };

    // One channel for the process lifetime: events emitted while disconnected
    // wait here and flush on the next connection.
    let (tx, rx) = mpsc::unbounded_channel();
    let ctx = Arc::new(Ctx::new(
        opts.data_dir,
        opts.orchestrator,
        opts.token,
        tx,
        opts.ephemeral,
        opts.max_tasks,
    ));

    tokio::spawn(shutdown(ctx.clone()));

    connection::run(link, ctx, rx).await
}

fn load_connector(path: &Path) -> Result<Connector> {
    let pem = std::fs::read(path).with_context(|| format!("read CA {}", path.display()))?;
    let count = connection::load_roots(&pem)?;
    tracing::info!(
        "trusting {count} extra CA certificate(s) from {}",
        path.display()
    );
    connection::ca_connector(&pem)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_capabilities_starts_with_os_and_arch() {
        let tags = detect_capabilities();
        assert_eq!(tags[0], format!("os:{}", std::env::consts::OS));
        assert_eq!(tags[1], format!("arch:{}", std::env::consts::ARCH));
    }
}
