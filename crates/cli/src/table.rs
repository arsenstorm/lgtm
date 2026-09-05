//! The `tasks` table and the cells in it.

use std::collections::HashMap;

use lgtm_protocol::{
    CiState, GoalSummary, Memory, Review, Skill, Task, TaskStatus, Todo, Verification,
};

/// The wire form ("awaiting_review") rather than Rust's Debug form, so a
/// cell matches the JSON everywhere else in the CLI's output.
pub(crate) fn wire_str(value: impl serde::Serialize) -> String {
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
    if task.status == TaskStatus::Queued && task.runner.is_none() && has_unmet_deps {
        "blocked".to_string()
    } else {
        status_str(task.status)
    }
}

/// Prints the `tasks`-style table: `ID STATUS RUNNER PR PROMPT`, children
/// ordered after their parent. Shared by `tasks` and `backlog status`.
pub fn print_task_table(tasks: Vec<Task>) {
    println!(
        "{:<10}{:<16}{:<16}{:<10}PROMPT",
        "ID", "STATUS", "RUNNER", "PR"
    );
    for t in order_tasks(tasks.clone()) {
        let runner = t.runner.as_deref().unwrap_or("-");
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
            t.id, status, runner, pr, prompt
        );
    }
}

/// One row of the `memory list` table. A memory with no repository shows `*`,
/// since it applies to all of them. `BY` is the task id that proposed it, or
/// `-` for one a person added directly.
pub fn memory_row(memory: &Memory) -> String {
    let state = match memory.verification {
        Verification::UserApproved => "approved",
        Verification::AgentProposed => "proposed",
    };
    format!(
        "{:<10}{:<10}{:<10}{:<48}{}",
        memory.id,
        state,
        memory.proposed_by.as_deref().unwrap_or("-"),
        memory.repository.as_deref().unwrap_or("*"),
        first_line_truncated(&memory.content, 80)
    )
}

pub fn print_memory_table(memories: &[Memory]) {
    println!(
        "{:<10}{:<10}{:<10}{:<48}CONTENT",
        "ID", "STATE", "BY", "REPOSITORY"
    );
    for memory in memories {
        println!("{}", memory_row(memory));
    }
}

/// One row of the `skill list` table, shaped like `memory_row`: the name
/// stands in for the content, since that is what the agent sees first.
pub fn skill_row(skill: &Skill) -> String {
    let state = match skill.verification {
        Verification::UserApproved => "approved",
        Verification::AgentProposed => "proposed",
    };
    format!(
        "{:<10}{:<10}{:<10}{:<48}{:<24}{}",
        skill.id,
        state,
        skill.proposed_by.as_deref().unwrap_or("-"),
        skill.repository.as_deref().unwrap_or("*"),
        first_line_truncated(&skill.name, 22),
        first_line_truncated(&skill.description, 60)
    )
}

pub fn print_skill_table(skills: &[Skill]) {
    println!(
        "{:<10}{:<10}{:<10}{:<48}{:<24}DESCRIPTION",
        "ID", "STATE", "BY", "REPOSITORY", "NAME"
    );
    for skill in skills {
        println!("{}", skill_row(skill));
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

/// One row of the `todo list` table. A todo with no repository shows `*`;
/// STATUS shows `blocked` (derived from `all`) in place of the wire status.
pub fn todo_row(todo: &Todo, all: &HashMap<String, Todo>) -> String {
    let status = if todo.is_blocked(all) {
        "blocked".to_string()
    } else {
        wire_str(todo.status)
    };
    format!(
        "{:<10}{:<14}{:<6}{:<10}{:<48}{}",
        todo.id,
        status,
        wire_str(todo.priority),
        todo.assignee.as_deref().unwrap_or("-"),
        todo.repository.as_deref().unwrap_or("*"),
        first_line_truncated(&todo.title, 60)
    )
}

pub fn print_todo_table(todos: &[Todo]) {
    let all: HashMap<String, Todo> = todos.iter().map(|t| (t.id.clone(), t.clone())).collect();
    println!(
        "{:<10}{:<14}{:<6}{:<10}{:<48}TITLE",
        "ID", "STATUS", "PRI", "BY", "REPOSITORY"
    );
    for todo in todos {
        println!("{}", todo_row(todo, &all));
    }
}

/// `MEM` cell for the `runners` table: whole gigabytes, nearest, `?` when
/// the runner never reported (0, before it upgraded past phase one).
pub fn mem_gb_cell(memory_mb: u64) -> String {
    if memory_mb == 0 {
        return "?".to_string();
    }
    format!("{} GB", (memory_mb + 512) / 1024)
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
            title: None,
            spec: TaskSpec {
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                base_branch: "main".into(),
                prompt: "add a /health endpoint".into(),
                executor: Executor::Claude,
                runner: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: vec![],
                depends_on_condition: Default::default(),
                batch: None,
                sandbox: None,
                requirements: vec![],
                goal: None,
                review_executor: None,
                model: None,
                reasoning_effort: None,
                allowed_hosts: Vec::new(),
                created_by: None,
            },
            status: TaskStatus::Approved,
            runner: None,
            created_at: 1,
            result: None,
            error: None,
            pull_request,
            ci,
            pr_review: None,
            executions: Vec::new(),
            scratchpad: String::new(),
            files: Vec::new(),
            workspace: None,
            created_by: None,
            archived: false,
        }
    }

    /// A minimal task for `order_tasks`/`display_status` tests, where only
    /// id, status, runner, parent, and dependencies matter.
    fn task(
        id: &str,
        status: TaskStatus,
        runner: Option<&str>,
        parent: Option<&str>,
        depends_on: &[&str],
    ) -> Task {
        let mut t = sample_task(None, None);
        t.id = id.into();
        t.status = status;
        t.runner = runner.map(String::from);
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
            source: lgtm_protocol::MemorySource::User,
            verification: Verification::UserApproved,
            proposed_by: None,
            workspace: None,
            created_by: None,
        }
    }

    #[test]
    fn memory_row_stars_every_repository_and_truncates() {
        let row = memory_row(&memory(None, &"x".repeat(100)));
        assert!(row.starts_with("0123abcd  approved  -         *"));
        assert!(row.ends_with(&"x".repeat(80)));
        assert_eq!(row.len(), 10 + 10 + 10 + 48 + 80);
    }

    #[test]
    fn memory_row_shows_its_repository() {
        let row = memory_row(&memory(Some("https://example.com/r.git"), "no yarn"));
        assert!(row.contains("https://example.com/r.git"));
        assert!(row.ends_with("no yarn"));
    }

    #[test]
    fn memory_row_shows_proposed_state_and_task() {
        let mut memory = memory(None, "no yarn");
        memory.verification = Verification::AgentProposed;
        memory.proposed_by = Some("t1".into());
        let row = memory_row(&memory);
        assert!(row.starts_with("0123abcd  proposed  t1        "));
    }

    fn skill(name: &str, description: &str) -> Skill {
        Skill {
            id: "0123abcd".into(),
            name: name.into(),
            description: description.into(),
            repository: None,
            content: String::new(),
            files: Vec::new(),
            revision: 1,
            created_at: 1,
            updated_at: 1,
            source: lgtm_protocol::MemorySource::User,
            verification: Verification::UserApproved,
            proposed_by: None,
            workspace: None,
            created_by: None,
        }
    }

    #[test]
    fn skill_row_stars_every_repository_and_truncates_the_name() {
        let row = skill_row(&skill(&"a".repeat(40), "reviews a PR before it merges"));
        assert!(row.starts_with("0123abcd  approved  -         *"));
        assert!(row.contains(&"a".repeat(22)));
        assert!(!row.contains(&"a".repeat(23)));
        assert!(row.ends_with("reviews a PR before it merges"));
    }

    #[test]
    fn skill_row_shows_proposed_state_and_task() {
        let mut skill = skill("review", "reviews a PR before it merges");
        skill.verification = Verification::AgentProposed;
        skill.proposed_by = Some("t1".into());
        let row = skill_row(&skill);
        assert!(row.starts_with("0123abcd  proposed  t1        "));
    }

    fn todo(id: &str, blockers: Vec<String>) -> Todo {
        Todo {
            id: id.into(),
            repository: None,
            number: 1,
            title: "x".repeat(100),
            description: String::new(),
            status: lgtm_protocol::TodoStatus::Open,
            created_at: 1,
            task: None,
            priority: lgtm_protocol::Priority::Medium,
            assignee: None,
            blockers,
            tags: Vec::new(),
            workspace: None,
            created_by: None,
        }
    }

    #[test]
    fn todo_row_stars_no_repository_and_truncates() {
        let row = todo_row(&todo("0123abcd", Vec::new()), &HashMap::new());
        assert!(row.starts_with("0123abcd  open          medium-         *"));
        assert!(row.ends_with(&"x".repeat(60)));
    }

    #[test]
    fn todo_row_shows_priority_and_assignee() {
        let mut t = todo("0123abcd", Vec::new());
        t.priority = lgtm_protocol::Priority::High;
        t.assignee = Some("arsen".into());
        let row = todo_row(&t, &HashMap::new());
        assert!(row.starts_with("0123abcd  open          high  arsen     *"));
    }

    #[test]
    fn todo_row_shows_blocked_in_place_of_status() {
        let blocker = todo("blocker1", Vec::new());
        let t = todo("0123abcd", vec![blocker.id.clone()]);
        let mut all = HashMap::new();
        all.insert(blocker.id.clone(), blocker);
        let row = todo_row(&t, &all);
        assert!(row.starts_with("0123abcd  blocked       medium-         *"));
    }

    #[test]
    fn goal_row_shows_status_and_task_count() {
        let summary = GoalSummary {
            goal: lgtm_protocol::Goal {
                id: "0123abcd".into(),
                objective: "ship the health endpoint\nand the docs".into(),
                repository: "https://github.com/arsenstorm/lgtm.git".into(),
                created_at: 1,
                attention: None,
                workspace: None,
                created_by: None,
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

    #[test]
    fn mem_gb_cell_rounds_to_the_nearest_gigabyte() {
        assert_eq!(mem_gb_cell(32_768), "32 GB");
        assert_eq!(mem_gb_cell(1_500), "1 GB");
        assert_eq!(mem_gb_cell(0), "?");
    }
}
