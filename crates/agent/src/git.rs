//! Git invocations and the path layout under the data directory.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use lgtm_protocol::TaskResult;
use tokio::process::Command;

/// Identity for commits made on behalf of the agent; the worker machine has no
/// git identity of its own.
pub const IDENTITY: [&str; 4] = ["-c", "user.name=lgtm", "-c", "user.email=lgtm@localhost"];

/// Runs git and returns trimmed stdout, or an error naming the command and its stderr.
pub async fn git(args: &[&str], cwd: Option<&Path>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let out = cmd
        .output()
        .await
        .with_context(|| format!("git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed ({}): {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `git diff --cached --quiet` exits 1 when something is staged.
pub async fn has_staged_changes(worktree: &Path) -> Result<bool> {
    let status = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(worktree)
        .status()
        .await
        .context("git diff --cached --quiet")?;
    match status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        other => bail!("git diff --cached --quiet exited with {other:?}"),
    }
}

pub fn slug(url: &str) -> String {
    let last = url.rsplit(['/', ':']).next().unwrap_or("");
    let last = last.strip_suffix(".git").unwrap_or(last);
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "repo".to_string()
    } else {
        cleaned
    }
}

pub fn mirror_path(data_dir: &Path, repository: &str) -> PathBuf {
    data_dir
        .join("repos")
        .join(format!("{}.git", slug(repository)))
}

pub fn worktree_path(data_dir: &Path, task_id: &str) -> PathBuf {
    data_dir.join("worktrees").join(task_id)
}

/// Agent session id of the last run, so a follow-up can resume it.
pub fn session_path(data_dir: &Path, task_id: &str) -> PathBuf {
    data_dir
        .join("worktrees")
        .join(format!("{task_id}.session"))
}

/// The task as sent by the orchestrator, so a follow-up survives a restart.
pub fn task_path(data_dir: &Path, task_id: &str) -> PathBuf {
    data_dir
        .join("worktrees")
        .join(format!("{task_id}.task.json"))
}

pub fn branch_name(task_id: &str) -> String {
    format!("lgtm/{task_id}")
}

/// Fetches into `refs/remotes/origin/*`: a mirror's `refs/heads/*` may be
/// checked out by a live worktree, which git refuses to fetch into.
pub async fn fetch(mirror: &Path) -> Result<()> {
    git(
        &[
            "-C",
            &mirror.display().to_string(),
            "fetch",
            "--prune",
            "origin",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        None,
    )
    .await?;
    Ok(())
}

/// Creates the task's worktree on a fresh branch, replacing any leftover from
/// an earlier run of the same task.
pub async fn add_worktree(
    mirror: &Path,
    worktree: &Path,
    branch: &str,
    base_branch: &str,
) -> Result<()> {
    if worktree.exists() {
        let _ = remove_worktree(mirror, worktree, branch).await;
        // `worktree remove` fails on a directory git never registered.
        let _ = tokio::fs::remove_dir_all(worktree).await;
    }
    git(
        &[
            "-C",
            &mirror.display().to_string(),
            "worktree",
            "add",
            "-b",
            branch,
            &worktree.display().to_string(),
            &format!("origin/{base_branch}"),
        ],
        None,
    )
    .await?;
    Ok(())
}

/// Removes the worktree and its branch, running both even when the first
/// fails so a half-registered worktree still loses its branch.
pub async fn remove_worktree(mirror: &Path, worktree: &Path, branch: &str) -> Result<()> {
    let mirror_s = mirror.display().to_string();
    let worktree_s = worktree.display().to_string();
    let removed = git(
        &[
            "-C",
            &mirror_s,
            "worktree",
            "remove",
            "--force",
            &worktree_s,
        ],
        None,
    )
    .await;
    let deleted = git(&["-C", &mirror_s, "branch", "-D", branch], None).await;
    removed.and(deleted).map(|_| ())
}

/// Commits whatever the agent left in the worktree and describes the change.
pub async fn commit(
    prompt: &str,
    base_branch: &str,
    branch: &str,
    worktree: &Path,
) -> Result<TaskResult> {
    let cwd = Some(worktree);
    git(&["add", "-A"], cwd).await?;
    if has_staged_changes(worktree).await? {
        let message = commit_message(prompt);
        let mut args = IDENTITY.to_vec();
        args.extend_from_slice(&["commit", "-q", "-m", &message]);
        git(&args, cwd).await?;
    }
    let range = format!("origin/{base_branch}...{branch}");
    let diff = git(&["diff", &range], cwd).await?;
    let names = git(&["diff", "--name-only", &range], cwd).await?;
    Ok(TaskResult {
        branch: branch.to_string(),
        diff,
        changed_files: names.lines().map(str::to_string).collect(),
        validation: Vec::new(),
        plan: None,
        review: None,
        policy: None,
        cost_usd: 0.0,
    })
}

pub fn commit_message(prompt: &str) -> String {
    let first: String = prompt
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(72)
        .collect();
    format!("lgtm: {first}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_repository_urls() {
        assert_eq!(slug("https://github.com/arsenstorm/lgtm.git"), "lgtm");
        assert_eq!(slug("git@github.com:arsenstorm/olos.git"), "olos");
        assert_eq!(slug(""), "repo");
        assert_eq!(slug("https://x/y/we ird.git"), "we-ird");
    }

    #[test]
    fn commit_message_is_first_line_capped_at_72() {
        assert_eq!(commit_message("do a thing\nand more"), "lgtm: do a thing");
        let long = "x".repeat(100);
        assert_eq!(commit_message(&long), format!("lgtm: {}", "x".repeat(72)));
    }
}
