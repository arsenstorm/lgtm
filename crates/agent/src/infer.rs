//! One-shot model calls the orchestrator asks for: a prompt in, the model's
//! text out. No worktree, no checks, no commit machinery — an inference call
//! is not a task and never becomes one.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use lgtm_protocol::{Executor, RunnerMessage};
use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::connection::Ctx;
use crate::proc::{capture_answer, final_text, text_buffer};

/// Hard cap. The orchestrator waits 120s, so a child killed here still leaves
/// time for the failure to reach the caller it kept waiting.
const TIMEOUT: Duration = Duration::from_secs(90);

/// Inference is a guest on this runner: at most two at once, the rest queue
/// here, so a burst of utility calls never takes CPU or a model's rate limit
/// away from the task slots.
static SLOTS: Semaphore = Semaphore::const_new(2);

/// Runs one call and answers, whatever happens: an `Infer` that never gets a
/// reply wastes the orchestrator's whole timeout.
pub async fn run(id: String, executor: Executor, system: String, prompt: String, ctx: Arc<Ctx>) {
    let _permit = SLOTS.acquire().await;
    let (text, error) = match one_shot(executor, &system, &prompt).await {
        Ok(text) => (text, None),
        Err(err) => (String::new(), Some(format!("{err:#}"))),
    };
    if let Some(error) = &error {
        tracing::warn!(%id, %error, "inference failed");
    }
    let _ = ctx.tx.send(RunnerMessage::Inferred { id, text, error });
}

async fn one_shot(executor: Executor, system: &str, prompt: &str) -> Result<String> {
    let binary = executor.binary();
    let path = which::which(binary).with_context(|| format!("{binary} not found on PATH"))?;
    let mut cmd = Command::new(&path);
    cmd.args(args(executor, system, prompt))
        // A scratch directory: the call has no repository of its own to touch.
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(TIMEOUT, cmd.output())
        .await
        .map_err(|_| anyhow!("{binary} did not answer in {}s", TIMEOUT.as_secs()))?
        .with_context(|| format!("spawn {}", path.display()))?;
    let text = text_buffer();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        capture_answer(line, &text, executor);
    }
    let answer = final_text(&text);
    if answer.trim().is_empty() {
        bail!("{binary} answered nothing, exit {}", output.status);
    }
    Ok(answer)
}

/// The same output shapes an agent run produces, so [`capture_answer`] reads
/// the answer back. Codex `exec` has no system-prompt flag, so the
/// instructions lead the prompt there.
fn args(executor: Executor, system: &str, prompt: &str) -> Vec<String> {
    match executor {
        Executor::Claude => vec![
            "-p".into(),
            prompt.into(),
            "--append-system-prompt".into(),
            system.into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
        ],
        Executor::Codex => vec![
            "exec".into(),
            "--json".into(),
            "--sandbox".into(),
            "read-only".into(),
            format!("{system}\n\n{prompt}"),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_executors_carry_the_system_instructions() {
        let claude = args(Executor::Claude, "be brief", "fix login");
        assert_eq!(claude[0], "-p");
        assert_eq!(claude[1], "fix login");
        assert!(claude.contains(&"--append-system-prompt".to_string()));
        assert!(claude.contains(&"be brief".to_string()));

        let codex = args(Executor::Codex, "be brief", "fix login");
        assert_eq!(codex[0], "exec");
        assert_eq!(codex.last().unwrap(), "be brief\n\nfix login");
    }
}
