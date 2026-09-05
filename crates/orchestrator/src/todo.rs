//! Promoting a todo into a task, and patching one's priority, assignee, or
//! blockers.

use lgtm_protocol::{Executor, Task, TaskId, TaskKind, TaskSpec, Todo, TodoPatch, TodoStatus};

use crate::state::State;

/// Where a promoted todo's task should run.
pub struct PromoteInto {
    pub base_branch: String,
    pub executor: Executor,
    pub runner: Option<String>,
    pub created_by: Option<String>,
}

/// Why `update_todo` refused a patch.
#[derive(Debug)]
pub enum UpdateTodoError {
    NotFound,
    UnknownBlocker(String),
    SelfBlocker,
    EmptyTitle,
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
        if let Some(blocker) = todo.blockers.iter().find(|blocker| {
            self.todos
                .get(blocker.as_str())
                .is_some_and(|b| b.status != TodoStatus::Done)
        }) {
            return Err(format!("todo is blocked by {blocker}"));
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
            reasoning_effort: None,
            allowed_hosts: Vec::new(),
            created_by: into.created_by,
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

    /// Applies the fields a `PATCH /todos/:id` body actually sent. Any status
    /// transition is allowed: a person may reopen a todo someone called done.
    pub fn update_todo(&mut self, id: &str, patch: TodoPatch) -> Result<Todo, UpdateTodoError> {
        if !self.todos.contains_key(id) {
            return Err(UpdateTodoError::NotFound);
        }
        // A todo with no title is unusable, so a patch may not empty one.
        let title = match patch.title.as_deref().map(str::trim) {
            Some("") => return Err(UpdateTodoError::EmptyTitle),
            title => title.map(str::to_string),
        };
        if let Some(blockers) = &patch.blockers {
            for blocker in blockers {
                if blocker == id {
                    return Err(UpdateTodoError::SelfBlocker);
                }
                if !self.todos.contains_key(blocker) {
                    return Err(UpdateTodoError::UnknownBlocker(blocker.clone()));
                }
            }
        }
        let todo = self.todos.get_mut(id).expect("checked above");
        if let Some(title) = title {
            todo.title = title;
        }
        if let Some(description) = patch.description {
            todo.description = description;
        }
        if let Some(status) = patch.status {
            todo.status = status;
        }
        if let Some(priority) = patch.priority {
            todo.priority = priority;
        }
        if let Some(assignee) = patch.assignee {
            todo.assignee = assignee;
        }
        if let Some(blockers) = patch.blockers {
            todo.blockers = blockers;
        }
        if let Some(tags) = patch.tags {
            todo.tags = tags;
        }
        Ok(todo.clone())
    }
}

#[cfg(test)]
#[path = "todo_tests.rs"]
mod tests;
