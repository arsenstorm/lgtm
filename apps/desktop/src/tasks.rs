//! How the chrome reads a task: its repository, its age, its colour.

use crate::labels::status_label;
use crate::theme::Tokens;
use gpui::Hsla;
use lgtm_protocol::{BatchSource, GoalStatus, Task};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `https://github.com/you/repo.git` -> `repo`. Also handles scp-style git
/// URLs, whose last `/` segment is the repository too.
pub fn repo_slug(repository: &str) -> String {
    let last = repository
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(repository);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

/// Coarse age, one unit only: `12s`, `5m`, `2h`, `3d`.
pub fn relative_age(created_at: u64, now: u64) -> String {
    duration(now.saturating_sub(created_at))
}

/// A span of milliseconds in that same one coarse unit.
pub fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86_399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

pub fn goal_color(status: GoalStatus, t: &Tokens) -> Hsla {
    match status {
        GoalStatus::Running | GoalStatus::Planning => t.info,
        GoalStatus::Review => t.warning,
        GoalStatus::Blocked => t.danger,
        GoalStatus::Completed => t.success,
        GoalStatus::Cancelled | GoalStatus::Draft => t.muted_fg,
    }
}

pub fn status_color(task: &Task, tasks: &[Task], t: &Tokens) -> Hsla {
    match status_label(task, tasks) {
        "awaiting_review" | "conflicted" => t.warning,
        "running" => t.info,
        "approved" | "merged" => t.success,
        "failed" | "timed_out" | "runner_lost" | "rejected" | "cancelled" => t.danger,
        _ => t.muted_fg,
    }
}

/// `o/r label:L` for a GitHub label batch, `TEAM/STATE` for a Linear batch.
pub fn batch_label(source: &BatchSource) -> String {
    match source {
        BatchSource::GithubLabel { owner, repo, label } => format!("{owner}/{repo} label:{label}"),
        BatchSource::Linear { team, state } => format!("{team}/{state}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_drops_host_and_git_suffix() {
        assert_eq!(repo_slug("https://github.com/you/repo.git"), "repo");
        assert_eq!(repo_slug("git@github.com:you/repo.git"), "repo");
        assert_eq!(repo_slug("https://github.com/you/repo/"), "repo");
        assert_eq!(repo_slug("repo"), "repo");
    }

    #[test]
    fn age_uses_one_coarse_unit() {
        let now = 10_000_000_000;
        assert_eq!(relative_age(now - 12_000, now), "12s");
        assert_eq!(relative_age(now - 5 * 60_000, now), "5m");
        assert_eq!(relative_age(now - 2 * 3_600_000, now), "2h");
        assert_eq!(relative_age(now - 3 * 86_400_000, now), "3d");
        assert_eq!(relative_age(now + 1000, now), "0s");
    }

    #[test]
    fn batch_label_formats_both_sources() {
        assert_eq!(
            batch_label(&BatchSource::GithubLabel {
                owner: "o".into(),
                repo: "r".into(),
                label: "L".into()
            }),
            "o/r label:L"
        );
        assert_eq!(
            batch_label(&BatchSource::Linear {
                team: "TEAM".into(),
                state: "STATE".into()
            }),
            "TEAM/STATE"
        );
    }
}
