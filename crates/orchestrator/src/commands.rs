//! What a reviewer can do to a task: approve, reject, cancel, merge, message.

use lgtm_protocol::{OrchestratorMessage, Task, TaskEvent, TaskId, TaskStatus};

use crate::state::{CmdError, PrPlan, State, TITLE_MAX};

impl State {
    /// Shared guard for cancel/approve/reject: the task must exist, be in one
    /// of `allowed`, and its worker must still be connected.
    pub fn command(
        &mut self,
        task_id: &str,
        allowed: &[TaskStatus],
        wrong_status: &str,
        msg: impl FnOnce(TaskId) -> OrchestratorMessage,
    ) -> Result<Task, CmdError> {
        let rec = self.tasks.get(task_id).ok_or(CmdError::NotFound)?;
        if !allowed.contains(&rec.task.status) {
            return Err(CmdError::Conflict(wrong_status.to_string()));
        }
        let name = rec.task.worker.clone().unwrap_or_default();
        let worker = self
            .workers
            .get(&name)
            .filter(|worker| worker.is_connected())
            .ok_or_else(|| CmdError::Conflict(format!("worker {name} is not connected")))?;
        worker.send(msg(task_id.to_string()));
        Ok(rec.task.clone())
    }

    /// The pull request an approved task still needs, or `None` when there is
    /// nothing to open: GitHub is off, the task is not approved, it already
    /// has a pull request, or its repository is not on GitHub.
    pub fn pull_request_plan(&self, task_id: &str, github_enabled: bool) -> Option<PrPlan> {
        if !github_enabled {
            return None;
        }
        let rec = self.tasks.get(task_id)?;
        if rec.task.status != TaskStatus::Approved || rec.task.pull_request.is_some() {
            return None;
        }
        let spec = &rec.task.spec;
        let repo = lgtm_github::parse_repo(&spec.repository)?;
        let mut body = format!("{}\n\n", spec.prompt);
        if let Some(issue) = &spec.issue {
            body.push_str(&format!("Closes #{}\n\n", issue.number));
        }
        body.push_str(&format!("Created by LGTM task {task_id}"));
        let pull = lgtm_github::NewPull {
            repo,
            head: format!("lgtm/{task_id}"),
            base: spec.base_branch.clone(),
            title: spec
                .prompt
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .take(TITLE_MAX)
                .collect(),
            body,
        };
        Some(PrPlan {
            pull,
            sha: rec.pushed_sha().unwrap_or_default(),
        })
    }

    /// Returns the merged task and the ids to persist; merging can release
    /// tasks that were waiting on it.
    pub fn mark_merged(&mut self, task_id: &str) -> Result<(Task, Vec<TaskId>), CmdError> {
        let rec = self.tasks.get_mut(task_id).ok_or(CmdError::NotFound)?;
        if rec.task.status != TaskStatus::Approved {
            return Err(CmdError::Conflict("task is not approved".into()));
        }
        rec.task.status = TaskStatus::Merged;
        let task = rec.task.clone();
        tracing::info!(task = %task_id, "task merged");
        let mut changed = vec![task_id.to_string()];
        changed.extend(self.schedule());
        Ok((task, changed))
    }

    pub fn cancel(&mut self, task_id: &str) -> Result<Task, CmdError> {
        let rec = self.tasks.get(task_id).ok_or(CmdError::NotFound)?;
        // Nothing has been told to run it yet, so it ends here.
        if rec.task.status == TaskStatus::Queued && rec.task.worker.is_none() {
            self.apply_event(task_id, TaskEvent::Cancelled);
            return self
                .tasks
                .get(task_id)
                .map(|rec| rec.task.clone())
                .ok_or(CmdError::NotFound);
        }
        self.command(
            task_id,
            &[TaskStatus::Queued, TaskStatus::Running],
            "task is not running",
            |task_id| OrchestratorMessage::Cancel { task_id },
        )
    }

    /// Records a follow-up and hands it to the worker; the slot is taken again
    /// until the worker reports the run finished.
    pub fn message(
        &mut self,
        task_id: &str,
        text: String,
    ) -> Result<(Task, Vec<TaskId>), CmdError> {
        let rec = self.tasks.get(task_id).ok_or(CmdError::NotFound)?;
        if rec.task.status != TaskStatus::AwaitingReview {
            return Err(CmdError::Conflict("task is not awaiting review".into()));
        }
        let name = rec.task.worker.clone().unwrap_or_default();
        let connected = self
            .workers
            .get(&name)
            .is_some_and(|worker| worker.is_connected());
        if !connected {
            return Err(CmdError::Conflict(format!(
                "worker {name} is not connected"
            )));
        }
        let changed = self.apply_event(task_id, TaskEvent::Message { text: text.clone() });
        if let Some(worker) = self.workers.get_mut(&name) {
            worker.running.insert(task_id.to_string());
            worker.send(OrchestratorMessage::Message {
                task_id: task_id.to_string(),
                text,
            });
        }
        self.tasks
            .get(task_id)
            .map(|rec| (rec.task.clone(), changed))
            .ok_or(CmdError::NotFound)
    }
}
