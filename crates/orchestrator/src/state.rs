//! Shared state and every status transition, kept free of I/O so it can be
//! tested without sockets or files.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use lgtm_protocol::{
    Batch, OrchestratorMessage, StoredEvent, Task, TaskEvent, TaskId, TaskSpec, TaskStatus,
};
use tokio::sync::{broadcast, mpsc};

use crate::persist::{Persist, Stored};
pub use crate::worker::{Conn, WorkerConn};

const LIVE_CAPACITY: usize = 1024;

pub struct App {
    pub token: String,
    pub state: Mutex<State>,
    /// Writes go to a task that owns the directory; a request handler never
    /// holds a path of its own.
    pub persist: mpsc::UnboundedSender<Persist>,
    /// `None` when no `GITHUB_TOKEN` was set, which turns the pull request,
    /// CI and merge routes off.
    pub github: Option<lgtm_github::GitHub>,
    /// `None` when no `LINEAR_API_KEY` was set, which turns the from-linear
    /// route and every issue sync off.
    pub linear: Option<lgtm_linear::Linear>,
}

impl App {
    /// Queues a write for each id a transition reported as changed.
    pub fn persist_ids(&self, state: &State, ids: &[TaskId]) {
        for rec in ids.iter().filter_map(|id| state.tasks.get(id)) {
            let _ = self
                .persist
                .send(Persist::Task(Box::new(Stored::from(rec))));
        }
    }

    pub fn persist_batch(&self, batch: &Batch) {
        let _ = self.persist.send(Persist::Batch(batch.clone()));
    }
}

#[derive(Default)]
pub struct State {
    pub workers: HashMap<String, WorkerConn>,
    pub tasks: HashMap<TaskId, TaskRecord>,
    /// Backlog imports, by id. A task points back with `spec.batch`.
    pub batches: HashMap<String, Batch>,
    /// Accept tasks no connected worker can run, because provisioning is on
    /// and a worker for them is a queue away.
    pub queue_without_workers: bool,
}

pub struct TaskRecord {
    pub task: Task,
    pub events: Vec<StoredEvent>,
    pub live: broadcast::Sender<StoredEvent>,
}

impl TaskRecord {
    pub fn new(task: Task, events: Vec<StoredEvent>) -> Self {
        let (live, _) = broadcast::channel(LIVE_CAPACITY);
        Self { task, events, live }
    }

    /// Head of the pushed branch, from the last `Pushed` event that carried
    /// one. Workers before phase 5 pushed without a sha.
    pub fn pushed_sha(&self) -> Option<String> {
        self.events
            .iter()
            .rev()
            .find_map(|stored| match &stored.event {
                TaskEvent::Pushed { sha, .. } if !sha.is_empty() => Some(sha.clone()),
                _ => None,
            })
    }
}

/// Everything needed to open one pull request, resolved under the lock so the
/// GitHub call itself can run without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrPlan {
    pub pull: lgtm_github::NewPull,
    /// Head sha to poll checks for, empty when the worker did not report one.
    pub sha: String,
}

pub(crate) const TITLE_MAX: usize = 72;

/// One thing to tell Linear about a task.
#[derive(Debug, PartialEq, Eq)]
pub enum LinearSync {
    Move(lgtm_linear::Target),
    Comment(String),
}

/// What to tell Linear after `task` moved from `previous` to its current
/// status (or after a pull request was recorded when `pr_recorded`).
pub fn linear_sync_plan(task: &Task, previous: TaskStatus, pr_recorded: bool) -> Vec<LinearSync> {
    if task.spec.linear.is_none() {
        return Vec::new();
    }
    let mut plan = Vec::new();
    match task.status {
        // A follow-up run moves the issue back out of review.
        TaskStatus::Running if previous != TaskStatus::Running => {
            plan.push(LinearSync::Move(lgtm_linear::Target::Started));
        }
        TaskStatus::AwaitingReview if previous == TaskStatus::Running => {
            plan.push(LinearSync::Move(lgtm_linear::Target::InReview));
        }
        TaskStatus::Merged => plan.push(LinearSync::Move(lgtm_linear::Target::Completed)),
        _ => {}
    }
    if let Some(pull) = task.pull_request.as_ref().filter(|_| pr_recorded) {
        plan.push(LinearSync::Comment(format!("Pull request: {}", pull.url)));
    }
    plan
}

/// Why a command against a task could not run.
#[derive(Debug)]
pub enum CmdError {
    NotFound,
    Conflict(String),
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Eight lowercase hex characters, which `persist::file_stem` accepts.
fn random_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

impl State {
    pub(crate) fn new_id(&self) -> TaskId {
        std::iter::repeat_with(random_id)
            .find(|id| !self.tasks.contains_key(id))
            .unwrap_or_default()
    }

    pub(crate) fn new_batch_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.batches.contains_key(id))
            .unwrap_or_default()
    }

    /// Whether a task could ever run, so one that cannot is refused instead of
    /// queued forever. `Err` holds the 409 message.
    pub fn check_eligible(&self, spec: &TaskSpec) -> Result<(), String> {
        for id in &spec.depends_on {
            if !self.tasks.contains_key(id) {
                return Err(format!("unknown dependency {id}"));
            }
        }
        if let Some(name) = &spec.worker {
            let worker = self
                .workers
                .get(name)
                .filter(|worker| worker.is_connected())
                .ok_or_else(|| format!("worker {name} is not connected"))?;
            if !worker.info.executors.contains(&spec.executor) {
                let executor = spec.executor.binary();
                return Err(format!("worker {name} does not have {executor}"));
            }
            return Ok(());
        }
        let any = self.queue_without_workers
            || self.workers.values().any(|worker| {
                worker.is_connected() && worker.info.executors.contains(&spec.executor)
            });
        any.then_some(()).ok_or_else(|| "no eligible worker".into())
    }

    /// Connected worker with the most free slots that can run `spec`, ties
    /// broken by the lowest name.
    fn candidate(&self, spec: &TaskSpec) -> Option<String> {
        self.workers
            .values()
            .filter(|worker| worker.can_run(spec) && spec.pins(&worker.info.name))
            .max_by(|a, b| {
                a.free_slots()
                    .cmp(&b.free_slots())
                    .then_with(|| b.info.name.cmp(&a.info.name))
            })
            .map(|worker| worker.info.name.clone())
    }

    /// Assigns unassigned queued tasks, oldest first, and starts them.
    /// Returns the ids it assigned.
    /// Queued, unassigned, and not waiting on any dependency.
    pub fn is_ready(&self, task: &Task) -> bool {
        task.status == TaskStatus::Queued && task.worker.is_none() && self.deps_met(&task.spec)
    }

    pub fn schedule(&mut self) -> Vec<TaskId> {
        let mut queued: Vec<(u64, TaskId)> = self
            .tasks
            .values()
            .filter(|rec| self.is_ready(&rec.task))
            .map(|rec| (rec.task.created_at, rec.task.id.clone()))
            .collect();
        queued.sort();
        let mut assigned = Vec::new();
        for (_, id) in queued {
            let Some(name) = self
                .tasks
                .get(&id)
                .and_then(|rec| self.candidate(&rec.task.spec))
            else {
                continue;
            };
            let Some(rec) = self.tasks.get_mut(&id) else {
                continue;
            };
            rec.task.worker = Some(name.clone());
            let task = rec.task.clone();
            if let Some(worker) = self.workers.get_mut(&name) {
                worker.running.insert(id.clone());
                worker.send(OrchestratorMessage::Start {
                    task: Box::new(task),
                });
            }
            tracing::info!(task = %id, worker = %name, "task assigned");
            assigned.push(id);
        }
        assigned
    }

    /// Queues a task and schedules it. Returns the task and the ids to persist.
    pub fn create_task(&mut self, spec: TaskSpec) -> Result<(Task, Vec<TaskId>), String> {
        self.check_eligible(&spec)?;
        let task = Task {
            id: self.new_id(),
            spec,
            status: TaskStatus::Queued,
            worker: None,
            created_at: now_ms(),
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            executions: Vec::new(),
        };
        let id = task.id.clone();
        tracing::info!(task = %id, "task created");
        self.tasks
            .insert(id.clone(), TaskRecord::new(task, Vec::new()));
        let mut changed = vec![id.clone()];
        changed.extend(self.schedule());
        Ok((self.tasks[&id].task.clone(), changed))
    }

    pub(crate) fn fail_unfinished(&mut self, id: &TaskId, error: &str) -> Vec<TaskId> {
        match self.tasks.get(id) {
            Some(rec) if !rec.task.status.is_terminal() => self.apply_event(
                id,
                TaskEvent::Failed {
                    error: error.into(),
                },
            ),
            _ => Vec::new(),
        }
    }

    /// Records a worker event, applies its status transition, and reschedules
    /// if it freed a slot. Returns the ids to persist.
    pub fn apply_event(&mut self, task_id: &str, event: TaskEvent) -> Vec<TaskId> {
        let Some(rec) = self.tasks.get_mut(task_id) else {
            tracing::warn!(task = %task_id, "event for unknown task, ignoring");
            return Vec::new();
        };
        let stored = StoredEvent {
            at: now_ms(),
            event,
        };
        rec.events.push(stored.clone());
        let terminal = rec.task.status.is_terminal();
        crate::execution::record(&mut rec.task, &stored.event, stored.at);
        let finished = transition(&mut rec.task, &stored.event);
        let status = rec.task.status;
        let worker = rec.task.worker.clone();
        let _ = rec.live.send(stored);
        tracing::debug!(task = %task_id, ?status, "task event applied");
        let freed = finished
            && worker
                .and_then(|name| self.workers.get_mut(&name))
                .is_some_and(|worker| worker.running.remove(task_id));
        let mut changed = vec![task_id.to_string()];
        if freed {
            changed.extend(self.schedule());
        }
        // Only the transition itself, never a late event repeating it.
        if !terminal {
            match status {
                TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Rejected => {
                    changed.extend(self.fail_dependents(task_id));
                }
                TaskStatus::Approved => changed.extend(self.schedule()),
                _ => {}
            }
        }
        changed
    }
}

/// Moves the task's status for `event` and says whether the run ended. A
/// worker that reconnects may keep reporting on a task we already failed;
/// the event is kept but a terminal status is left alone.
fn transition(task: &mut Task, event: &TaskEvent) -> bool {
    let terminal = task.status.is_terminal();
    let (status, finished) = match event {
        TaskEvent::Started => (Some(TaskStatus::Running), false),
        TaskEvent::Completed { result } => {
            if !terminal {
                task.result = Some(result.clone());
            }
            (Some(TaskStatus::AwaitingReview), true)
        }
        TaskEvent::Failed { error } => {
            if !terminal {
                task.error = Some(error.clone());
            }
            (Some(TaskStatus::Failed), true)
        }
        TaskEvent::Cancelled => (Some(TaskStatus::Cancelled), true),
        TaskEvent::Pushed { .. } => (Some(TaskStatus::Approved), false),
        TaskEvent::Discarded => (Some(TaskStatus::Rejected), false),
        // Retry and the two policy notes are for the reader, not the
        // status; a run in progress stays exactly where it was.
        TaskEvent::Output { .. }
        | TaskEvent::Message { .. }
        | TaskEvent::Retry { .. }
        | TaskEvent::AutoApproved
        | TaskEvent::AutoMerged => (None, false),
    };
    if let (Some(status), false) = (status, terminal) {
        task.status = status;
    }
    finished
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
