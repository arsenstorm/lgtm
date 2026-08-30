//! Git invocations and the path layout under the data directory.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use lgtm_protocol::TaskResult;
use tokio::process::Command;

/// Identity for commits made on behalf of the agent; the runner machine has no
/// git identity of its own.
pub const IDENTITY: [&str; 4] = ["-c", "user.name=lgtm", "-c", "user.email=lgtm@localhost"];

/// The agent's working notes, relative to the worktree.
pub const SCRATCHPAD: &str = ".lgtm/scratchpad.md";

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
        .with_context(|| format!("git {}", redact(args)))?;
    if !out.status.success() {
        bail!(
            "git {} failed ({}): {}",
            redact(args),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The push token rides in a `-c http.extraheader=...` argument; an error
/// naming the failed command must not repeat it.
fn redact(args: &[&str]) -> String {
    args.iter()
        .map(|arg| {
            if arg.starts_with("http.extraheader=") {
                "http.extraheader=REDACTED"
            } else {
                arg
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A `-c http.extraheader=...` pair authorizing one push or fetch, or empty
/// to leave it to git's own credential helper.
pub fn auth_args(token: Option<&str>) -> Vec<String> {
    let Some(token) = token else {
        return Vec::new();
    };
    let basic = base64_encode(format!("x-access-token:{token}").as_bytes());
    vec![
        "-c".to_string(),
        format!("http.extraheader=AUTHORIZATION: basic {basic}"),
    ]
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// No crate in the workspace speaks base64; the alphabet is small enough to
/// spell out here.
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        let chars = [
            BASE64_ALPHABET[(n >> 18 & 0x3f) as usize],
            BASE64_ALPHABET[(n >> 12 & 0x3f) as usize],
            BASE64_ALPHABET[(n >> 6 & 0x3f) as usize],
            BASE64_ALPHABET[(n & 0x3f) as usize],
        ];
        out.push(chars[0] as char);
        out.push(chars[1] as char);
        out.push(if chunk.len() > 1 {
            chars[2] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            chars[3] as char
        } else {
            '='
        });
    }
    out
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

/// Brings the worktree's branch up to date with `base`. The outer `Err` is
/// git itself failing; `Ok(Err(files))` is a conflict, already aborted, with
/// the paths that stopped the rebase. `token` authorizes the fetch on a
/// private repository.
pub async fn rebase_onto(
    worktree: &Path,
    base: &str,
    token: Option<&str>,
) -> Result<Result<(), Vec<String>>> {
    let worktree_s = worktree.display().to_string();
    // The mirror is a bare clone with no fetch refspec of its own, so
    // `origin/<base>` only moves when the refspec is spelled out.
    let refspec = format!("+refs/heads/{base}:refs/remotes/origin/{base}");
    let auth = auth_args(token);
    let mut fetch_args: Vec<&str> = auth.iter().map(String::as_str).collect();
    fetch_args.extend_from_slice(&["-C", &worktree_s, "fetch", "origin", &refspec]);
    git(&fetch_args, None).await?;
    let onto = format!("origin/{base}");
    let mut args = IDENTITY.to_vec();
    args.extend_from_slice(&["-C", &worktree_s, "rebase", &onto]);
    if git(&args, None).await.is_ok() {
        return Ok(Ok(()));
    }
    let unmerged = git(
        &["-C", &worktree_s, "diff", "--name-only", "--diff-filter=U"],
        None,
    )
    .await?;
    let files = conflicted_files(&unmerged);
    git(&["-C", &worktree_s, "rebase", "--abort"], None).await?;
    Ok(Err(files))
}

fn conflicted_files(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
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
    exclude(worktree, SCRATCHPAD).await?;
    Ok(())
}

/// Keeps `pattern` out of every commit made in the worktree. The mirror's
/// exclude file is shared by its worktrees, so the line is written once.
pub async fn exclude(worktree: &Path, pattern: &str) -> Result<()> {
    let worktree_s = worktree.display().to_string();
    let found = git(
        &["-C", &worktree_s, "rev-parse", "--git-path", "info/exclude"],
        None,
    )
    .await?;
    let path = worktree.join(found);
    let current = tokio::fs::read_to_string(&path).await.unwrap_or_default();
    let Some(text) = with_line(&current, pattern) else {
        return Ok(());
    };
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    tokio::fs::write(&path, text)
        .await
        .with_context(|| format!("write {}", path.display()))
}

/// The file with `line` appended, or `None` when it is already there.
fn with_line(current: &str, line: &str) -> Option<String> {
    if current.lines().any(|existing| existing == line) {
        return None;
    }
    let sep = if current.is_empty() || current.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    Some(format!("{current}{sep}{line}\n"))
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
    fn exclude_writes_its_line_once() {
        assert_eq!(with_line("", SCRATCHPAD).unwrap(), ".lgtm/scratchpad.md\n");
        assert_eq!(
            with_line("*.log", SCRATCHPAD).unwrap(),
            "*.log\n.lgtm/scratchpad.md\n"
        );
        assert_eq!(
            with_line("*.log\n", SCRATCHPAD).unwrap(),
            "*.log\n.lgtm/scratchpad.md\n"
        );
        assert_eq!(with_line("*.log\n.lgtm/scratchpad.md\n", SCRATCHPAD), None);
    }

    #[test]
    fn conflicted_files_are_one_per_line() {
        assert_eq!(
            conflicted_files("src/a.rs\nsrc/b.rs\n"),
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
        assert!(conflicted_files("").is_empty());
        assert!(conflicted_files("\n \n").is_empty());
    }

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
        assert_eq!(base64_encode(b"Ma"), "TWE=");
        assert_eq!(base64_encode(b"M"), "TQ==");
    }

    #[test]
    fn auth_args_are_empty_without_a_token() {
        assert!(auth_args(None).is_empty());
        let args = auth_args(Some("t"));
        assert_eq!(args[0], "-c");
        assert!(args[1].starts_with("http.extraheader=AUTHORIZATION: basic "));
    }

    #[test]
    fn redact_hides_the_auth_header() {
        let header = "http.extraheader=AUTHORIZATION: basic secret".to_string();
        let args = ["-c", &header, "push"];
        let out = redact(&args);
        assert!(!out.contains("secret"), "{out}");
        assert!(out.contains("http.extraheader=REDACTED"));
    }

    #[test]
    fn commit_message_is_first_line_capped_at_72() {
        assert_eq!(commit_message("do a thing\nand more"), "lgtm: do a thing");
        let long = "x".repeat(100);
        assert_eq!(commit_message(&long), format!("lgtm: {}", "x".repeat(72)));
    }
}
