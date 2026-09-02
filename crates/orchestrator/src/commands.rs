//! What a reviewer can do to a task: approve, reject, cancel, merge, message,
//! retry.

use lgtm_protocol::{Executor, OrchestratorMessage, Task, TaskEvent, TaskId, TaskSpec, TaskStatus};

use crate::state::{CmdError, PrPlan, State, TaskRecord, TITLE_MAX};

/// Where a retried task should run. `None` keeps what the spec already says.
pub struct RetryInto {
    pub runner: Option<String>,
    pub executor: Option<Executor>,
}

/// What a follow-up to a conflicted task has to say before the developer's own
/// words: the agent is in the worktree and only it can resolve the rebase.
/// `None` for a task whose branch still applies.
fn conflict_prefix(rec: &TaskRecord) -> Option<String> {
    if rec.task.status != TaskStatus::Conflicted {
        return None;
    }
    rec.events
        .iter()
        .rev()
        .find_map(|stored| match &stored.event {
            TaskEvent::Conflicted { base, files } => Some(format!(
                "The branch conflicts with {base} on: {}. Rebase onto origin/{base}, \
                 resolve the conflicts, finish the rebase, then continue with: ",
                files.join(", ")
            )),
            _ => None,
        })
}

impl State {
    /// Shared guard for cancel/approve/reject: the task must exist, be in one
    /// of `allowed`, and its runner must still be connected.
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
        let name = rec.task.runner.clone().unwrap_or_default();
        let runner = self
            .runners
            .get(&name)
            .filter(|runner| runner.is_connected())
            .ok_or_else(|| CmdError::Conflict(format!("runner {name} is not connected")))?;
        runner.send(msg(task_id.to_string()));
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
        if rec.task.status == TaskStatus::Queued && rec.task.runner.is_none() {
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

    /// SIGINT before the kill, unlike `cancel`; only a task with a process to
    /// signal qualifies, so a queued task is refused rather than cancelled.
    pub fn interrupt(&mut self, task_id: &str) -> Result<Task, CmdError> {
        self.command(
            task_id,
            &[TaskStatus::Running],
            "task is not running",
            |task_id| OrchestratorMessage::Interrupt { task_id },
        )
    }

    /// Records a follow-up and hands it to the runner; the slot is taken again
    /// until the runner reports the run finished.
    pub fn message(
        &mut self,
        task_id: &str,
        text: String,
        by: Option<String>,
    ) -> Result<(Task, Vec<TaskId>), CmdError> {
        let rec = self.tasks.get(task_id).ok_or(CmdError::NotFound)?;
        if !matches!(
            rec.task.status,
            TaskStatus::AwaitingReview | TaskStatus::Conflicted
        ) {
            return Err(CmdError::Conflict("task is not awaiting review".into()));
        }
        let prefix = conflict_prefix(rec).unwrap_or_default();
        let name = rec.task.runner.clone().unwrap_or_default();
        let repository = rec.task.spec.repository.clone();
        let connected = self
            .runners
            .get(&name)
            .is_some_and(|runner| runner.is_connected());
        if !connected {
            return Err(CmdError::Conflict(format!(
                "runner {name} is not connected"
            )));
        }
        let instruction = format!("{prefix}{text}");
        let changed = self.apply_event(
            task_id,
            TaskEvent::Message {
                text,
                by: by.clone(),
            },
        );
        let memories = self.memories_for(&repository);
        // The runner's own copy of the task predates any spec change (e.g. an
        // allowed host) made since its last run, so the current one rides along.
        let task = self
            .tasks
            .get(task_id)
            .map(|rec| Box::new(rec.task.clone()));
        // Resolved again rather than remembered: a credential registered
        // since the task started is the one that should sign the follow-up.
        let authorship = task
            .as_ref()
            .map(|task| self.authorship(task))
            .unwrap_or_default();
        if let Some(runner) = self.runners.get_mut(&name) {
            runner.running.insert(task_id.to_string());
            runner.send(OrchestratorMessage::Message {
                task_id: task_id.to_string(),
                text: instruction,
                memories,
                task,
                authorship,
            });
        }
        self.tasks
            .get(task_id)
            .map(|rec| (rec.task.clone(), changed))
            .ok_or(CmdError::NotFound)
    }

    /// The spec a retry would run under: `into` applied over what the task
    /// already says, refused unless the task ended badly and could run again.
    fn retry_spec(&self, task_id: &str, into: RetryInto) -> Result<TaskSpec, CmdError> {
        let task = &self.tasks.get(task_id).ok_or(CmdError::NotFound)?.task;
        let status = task.status;
        if !matches!(
            status,
            TaskStatus::Failed
                | TaskStatus::TimedOut
                | TaskStatus::RunnerLost
                | TaskStatus::Cancelled
        ) {
            return Err(CmdError::Conflict(format!(
                "task cannot be retried from {status:?}"
            )));
        }
        let mut spec = task.spec.clone();
        spec.runner = into.runner.or(spec.runner);
        spec.executor = into.executor.unwrap_or(spec.executor);
        self.check_eligible(&spec).map_err(CmdError::Conflict)?;
        Ok(spec)
    }

    /// Adds `host` to the task's allowlist for its next run. Any status: the
    /// run that asked for it may already be over, and a retry or follow-up
    /// might not start for a while yet.
    pub fn allow_host(
        &mut self,
        task_id: &str,
        host: String,
    ) -> Result<(Task, Vec<TaskId>), CmdError> {
        let rec = self.tasks.get(task_id).ok_or(CmdError::NotFound)?;
        if rec.task.spec.allowed_hosts.contains(&host) {
            return Ok((rec.task.clone(), Vec::new()));
        }
        let changed = self.apply_event(task_id, TaskEvent::HostAllowed { host: host.clone() });
        let rec = self.tasks.get_mut(task_id).ok_or(CmdError::NotFound)?;
        rec.task.spec.allowed_hosts.push(host);
        Ok((rec.task.clone(), changed))
    }

    /// Puts a task that ended badly back in the queue as a fresh attempt.
    // The old runner may still hold a worktree for this id; the runner's
    // `add_worktree` replaces a stale one, so nothing has to be torn down.
    pub fn retry(
        &mut self,
        task_id: &str,
        into: RetryInto,
    ) -> Result<(Task, Vec<TaskId>), CmdError> {
        let spec = self.retry_spec(task_id, into)?;
        let event = TaskEvent::Requeued {
            runner: spec.runner.clone(),
            executor: spec.executor,
        };
        let mut changed = self.apply_event(task_id, event);
        let rec = self.tasks.get_mut(task_id).ok_or(CmdError::NotFound)?;
        rec.task.spec = spec;
        rec.task.status = TaskStatus::Queued;
        rec.task.runner = None;
        rec.task.error = None;
        tracing::info!(task = %task_id, "task requeued");
        changed.extend(self.schedule());
        self.tasks
            .get(task_id)
            .map(|rec| (rec.task.clone(), changed))
            .ok_or(CmdError::NotFound)
    }
}
