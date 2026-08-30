//! Promoting a todo into a task.

use lgtm_protocol::{Executor, Task, TaskId, TaskKind, TaskSpec, TodoStatus};

use crate::state::State;

/// Where a promoted todo's task should run.
pub struct PromoteInto {
    pub base_branch: String,
    pub executor: Executor,
    pub runner: Option<String>,
}

fn wire_status(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Open => "open",
        TodoStatus::InProgress => "in_progress",
        TodoStatus::Done => "done",
    }
}

impl State {
    /// Turns an open todo into a queued task, and moves the todo to
    /// `InProgress` pointing at it. Returns the task and the ids to persist.
    pub fn promote_todo(
        &mut self,
        id: &str,
        into: PromoteInto,
    ) -> Result<(Task, Vec<TaskId>), String> {
        let todo = self
            .todos
            .get(id)
            .ok_or_else(|| "todo not found".to_string())?;
        if todo.status != TodoStatus::Open {
            return Err(format!("todo is already {}", wire_status(todo.status)));
        }
        let repository = todo
            .repository
            .clone()
            .ok_or_else(|| "todo has no repository".to_string())?;
        let prompt = if todo.description.is_empty() {
            todo.title.clone()
        } else {
            format!("{}\n\n{}", todo.title, todo.description)
        };
        let spec = TaskSpec {
            repository,
            base_branch: into.base_branch,
            prompt,
            executor: into.executor,
            runner: into.runner,
            issue: None,
            linear: None,
            kind: TaskKind::Run,
            parent: None,
            depends_on: Vec::new(),
            depends_on_condition: Default::default(),
            batch: None,
            sandbox: None,
            goal: None,
            review_executor: None,
            requirements: Vec::new(),
            model: None,
            allowed_hosts: Vec::new(),
        };
        let (task, changed) = self.create_task(spec)?;
        let todo = self
            .todos
            .get_mut(id)
            .ok_or_else(|| "todo not found".to_string())?;
        todo.task = Some(task.id.clone());
        todo.status = TodoStatus::InProgress;
        Ok((task, changed))
    }
}

#[cfg(test)]
#[path = "todo_tests.rs"]
mod tests;
