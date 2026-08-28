//! Shared state and every status transition, kept free of I/O so it can be
//! tested without sockets or files.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use lgtm_protocol::{
    OrchestratorMessage, StoredEvent, Task, TaskEvent, TaskId, TaskSpec, TaskStatus, WorkerInfo,
};
use tokio::sync::{broadcast, mpsc};

use crate::persist::Stored;

const LIVE_CAPACITY: usize = 1024;

pub struct App {
    pub token: String,
    pub state: Mutex<State>,
    /// Writes go to a task that owns the directory; a request handler never
    /// holds a path of its own.
    pub persist: mpsc::UnboundedSender<crate::persist::Stored>,
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
            let _ = self.persist.send(Stored::from(rec));
        }
    }
}

#[derive(Default)]
pub struct State {
    pub workers: HashMap<String, WorkerConn>,
    pub tasks: HashMap<TaskId, TaskRecord>,
}

/// A live worker socket.
pub struct Conn {
    pub tx: mpsc::UnboundedSender<OrchestratorMessage>,
    /// Identifies the socket that registered this entry. A reconnecting worker
    /// replaces the entry under the same name, and the old socket's cleanup
    /// must not disconnect the new registration.
    pub conn_id: u64,
}

pub struct WorkerConn {
    pub info: WorkerInfo,
    pub running: HashSet<TaskId>,
    /// `None` while the worker is gone but still inside its grace period.
    pub conn: Option<Conn>,
    /// Bumped on every connect and disconnect, so a grace timer only expires
    /// the disconnect it was started for.
    pub generation: u64,
}

impl WorkerConn {
    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    fn free_slots(&self) -> u32 {
        let running = u32::try_from(self.running.len()).unwrap_or(u32::MAX);
        self.info.slots.saturating_sub(running)
    }

    fn send(&self, msg: OrchestratorMessage) {
        if let Some(conn) = &self.conn {
            let _ = conn.tx.send(msg);
        }
    }
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
    pub repo: lgtm_github::Repo,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    /// Head sha to poll checks for, empty when the worker did not report one.
    pub sha: String,
}

const TITLE_MAX: usize = 72;

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

impl State {
    fn new_id(&self) -> TaskId {
        loop {
            let id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
            if !self.tasks.contains_key(&id) {
                return id;
            }
        }
    }

    /// Whether a task could ever run, so one that cannot is refused instead of
    /// queued forever. `Err` holds the 409 message.
    pub fn check_eligible(&self, spec: &TaskSpec) -> Result<(), String> {
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
        let any = self
            .workers
            .values()
            .any(|worker| worker.is_connected() && worker.info.executors.contains(&spec.executor));
        any.then_some(()).ok_or_else(|| "no eligible worker".into())
    }

    /// Connected worker with the most free slots that can run `spec`, ties
    /// broken by the lowest name.
    fn candidate(&self, spec: &TaskSpec) -> Option<String> {
        self.workers
            .values()
            .filter(|worker| {
                worker.is_connected()
                    && worker.free_slots() > 0
                    && worker.info.executors.contains(&spec.executor)
                    && spec
                        .worker
                        .as_ref()
                        .is_none_or(|name| *name == worker.info.name)
            })
            .max_by(|a, b| {
                a.free_slots()
                    .cmp(&b.free_slots())
                    .then_with(|| b.info.name.cmp(&a.info.name))
            })
            .map(|worker| worker.info.name.clone())
    }

    /// Assigns unassigned queued tasks, oldest first, and starts them.
    /// Returns the ids it assigned.
    pub fn schedule(&mut self) -> Vec<TaskId> {
        let mut queued: Vec<(u64, TaskId)> = self
            .tasks
            .values()
            .filter(|rec| rec.task.status == TaskStatus::Queued && rec.task.worker.is_none())
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
        };
        let id = task.id.clone();
        tracing::info!(task = %id, "task created");
        self.tasks
            .insert(id.clone(), TaskRecord::new(task, Vec::new()));
        let mut changed = vec![id.clone()];
        changed.extend(self.schedule());
        Ok((self.tasks[&id].task.clone(), changed))
    }

    /// Registers a connection under `info.name`, restoring the tasks the worker
    /// says it is still running. Returns the ids to persist.
    pub fn worker_hello(
        &mut self,
        info: WorkerInfo,
        running: Vec<TaskId>,
        conn: Conn,
    ) -> Vec<TaskId> {
        let name = info.name.clone();
        let restored: HashSet<TaskId> = running
            .into_iter()
            .filter(|id| {
                self.tasks.get(id).is_some_and(|rec| {
                    rec.task.status == TaskStatus::Running
                        || (rec.task.status == TaskStatus::Queued
                            && rec.task.worker.as_ref() == Some(&name))
                })
            })
            .collect();
        let previous = match self.workers.get_mut(&name) {
            Some(worker) => {
                worker.info = info;
                worker.conn = Some(conn);
                worker.generation += 1;
                std::mem::replace(&mut worker.running, restored.clone())
            }
            None => {
                self.workers.insert(
                    name.clone(),
                    WorkerConn {
                        info,
                        running: restored.clone(),
                        conn: Some(conn),
                        generation: 1,
                    },
                );
                HashSet::new()
            }
        };
        tracing::info!(worker = %name, tasks = restored.len(), "worker connected");
        let mut changed = Vec::new();
        for id in previous.difference(&restored) {
            changed.extend(self.fail_unfinished(id, "lost on worker"));
        }
        changed.extend(self.schedule());
        changed
    }

    /// Drops the socket but keeps the worker's tasks. Returns the generation
    /// the grace timer should expire, or `None` if a newer socket owns the name.
    pub fn disconnect(&mut self, name: &str, conn_id: u64) -> Option<u64> {
        let worker = self.workers.get_mut(name)?;
        if worker
            .conn
            .as_ref()
            .is_none_or(|conn| conn.conn_id != conn_id)
        {
            return None;
        }
        worker.conn = None;
        worker.generation += 1;
        tracing::info!(worker = %name, tasks = worker.running.len(), "worker disconnected");
        Some(worker.generation)
    }

    /// End of the grace period: the worker never came back, so its tasks are
    /// lost and the entry goes away. A no-op if it reconnected since.
    pub fn expire_worker(&mut self, name: &str, generation: u64) -> Vec<TaskId> {
        let Some(worker) = self.workers.get(name) else {
            return Vec::new();
        };
        if worker.is_connected() || worker.generation != generation {
            return Vec::new();
        }
        let running: Vec<TaskId> = worker.running.iter().cloned().collect();
        self.workers.remove(name);
        tracing::info!(worker = %name, tasks = running.len(), "worker grace period expired");
        let mut changed = Vec::new();
        for id in running {
            changed.extend(self.fail_unfinished(&id, "worker disconnected"));
        }
        changed
    }

    fn fail_unfinished(&mut self, id: &TaskId, error: &str) -> Vec<TaskId> {
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
        // A worker that reconnects may keep reporting on a task we already
        // failed; keep the event but leave a terminal status alone.
        let terminal = rec.task.status.is_terminal();
        let mut finished = false;
        match &stored.event {
            TaskEvent::Started if !terminal => rec.task.status = TaskStatus::Running,
            TaskEvent::Completed { result } => {
                finished = true;
                if !terminal {
                    rec.task.status = TaskStatus::AwaitingReview;
                    rec.task.result = Some(result.clone());
                }
            }
            TaskEvent::Failed { error } => {
                finished = true;
                if !terminal {
                    rec.task.status = TaskStatus::Failed;
                    rec.task.error = Some(error.clone());
                }
            }
            TaskEvent::Cancelled => {
                finished = true;
                if !terminal {
                    rec.task.status = TaskStatus::Cancelled;
                }
            }
            TaskEvent::Pushed { .. } if !terminal => rec.task.status = TaskStatus::Approved,
            TaskEvent::Discarded if !terminal => rec.task.status = TaskStatus::Rejected,
            TaskEvent::Started
            | TaskEvent::Output { .. }
            | TaskEvent::Message { .. }
            | TaskEvent::Pushed { .. }
            | TaskEvent::Discarded => {}
        }
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
        changed
    }

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
        Some(PrPlan {
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
            sha: rec.pushed_sha().unwrap_or_default(),
        })
    }

    pub fn mark_merged(&mut self, task_id: &str) -> Result<Task, CmdError> {
        let rec = self.tasks.get_mut(task_id).ok_or(CmdError::NotFound)?;
        if rec.task.status != TaskStatus::Approved {
            return Err(CmdError::Conflict("task is not approved".into()));
        }
        rec.task.status = TaskStatus::Merged;
        tracing::info!(task = %task_id, "task merged");
        Ok(rec.task.clone())
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

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
