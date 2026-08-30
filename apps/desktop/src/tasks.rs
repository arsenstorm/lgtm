//! How the chrome reads a task list: grouped by repository, aged, coloured.

use crate::labels::status_label;
use crate::theme::Tokens;
use gpui::Hsla;
use lgtm_protocol::{BatchSource, GoalStatus, Task};
use std::collections::HashSet;
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

/// Tasks bucketed by repository slug, groups and rows newest-first, with every
/// task's children directly after it.
pub fn group_by_repo(tasks: &[Task]) -> Vec<(String, Vec<&Task>)> {
    let mut newest: Vec<&Task> = tasks.iter().collect();
    newest.sort_by_key(|task| std::cmp::Reverse(task.created_at));

    let mut groups: Vec<(String, Vec<&Task>)> = Vec::new();
    for task in newest {
        let slug = repo_slug(&task.spec.repository);
        match groups.iter_mut().find(|(name, _)| name == &slug) {
            Some((_, rows)) => rows.push(task),
            None => groups.push((slug, vec![task])),
        }
    }
    for (_, rows) in &mut groups {
        *rows = parents_first(rows);
    }
    groups
}

fn parents_first<'a>(rows: &[&'a Task]) -> Vec<&'a Task> {
    let present: HashSet<&str> = rows.iter().map(|task| task.id.as_str()).collect();
    let mut out = Vec::with_capacity(rows.len());
    for task in rows {
        // Emitted with its parent below; a child whose parent is in another
        // repository has nothing to hang under, so it stays top level.
        if task
            .spec
            .parent
            .as_deref()
            .is_some_and(|parent| present.contains(parent))
        {
            continue;
        }
        out.push(*task);
        push_children(task, rows, &mut out);
    }
    out
}

fn push_children<'a>(parent: &Task, rows: &[&'a Task], out: &mut Vec<&'a Task>) {
    if out.len() >= rows.len() {
        return;
    }
    for task in rows {
        if task.spec.parent.as_deref() == Some(parent.id.as_str()) {
            out.push(*task);
            push_children(task, rows, out);
        }
    }
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
    use lgtm_protocol::{Executor, TaskKind, TaskSpec, TaskStatus};

    fn task(id: &str, repo: &str, created_at: u64, parent: Option<&str>) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: repo.into(),
                base_branch: "main".into(),
                prompt: "p".into(),
                executor: Executor::Claude,
                runner: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: parent.map(String::from),
                depends_on: vec![],
                depends_on_condition: Default::default(),
                batch: None,
                sandbox: None,
                requirements: vec![],
                goal: None,
                review_executor: None,
                model: None,
                allowed_hosts: Vec::new(),
            },
            status: TaskStatus::Queued,
            runner: None,
            created_at,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            executions: Vec::new(),
            scratchpad: String::new(),
        }
    }

    #[test]
    fn slug_drops_host_and_git_suffix() {
        assert_eq!(repo_slug("https://github.com/you/repo.git"), "repo");
        assert_eq!(repo_slug("git@github.com:you/repo.git"), "repo");
        assert_eq!(repo_slug("https://github.com/you/repo/"), "repo");
        assert_eq!(repo_slug("repo"), "repo");
    }

    #[test]
    fn groups_are_newest_first_within_and_across_repositories() {
        let tasks = vec![
            task("a", "https://x/one.git", 10, None),
            task("b", "https://x/two.git", 30, None),
            task("c", "https://x/one.git", 20, None),
        ];
        let groups = group_by_repo(&tasks);
        assert_eq!(groups[0].0, "two");
        assert_eq!(groups[1].0, "one");
        let ids: Vec<&str> = groups[1].1.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "a"]);
    }

    #[test]
    fn children_follow_their_parent_even_when_newer() {
        let tasks = vec![
            task("parent", "https://x/one.git", 10, None),
            task("child", "https://x/one.git", 99, Some("parent")),
            task("other", "https://x/one.git", 50, None),
        ];
        let ids: Vec<&str> = group_by_repo(&tasks)[0]
            .1
            .iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(ids, vec!["other", "parent", "child"]);
    }

    #[test]
    fn orphan_child_stays_at_top_level() {
        let tasks = vec![task("child", "https://x/one.git", 1, Some("elsewhere"))];
        assert_eq!(group_by_repo(&tasks)[0].1.len(), 1);
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
