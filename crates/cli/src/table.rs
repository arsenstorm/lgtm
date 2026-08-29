//! The `tasks` table and the cells in it.

use lgtm_protocol::{CiState, GoalSummary, Memory, Review, Task, TaskStatus, Todo};

/// The wire form ("awaiting_review") rather than Rust's Debug form, so a
/// cell matches the JSON everywhere else in the CLI's output.
fn wire_str(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

pub fn status_str(status: TaskStatus) -> String {
    wire_str(status)
}

pub fn ci_str(state: CiState) -> String {
    wire_str(state)
}

/// `#<pr-number> <mark>` for the `tasks` table's PR column, empty when the
/// task has no pull request. Mark is ✓/✗ for CI success/failure, … for
/// pending or missing CI info.
pub fn pr_cell(task: &Task) -> String {
    let Some(pr) = &task.pull_request else {
        return String::new();
    };
    let mark = match task.ci.as_ref().map(|ci| ci.state) {
        Some(CiState::Success) => "✓",
        Some(CiState::Failure) => "✗",
        Some(CiState::Pending) | None => "…",
    };
    format!("#{} {mark}", pr.number)
}

/// Reorders `tasks` for the `tasks` table so each child (`spec.parent`
/// `Some`) is listed right after its parent. Top-level tasks (and parents)
/// keep their existing relative order; a task's own children keep their
/// existing relative order too. A task whose declared parent isn't in the
/// list is treated as top-level.
pub fn order_tasks(tasks: Vec<Task>) -> Vec<Task> {
    let ids: std::collections::HashSet<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let mut children: std::collections::HashMap<String, Vec<Task>> =
        std::collections::HashMap::new();
    let mut top_level: Vec<Task> = Vec::new();
    for task in tasks {
        match &task.spec.parent {
            Some(parent_id) if ids.contains(parent_id.as_str()) => {
                children.entry(parent_id.clone()).or_default().push(task);
            }
            _ => top_level.push(task),
        }
    }
    let mut ordered = Vec::with_capacity(top_level.len());
    for task in top_level {
        let id = task.id.clone();
        ordered.push(task);
        if let Some(kids) = children.remove(&id) {
            ordered.extend(kids);
        }
    }
    ordered
}

/// STATUS cell for the `tasks` table: `display_status` only, not the raw
/// wire status. A queued, unassigned task with unmet dependencies shows
/// `blocked` instead of `queued`, so the table hints at why it isn't
/// running yet.
pub fn display_status(task: &Task, all: &[Task]) -> String {
    let has_unmet_deps = !task.spec.depends_on.is_empty()
        && !task.spec.depends_on.iter().all(|dep_id| {
            all.iter().any(|t| {
                &t.id == dep_id && matches!(t.status, TaskStatus::Approved | TaskStatus::Merged)
            })
        });
    if task.status == TaskStatus::Queued && task.worker.is_none() && has_unmet_deps {
        "blocked".to_string()
    } else {
        status_str(task.status)
    }
}

/// Prints the `tasks`-style table: `ID STATUS WORKER PR PROMPT`, children
/// ordered after their parent. Shared by `tasks` and `backlog status`.
pub fn print_task_table(tasks: Vec<Task>) {
    println!(
        "{:<10}{:<16}{:<16}{:<10}PROMPT",
        "ID", "STATUS", "WORKER", "PR"
    );
    for t in order_tasks(tasks.clone()) {
        let worker = t.worker.as_deref().unwrap_or("-");
        let prefix = if t.spec.parent.is_some() { "↳ " } else { "" };
        let prompt = format!("{prefix}{}", first_line_truncated(&t.spec.prompt, 60));
        let failed = t.result.as_ref().is_some_and(|r| {
            r.validation_failed() || r.review.as_ref().is_some_and(Review::has_blocking)
        });
        let status = display_status(&t, &tasks);
        let status = if failed { format!("{status}!") } else { status };
        let pr = pr_cell(&t);
        println!(
            "{:<10}{:<16}{:<16}{:<10}{}",
            t.id, status, worker, pr, prompt
        );
    }
}

/// One row of the `memory list` table. A memory with no repository shows `*`,
/// since it applies to all of them.
pub fn memory_row(memory: &Memory) -> String {
    format!(
        "{:<10}{:<48}{}",
        memory.id,
        memory.repository.as_deref().unwrap_or("*"),
        first_line_truncated(&memory.content, 80)
    )
}

pub fn print_memory_table(memories: &[Memory]) {
    println!("{:<10}{:<48}CONTENT", "ID", "REPOSITORY");
    for memory in memories {
        println!("{}", memory_row(memory));
    }
}

/// One row of the `goals` table: `ID STATUS TASKS OBJECTIVE`.
pub fn goal_row(summary: &GoalSummary) -> String {
    format!(
        "{:<10}{:<12}{:<7}{}",
        summary.goal.id,
        wire_str(summary.status),
        summary.tasks.total(),
        first_line_truncated(&summary.goal.objective, 60)
    )
}

pub fn print_goal_table(goals: Vec<GoalSummary>) {
    println!("{:<10}{:<12}{:<7}OBJECTIVE", "ID", "STATUS", "TASKS");
    for summary in &goals {
        println!("{}", goal_row(summary));
    }
}

/// One row of the `todo list` table. A todo with no repository shows `*`.
pub fn todo_row(todo: &Todo) -> String {
    format!(
        "{:<10}{:<14}{:<48}{}",
        todo.id,
        wire_str(todo.status),
        todo.repository.as_deref().unwrap_or("*"),
        first_line_truncated(&todo.title, 60)
    )
}

pub fn print_todo_table(todos: &[Todo]) {
    println!("{:<10}{:<14}{:<48}TITLE", "ID", "STATUS", "REPOSITORY");
    for todo in todos {
        println!("{}", todo_row(todo));
    }
}

pub fn first_line_truncated(s: &str, max: usize) -> String {
    let first = s.lines().next().unwrap_or("");
    if first.chars().count() > max {
        first.chars().take(max).collect()
    } else {
        first.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{CiStatus, Executor, PullRequest, TaskKind, TaskSpec};

    fn sample_task(pull_request: Option<PullRequest>, ci: Option<CiStatus>) -> Task {
        Task {
            id: "0123abcd".into(),
            spec: TaskSpec {
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                base_branch: "main".into(),
                prompt: "add a /health endpoint".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: vec![],
                batch: None,
                sandbox: None,
                requirements: vec![],
                goal: None,
            },
            status: TaskStatus::Approved,
            worker: None,
            created_at: 1,
            result: None,
            error: None,
            pull_request,
            ci,
            executions: Vec::new(),
        }
    }

    /// A minimal task for `order_tasks`/`display_status` tests, where only
    /// id, status, worker, parent, and dependencies matter.
    fn task(
        id: &str,
        status: TaskStatus,
        worker: Option<&str>,
        parent: Option<&str>,
        depends_on: &[&str],
    ) -> Task {
        let mut t = sample_task(None, None);
        t.id = id.into();
        t.status = status;
        t.worker = worker.map(String::from);
        t.spec.parent = parent.map(String::from);
        t.spec.depends_on = depends_on.iter().map(|s| s.to_string()).collect();
        t
    }

    fn memory(repository: Option<&str>, content: &str) -> Memory {
        Memory {
            id: "0123abcd".into(),
            repository: repository.map(String::from),
            content: content.into(),
            created_at: 1,
        }
    }

    #[test]
    fn memory_row_stars_every_repository_and_truncates() {
        let row = memory_row(&memory(None, &"x".repeat(100)));
        assert!(row.starts_with("0123abcd  *         "));
        assert!(row.ends_with(&"x".repeat(80)));
        assert_eq!(row.len(), 10 + 48 + 80);
    }

    #[test]
    fn memory_row_shows_its_repository() {
        let row = memory_row(&memory(Some("https://example.com/r.git"), "no yarn"));
        assert!(row.contains("https://example.com/r.git"));
        assert!(row.ends_with("no yarn"));
    }

    #[test]
    fn todo_row_stars_no_repository_and_truncates() {
        let todo = Todo {
            id: "0123abcd".into(),
            repository: None,
            title: "x".repeat(100),
            description: String::new(),
            status: lgtm_protocol::TodoStatus::Open,
            created_at: 1,
            task: None,
        };
        let row = todo_row(&todo);
        assert!(row.starts_with("0123abcd  open          *"));
        assert!(row.ends_with(&"x".repeat(60)));
    }

    #[test]
    fn goal_row_shows_status_and_task_count() {
        let summary = GoalSummary {
            goal: lgtm_protocol::Goal {
                id: "0123abcd".into(),
                objective: "ship the health endpoint\nand the docs".into(),
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                created_at: 1,
            },
            status: lgtm_protocol::GoalStatus::Running,
            tasks: lgtm_protocol::BatchSummary {
                running: 1,
                queued: 1,
                ..lgtm_protocol::BatchSummary::default()
            },
        };
        assert_eq!(
            goal_row(&summary),
            "0123abcd  running     2      ship the health endpoint"
        );
    }

    #[test]
    fn pr_cell_empty_without_pull_request() {
        assert_eq!(pr_cell(&sample_task(None, None)), "");
    }

    #[test]
    fn pr_cell_pending_without_ci() {
        let pr = PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        };
        assert_eq!(pr_cell(&sample_task(Some(pr), None)), "#12 …");
    }

    #[test]
    fn pr_cell_marks_ci_success() {
        let pr = PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        };
        let ci = CiStatus {
            state: CiState::Success,
            url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
        };
        assert_eq!(pr_cell(&sample_task(Some(pr), Some(ci))), "#12 ✓");
    }

    #[test]
    fn pr_cell_marks_ci_failure() {
        let pr = PullRequest {
            number: 12,
            url: "https://github.com/arsenstorm/lgtm/pull/12".into(),
        };
        let ci = CiStatus {
            state: CiState::Failure,
            url: "https://github.com/arsenstorm/lgtm/pull/12/checks".into(),
        };
        assert_eq!(pr_cell(&sample_task(Some(pr), Some(ci))), "#12 ✗");
    }

    #[test]
    fn order_tasks_places_children_after_parent() {
        let p = task("p", TaskStatus::Queued, None, None, &[]);
        let q = task("q", TaskStatus::Queued, None, None, &[]);
        let c1 = task("c1", TaskStatus::Queued, None, Some("p"), &[]);
        let c2 = task("c2", TaskStatus::Queued, None, Some("p"), &[]);
        let ordered = order_tasks(vec![p, q, c1, c2]);
        let ids: Vec<&str> = ordered.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["p", "c1", "c2", "q"]);
    }

    #[test]
    fn display_status_blocks_queued_task_with_unmet_dependency() {
        let dep = task("d", TaskStatus::Queued, None, None, &[]);
        let t = task("t", TaskStatus::Queued, None, None, &["d"]);
        assert_eq!(display_status(&t, &[dep, t.clone()]), "blocked");
    }

    #[test]
    fn display_status_queued_once_dependency_is_approved() {
        let dep = task("d", TaskStatus::Approved, None, None, &[]);
        let t = task("t", TaskStatus::Queued, None, None, &["d"]);
        assert_eq!(display_status(&t, &[dep, t.clone()]), "queued");
    }
}
