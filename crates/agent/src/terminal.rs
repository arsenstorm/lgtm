//! A shell in a task's worktree, owned by this runner so it outlives whoever
//! attached to it.
//
// This is a shell of its own, not the agent's session. Running the agent
// under `script` instead, so a person could watch it and take over, was
// measured and rejected: the pty echoes the EOF that closes stdin as a
// literal `^D\b\b` before the first byte of output, and ONLCR rewrites every
// `\n` to `\r\n`, so the opening line of `claude --output-format stream-json`
// and of `codex exec --json` stops parsing. `stty raw -echo` ahead of the
// exec fixes the newlines but loses the race for the prefix.
//
// ponytail: running the agent here needs the runner to own the pty — openpty
// FFI, `cfmakeraw` on the slave before exec. That fd is also the only way to
// set the window size, so until then there is nothing for a resize message
// to do and the terminal does not resize.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use lgtm_protocol::{RunnerMessage, TaskId};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};

use crate::connection::Ctx;
use crate::git::worktree_path;
use crate::sandbox;

/// One running shell. The child is kept so `close` can kill it.
pub struct Terminal {
    stdin: ChildStdin,
    child: Child,
}

const READ_SIZE: usize = 4096;

/// The argv that runs `shell` under a pty, or `None` where the system has no
/// `script`. Nothing in the dependency graph allocates a pty, and `script` is
/// the one allocator every unix already ships.
pub fn script_command(shell: &str) -> Option<Vec<String>> {
    let argv = if cfg!(target_os = "macos") {
        vec!["-q".to_string(), "/dev/null".to_string(), shell.to_string()]
    } else if cfg!(target_os = "linux") {
        vec![
            "-q".to_string(),
            "-c".to_string(),
            format!("{shell} -i"),
            "/dev/null".to_string(),
        ]
    } else {
        return None;
    };
    Some(std::iter::once("script".to_string()).chain(argv).collect())
}

/// Starts the task's shell, unless it already has one.
pub async fn open(task_id: TaskId, ctx: Arc<Ctx>) {
    if ctx.terminals.lock().await.contains_key(&task_id) {
        return;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let Some(argv) = script_command(&shell) else {
        tracing::warn!(task = %task_id, "unsupported: no pty on this platform");
        let _ = ctx.tx.send(RunnerMessage::TerminalClosed { task_id });
        return;
    };
    match spawn(&argv, &worktree_path(&ctx.data_dir, &task_id)) {
        Ok(child) => attach(task_id, child, ctx).await,
        Err(err) => {
            tracing::warn!(task = %task_id, "terminal: {err}");
            let _ = ctx.tx.send(RunnerMessage::TerminalClosed { task_id });
        }
    }
}

fn spawn(argv: &[String], worktree: &Path) -> std::io::Result<Child> {
    Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(worktree)
        .env_clear()
        .envs(std::env::vars().filter(|(name, _)| sandbox::keep_env(name)))
        .env("TERM", "xterm-256color")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

async fn attach(task_id: TaskId, mut child: Child, ctx: Arc<Ctx>) {
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return;
    };
    ctx.terminals
        .lock()
        .await
        .insert(task_id.clone(), Terminal { stdin, child });
    tokio::spawn(pump(task_id.clone(), stderr, ctx.clone()));
    tokio::spawn(until_eof(task_id, stdout, ctx));
}

/// Forwards output until the pipe ends.
// ponytail: a UTF-8 character split across two reads arrives as U+FFFD; hold
// the partial tail back if that ever shows up in practice.
async fn pump<R: AsyncRead + Unpin>(task_id: TaskId, mut out: R, ctx: Arc<Ctx>) {
    let mut buf = [0u8; READ_SIZE];
    while let Ok(read) = out.read(&mut buf).await {
        if read == 0 {
            return;
        }
        let _ = ctx.tx.send(RunnerMessage::Terminal {
            task_id: task_id.clone(),
            data: String::from_utf8_lossy(&buf[..read]).into_owned(),
        });
    }
}

async fn until_eof<R: AsyncRead + Unpin>(task_id: TaskId, out: R, ctx: Arc<Ctx>) {
    pump(task_id.clone(), out, ctx.clone()).await;
    close(&task_id, &ctx).await;
    let _ = ctx.tx.send(RunnerMessage::TerminalClosed { task_id });
}

/// Types `data` into the task's shell; a task with no shell ignores it.
pub async fn input(task_id: &str, data: &str, ctx: &Ctx) {
    let mut terminals = ctx.terminals.lock().await;
    let Some(terminal) = terminals.get_mut(task_id) else {
        return;
    };
    if let Err(err) = terminal.stdin.write_all(data.as_bytes()).await {
        tracing::warn!(task = %task_id, "terminal input: {err}");
        return;
    }
    let _ = terminal.stdin.flush().await;
}

/// Kills the task's shell. Closing the pty hangs up the shell inside it.
pub async fn close(task_id: &str, ctx: &Ctx) {
    let terminal = ctx.terminals.lock().await.remove(task_id);
    if let Some(mut terminal) = terminal {
        let _ = terminal.child.kill().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_wraps_the_shell_in_a_pty_the_way_this_platform_wants() {
        let argv = script_command("/bin/zsh");
        if cfg!(target_os = "macos") {
            assert_eq!(
                argv.unwrap(),
                ["script", "-q", "/dev/null", "/bin/zsh"].map(String::from)
            );
        } else if cfg!(target_os = "linux") {
            assert_eq!(
                argv.unwrap(),
                ["script", "-q", "-c", "/bin/zsh -i", "/dev/null"].map(String::from)
            );
        } else {
            assert!(argv.is_none());
        }
    }
}
