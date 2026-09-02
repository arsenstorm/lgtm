//! lgtm runner agent: connects out to the orchestrator and runs coding tasks.

mod artefacts;
mod automation;
mod connection;
mod git;
mod plan;
mod policy;
mod proc;
mod proxy;
mod runner;
/// Public so the seatbelt and bubblewrap builders stay compiled, and tested,
/// on every platform.
pub mod sandbox;
pub mod terminal;
mod validate;

/// Both renderers print a Codex failure the way they print Claude's, so the
/// one parser lives here rather than twice over.
pub use proc::codex_error;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lgtm_protocol::{Executor, RunnerInfo, RUNNER_WS_PATH};
use tokio::sync::mpsc;
use tokio_tungstenite::Connector;

use crate::connection::Ctx;

/// Everything needed to start a runner; the caller (a CLI, usually) parses
/// flags and fills in defaults before calling [`run`].
#[derive(Clone, Debug)]
pub struct RunnerOptions {
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

/// The default runner name: `COMPUTERNAME`, else `HOSTNAME`, else the system
/// hostname via the `hostname` command, else `"runner"`.
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
        .unwrap_or_else(|| "runner".to_string())
}

/// `available_parallelism / 4`, minimum 1.
pub fn default_slots() -> u32 {
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    (cpus / 4).max(1)
}

/// CPU cores and total physical memory (MB), so a task can require a minimum
/// of either. 0 for whichever the platform call fails to report.
pub fn detect_resources() -> (u32, u64) {
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get() as u32);
    (cores, detect_memory_mb())
}

#[cfg(target_os = "macos")]
fn detect_memory_mb() -> u64 {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output();
    out.ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        })
        .map_or(0, |bytes| bytes / (1024 * 1024))
}

#[cfg(target_os = "linux")]
fn detect_memory_mb() -> u64 {
    let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    // "MemTotal:       16384000 kB"
    meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|kb| kb.parse::<u64>().ok())
        .map_or(0, |kb| kb / 1024)
}

/// `wmic` used to answer this, but Windows 11 24H2 dropped it, so a runner
/// there reported 0 MB and matched no task with a memory requirement. This is
/// the same number `MemTotal` gives on Linux: what the OS has to hand out.
#[cfg(windows)]
fn detect_memory_mb() -> u64 {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // The only documented failure is a malformed `length`, which is set above.
    match unsafe { GlobalMemoryStatusEx(&mut status) } {
        0 => 0,
        _ => status.total_phys / (1024 * 1024),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn detect_memory_mb() -> u64 {
    0
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

/// Which harnesses this machine can run, PATH probed once at startup.
pub fn detect_executors() -> Vec<Executor> {
    [Executor::Claude, Executor::Codex]
        .into_iter()
        .filter(|e| which::which(e.binary()).is_ok())
        .collect()
}

/// Runs the runner until it exits on purpose (ephemeral done -> `Ok(())`) or
/// the connector/CA fails to build (`Err`).
pub async fn run(opts: RunnerOptions) -> Result<()> {
    // ring and aws-lc-rs can both be linked; rustls will not guess.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let executors = detect_executors();
    let capabilities = detect_capabilities();
    let (cpu_cores, memory_mb) = detect_resources();
    tracing::info!(
        "runner {} in {} executors {executors:?} slots {} capabilities {capabilities:?} \
         cpu_cores {cpu_cores} memory_mb {memory_mb}",
        opts.name,
        opts.data_dir.display(),
        opts.slots
    );

    let info = RunnerInfo {
        name: opts.name,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        executors,
        slots: opts.slots,
        ephemeral: opts.ephemeral,
        capabilities,
        cpu_cores,
        memory_mb,
    };

    let link = connection::Link {
        url: format!("{}{RUNNER_WS_PATH}", opts.orchestrator),
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

    #[test]
    fn detect_resources_finds_at_least_one_core() {
        let (cores, _) = detect_resources();
        assert!(cores >= 1);
    }
}
