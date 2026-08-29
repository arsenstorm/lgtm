//! The words the chrome uses for a task: its preview and its display status.

use lgtm_protocol::{GoalStatus, Task, TaskStatus};

const HEADER_PREVIEW: usize = 80;

/// First line of the prompt, truncated to `limit` characters.
pub fn prompt_preview(prompt: &str, limit: usize) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    if line.chars().count() > limit {
        format!("{}…", line.chars().take(limit).collect::<String>())
    } else {
        line.to_string()
    }
}

pub fn header_preview(prompt: &str) -> String {
    prompt_preview(prompt, HEADER_PREVIEW)
}

/// Display status for a task. Queued tasks waiting on unmet dependencies show
/// as `blocked` instead of `queued` (display only, doesn't affect `status`).
pub fn status_label(task: &Task, tasks: &[Task]) -> &'static str {
    if task.status == TaskStatus::Queued && task.runner.is_none() && is_blocked(task, tasks) {
        return "blocked";
    }
    match task.status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::AwaitingReview => "awaiting_review",
        TaskStatus::ChangesRequested => "changes_requested",
        TaskStatus::Conflicted => "conflicted",
        TaskStatus::Approved => "approved",
        TaskStatus::Merged => "merged",
        TaskStatus::Rejected => "rejected",
        TaskStatus::Failed => "failed",
        TaskStatus::TimedOut => "timed_out",
        TaskStatus::RunnerLost => "runner_lost",
        TaskStatus::Cancelled => "cancelled",
    }
}

pub fn goal_status_label(status: GoalStatus) -> &'static str {
    match status {
        GoalStatus::Draft => "draft",
        GoalStatus::Planning => "planning",
        GoalStatus::Running => "running",
        GoalStatus::Review => "in review",
        GoalStatus::Blocked => "blocked",
        GoalStatus::Completed => "completed",
        GoalStatus::Cancelled => "cancelled",
    }
}

/// True when `task` depends on another task that isn't yet approved/merged.
/// Dependencies absent from `tasks` don't block (nothing known to wait on).
fn is_blocked(task: &Task, tasks: &[Task]) -> bool {
    !task.spec.depends_on.is_empty()
        && !task.spec.depends_on.iter().all(|dep_id| {
            tasks
                .iter()
                .find(|t| &t.id == dep_id)
                .is_none_or(|t| matches!(t.status, TaskStatus::Approved | TaskStatus::Merged))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec};

    fn task(id: &str, status: TaskStatus, depends_on: Vec<&str>) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: "r".into(),
                base_branch: "main".into(),
                prompt: "p".into(),
                executor: Executor::Claude,
                runner: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: depends_on.into_iter().map(String::from).collect(),
                depends_on_condition: Default::default(),
                batch: None,
                sandbox: None,
                requirements: vec![],
                goal: None,
                review_executor: None,
                model: None,
                allowed_hosts: Vec::new(),
            },
            status,
            runner: None,
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            executions: Vec::new(),
            scratchpad: String::new(),
        }
    }

    #[test]
    fn queued_task_with_unmet_dependency_is_blocked() {
        let dep = task("dep", TaskStatus::Running, vec![]);
        let queued = task("q", TaskStatus::Queued, vec!["dep"]);
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "blocked");
    }

    #[test]
    fn queued_task_with_approved_dependency_is_queued() {
        let dep = task("dep", TaskStatus::Approved, vec![]);
        let queued = task("q", TaskStatus::Queued, vec!["dep"]);
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "queued");
    }

    #[test]
    fn assigned_runner_is_not_blocked_even_with_unmet_dependency() {
        let dep = task("dep", TaskStatus::Running, vec![]);
        let mut queued = task("q", TaskStatus::Queued, vec!["dep"]);
        queued.runner = Some("compute".into());
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "queued");
    }

    #[test]
    fn prompt_preview_truncates_to_the_first_line() {
        assert_eq!(prompt_preview("one\ntwo", 32), "one");
        assert_eq!(prompt_preview("abcdef", 3), "abc…");
    }
}
