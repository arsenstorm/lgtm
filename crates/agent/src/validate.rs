//! Repository-defined checks, declared in `<worktree>/.lgtm/config.toml` and
//! run after the agent finished.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lgtm_protocol::ValidationResult;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

pub const TAIL_LINES: usize = 50;
const TIMEOUT: Duration = Duration::from_secs(600);

type Lines = Arc<Mutex<Vec<String>>>;

/// Last `TAIL_LINES` lines, joined. Output is for a human reading a failure.
pub fn tail(lines: &[String]) -> String {
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

pub fn load_validation(worktree: &Path) -> Vec<(String, String)> {
    let path = worktree.join(".lgtm").join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => parse_validation(&text),
        Err(_) => Vec::new(),
    }
}

fn parse_validation(text: &str) -> Vec<(String, String)> {
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(err) => {
            tracing::warn!(".lgtm/config.toml: {err}");
            return Vec::new();
        }
    };
    let Some(section) = table.get("validation").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    section
        .iter()
        .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
        .collect()
}

pub async fn run_validation(worktree: &Path, checks: &[(String, String)]) -> Vec<ValidationResult> {
    let mut results = Vec::with_capacity(checks.len());
    for (name, command) in checks {
        let result = run_check(worktree, name, command).await;
        tracing::info!(
            "validation {name}: {}",
            if result.ok { "ok" } else { "failed" }
        );
        results.push(result);
    }
    results
}

async fn run_check(worktree: &Path, name: &str, command: &str) -> ValidationResult {
    let done = |ok, output_tail| ValidationResult {
        name: name.to_string(),
        command: command.to_string(),
        ok,
        output_tail,
    };
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut child = match Command::new(shell)
        .args([flag, command])
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(err) => return done(false, format!("spawn {shell}: {err}")),
    };

    let lines: Lines = Arc::new(Mutex::new(Vec::new()));
    let out = child
        .stdout
        .take()
        .map(|pipe| tokio::spawn(collect(pipe, lines.clone())));
    let err = child
        .stderr
        .take()
        .map(|pipe| tokio::spawn(collect(pipe, lines.clone())));

    let mut ok = false;
    let mut timed_out = false;
    match tokio::time::timeout(TIMEOUT, child.wait()).await {
        Ok(Ok(status)) => ok = status.success(),
        Ok(Err(err)) => push(&lines, format!("wait failed: {err}")),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            timed_out = true;
        }
    }
    if let Some(out) = out {
        let _ = out.await;
    }
    if let Some(err) = err {
        let _ = err.await;
    }

    let mut lines = std::mem::take(&mut *lines.lock().expect("validation lines poisoned"));
    if timed_out {
        lines.push("timed out after 600s".to_string());
    }
    done(ok, tail(&lines))
}

async fn collect<R: AsyncRead + Unpin>(reader: R, lines: Lines) {
    let mut read = BufReader::new(reader).lines();
    while let Ok(Some(line)) = read.next_line().await {
        push(&lines, line);
    }
}

fn push(lines: &Lines, line: String) {
    lines.lock().expect("validation lines poisoned").push(line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checks_in_file_order() {
        let checks = parse_validation("[validation]\ntest = \"bun test\"\ncheck = \"tsc\"\n");
        assert_eq!(
            checks,
            vec![
                ("test".to_string(), "bun test".to_string()),
                ("check".to_string(), "tsc".to_string()),
            ]
        );
    }

    #[test]
    fn ignores_missing_section_and_non_strings() {
        assert!(parse_validation("[other]\nx = 1\n").is_empty());
        assert!(parse_validation("not toml at all = =").is_empty());
        let checks = parse_validation("[validation]\ntest = 3\nlint = \"biome\"\n");
        assert_eq!(checks, vec![("lint".to_string(), "biome".to_string())]);
    }

    #[test]
    fn tail_keeps_the_last_fifty_lines() {
        let lines: Vec<String> = (0..60).map(|n| n.to_string()).collect();
        let kept = tail(&lines);
        assert_eq!(kept.lines().count(), TAIL_LINES);
        assert!(kept.starts_with("10\n"));
        assert!(kept.ends_with("\n59"));
        assert_eq!(tail(&lines[..3]), "0\n1\n2");
    }
}
