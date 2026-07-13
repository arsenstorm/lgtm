use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;

use crate::error::AppError;

pub struct ExecLimits {
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
}

impl Default for ExecLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_stdout_bytes: 10 * 1024 * 1024,
        }
    }
}

pub struct GitOutput {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl GitOutput {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }
}

const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Run git with args, never through a shell. Returns regardless of exit status.
pub async fn run_git(repo_dir: &Path, args: &[&str]) -> Result<GitOutput, AppError> {
    run_git_with_limits(repo_dir, args, &ExecLimits::default()).await
}

pub async fn run_git_with_limits(
    repo_dir: &Path,
    args: &[&str],
    limits: &ExecLimits,
) -> Result<GitOutput, AppError> {
    run_git_with_env(repo_dir, args, limits, &[]).await
}

pub async fn run_git_with_env(
    repo_dir: &Path,
    args: &[&str],
    limits: &ExecLimits,
    extra_env: &[(String, String)],
) -> Result<GitOutput, AppError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C")
        .arg(repo_dir)
        .args(args)
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_EXTERNAL_DIFF")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    for (key, value) in extra_env {
        cmd.env(key, value);
    }

    let command_label = format!("git {}", args.first().copied().unwrap_or(""));
    run_command_with_limits(cmd, command_label, limits).await
}

/// Shared spawn/read/timeout logic, generalized over the program so it can be
/// exercised in tests with a reliably-blocking binary (see git/exec tests).
async fn run_command_with_limits(
    mut cmd: tokio::process::Command,
    command_label: String,
    limits: &ExecLimits,
) -> Result<GitOutput, AppError> {
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::GitUnavailable
        } else {
            AppError::Internal {
                message: format!("failed to spawn git: {e}"),
            }
        }
    })?;

    let mut stdout_pipe = child.stdout.take().expect("stdout piped");
    let mut stderr_pipe = child.stderr.take().expect("stderr piped");
    let max = limits.max_stdout_bytes;

    let work = async {
        // Read stdout up to max+1 bytes to detect overflow, draining concurrently
        // with stderr to avoid pipe deadlock.
        let stdout_fut = async {
            let mut stdout = Vec::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = stdout_pipe.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                if stdout.len() + n > max + 1 {
                    stdout.extend_from_slice(&buf[..(max + 1 - stdout.len())]);
                    break;
                }
                stdout.extend_from_slice(&buf[..n]);
            }
            Ok::<Vec<u8>, std::io::Error>(stdout)
        };
        let stderr_fut = async {
            let mut stderr = Vec::new();
            let mut buf = vec![0u8; 8 * 1024];
            loop {
                let n = stderr_pipe.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                if stderr.len() < MAX_STDERR_BYTES {
                    stderr.extend_from_slice(&buf[..n.min(MAX_STDERR_BYTES - stderr.len())]);
                }
            }
            Ok::<Vec<u8>, std::io::Error>(stderr)
        };
        let (stdout_res, stderr_res) = tokio::join!(stdout_fut, stderr_fut);
        let stdout = stdout_res.map_err(|e| AppError::Internal {
            message: format!("read git stdout: {e}"),
        })?;
        let stderr = stderr_res.map_err(|e| AppError::Internal {
            message: format!("read git stderr: {e}"),
        })?;
        let status = child.wait().await.map_err(|e| AppError::Internal {
            message: format!("wait for git: {e}"),
        })?;
        Ok::<GitOutput, AppError>(GitOutput {
            status: status.code(),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    };

    let output = match tokio::time::timeout(limits.timeout, work).await {
        Ok(result) => result?,
        Err(_) => {
            return Err(AppError::GitTimeout {
                command: command_label,
            });
        }
    };

    if output.stdout.len() > max {
        return Err(AppError::DiffTooLarge);
    }
    Ok(output)
}

/// Like run_git but errors when git exits non-zero.
pub async fn run_git_ok(repo_dir: &Path, args: &[&str]) -> Result<GitOutput, AppError> {
    let out = run_git(repo_dir, args).await?;
    if !out.ok() {
        return Err(AppError::GitCommandFailed {
            command: format!("git {}", args.join(" ")),
            status: out.status,
            stderr: out.stderr.chars().take(2000).collect(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stdout_cap_triggers_diff_too_large() {
        let dir = std::env::temp_dir();
        let limits = ExecLimits {
            timeout: Duration::from_secs(5),
            max_stdout_bytes: 16,
        };
        // `git --version --build-options` reliably prints more than 16 bytes.
        let result = run_git_with_limits(&dir, &["--version", "--build-options"], &limits).await;
        assert!(matches!(result, Err(AppError::DiffTooLarge)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_blocking_process() {
        let mut cmd = tokio::process::Command::new("/bin/sleep");
        cmd.args(["5"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let limits = ExecLimits {
            timeout: Duration::from_millis(50),
            max_stdout_bytes: 1024,
        };
        let result = run_command_with_limits(cmd, "sleep 5".to_string(), &limits).await;
        assert!(matches!(result, Err(AppError::GitTimeout { .. })));
    }

    #[tokio::test]
    async fn git_command_failed_carries_stderr_and_label() {
        let repo = crate::test_support::init_repo();
        let dir = repo.path();
        let result = run_git_ok(dir, &["rev-parse", "--verify", "not-a-real-ref"]).await;
        let Err(AppError::GitCommandFailed {
            command, stderr, ..
        }) = result
        else {
            panic!("expected GitCommandFailed");
        };
        assert_eq!(command, "git rev-parse --verify not-a-real-ref");
        assert!(!stderr.is_empty());
    }
}
