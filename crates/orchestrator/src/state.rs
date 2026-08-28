//! Shared state and every status transition, kept free of I/O so it can be
//! tested without sockets or files.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use lgtm_protocol::{
    OrchestratorMessage, StoredEvent, Task, TaskEvent, TaskId, TaskSpec, TaskStatus, WorkerInfo,
};
use tokio::sync::{broadcast, mpsc};

const LIVE_CAPACITY: usize = 1024;

pub struct App {
    pub token: String,
    pub tasks_dir: PathBuf,
    pub state: Mutex<State>,
}

#[derive(Default)]
pub struct State {
    pub workers: HashMap<String, WorkerConn>,
    pub tasks: HashMap<TaskId, TaskRecord>,
}

pub struct WorkerConn {
    pub info: WorkerInfo,
    pub tx: mpsc::UnboundedSender<OrchestratorMessage>,
    pub running: HashSet<TaskId>,
    /// Identifies the socket that registered this entry. A reconnecting worker
    /// replaces the entry under the same name, and the old socket's cleanup
    /// must not delete the new registration.
    pub conn_id: u64,
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
}

/// Why a command against a task could not run.
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

    /// Explicit worker if the spec names one, else the connected worker with
    /// the fewest running tasks. `Err` holds the 409 message.
    pub fn pick_worker(&self, spec: &TaskSpec) -> Result<String, String> {
        if let Some(name) = &spec.worker {
            let Some(conn) = self.workers.get(name) else {
                return Err(format!("worker {name} is not connected"));
            };
            if !conn.info.executors.contains(&spec.executor) {
                let executor = spec.executor.binary();
                return Err(format!("worker {name} does not have {executor}"));
            }
            return Ok(name.clone());
        }
        self.workers
            .values()
            .filter(|conn| conn.info.executors.contains(&spec.executor))
            .min_by(|a, b| {
                a.running
                    .len()
                    .cmp(&b.running.len())
                    .then_with(|| a.info.name.cmp(&b.info.name))
            })
            .map(|conn| conn.info.name.clone())
            .ok_or_else(|| "no eligible worker".to_string())
    }

    pub fn create_task(&mut self, spec: TaskSpec) -> Result<Task, String> {
        let worker = self.pick_worker(&spec)?;
        let task = Task {
            id: self.new_id(),
            spec,
            status: TaskStatus::Queued,
            worker: Some(worker.clone()),
            created_at: now_ms(),
            result: None,
            error: None,
        };
        if let Some(conn) = self.workers.get_mut(&worker) {
            conn.running.insert(task.id.clone());
            let _ = conn.tx.send(OrchestratorMessage::Start {
                task: Box::new(task.clone()),
            });
        }
        tracing::info!(task = %task.id, %worker, "task created");
        self.tasks
            .insert(task.id.clone(), TaskRecord::new(task.clone(), Vec::new()));
        Ok(task)
    }

    /// Records a worker event and applies its status transition. Returns the
    /// record so the caller can persist it.
    pub fn apply_event(&mut self, task_id: &str, event: TaskEvent) -> Option<&TaskRecord> {
        let Some(rec) = self.tasks.get_mut(task_id) else {
            tracing::warn!(task = %task_id, "event for unknown task, ignoring");
            return None;
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
            TaskEvent::Pushed { .. } => rec.task.status = TaskStatus::Approved,
            TaskEvent::Discarded => rec.task.status = TaskStatus::Rejected,
            TaskEvent::Started | TaskEvent::Output { .. } => {}
        }
        let status = rec.task.status;
        let worker = rec.task.worker.clone();
        let _ = rec.live.send(stored);
        tracing::debug!(task = %task_id, ?status, "task event applied");
        if finished {
            if let Some(conn) = worker.and_then(|name| self.workers.get_mut(&name)) {
                conn.running.remove(task_id);
            }
        }
        self.tasks.get(task_id)
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
        let conn = self
            .workers
            .get(&name)
            .ok_or_else(|| CmdError::Conflict(format!("worker {name} is not connected")))?;
        let _ = conn.tx.send(msg(task_id.to_string()));
        Ok(rec.task.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskResult};

    fn worker(state: &mut State, name: &str, executors: Vec<Executor>, running: &[&str]) {
        let (tx, _rx) = mpsc::unbounded_channel();
        state.workers.insert(
            name.to_string(),
            WorkerConn {
                info: WorkerInfo {
                    name: name.to_string(),
                    os: "linux".into(),
                    arch: "x86_64".into(),
                    executors,
                },
                tx,
                running: running.iter().map(|s| (*s).to_string()).collect(),
                conn_id: 1,
            },
        );
    }

    fn spec(executor: Executor, worker: Option<&str>) -> TaskSpec {
        TaskSpec {
            repository: "https://example.com/repo.git".into(),
            base_branch: "main".into(),
            prompt: "do the thing".into(),
            executor,
            worker: worker.map(str::to_string),
        }
    }

    #[test]
    fn picks_least_loaded_worker() {
        let mut state = State::default();
        worker(&mut state, "busy", vec![Executor::Claude], &["aaaaaaaa"]);
        worker(&mut state, "idle", vec![Executor::Claude], &[]);
        worker(&mut state, "codexonly", vec![Executor::Codex], &[]);

        assert_eq!(
            state.pick_worker(&spec(Executor::Claude, None)).unwrap(),
            "idle"
        );
        // The only Codex worker wins despite the Claude workers being idle.
        assert_eq!(
            state.pick_worker(&spec(Executor::Codex, None)).unwrap(),
            "codexonly"
        );
        assert_eq!(
            state
                .pick_worker(&spec(Executor::Claude, Some("codexonly")))
                .unwrap_err(),
            "worker codexonly does not have claude"
        );
        assert_eq!(
            state
                .pick_worker(&spec(Executor::Claude, Some("ghost")))
                .unwrap_err(),
            "worker ghost is not connected"
        );
        state.workers.clear();
        assert_eq!(
            state
                .pick_worker(&spec(Executor::Claude, None))
                .unwrap_err(),
            "no eligible worker"
        );
    }

    #[test]
    fn apply_event_transitions() {
        let mut state = State::default();
        worker(&mut state, "idle", vec![Executor::Claude], &[]);
        let id = state.create_task(spec(Executor::Claude, None)).unwrap().id;
        assert!(state.workers["idle"].running.contains(&id));

        state.apply_event(&id, TaskEvent::Started);
        assert_eq!(state.tasks[&id].task.status, TaskStatus::Running);

        let result = TaskResult {
            branch: format!("lgtm/{id}"),
            diff: "diff".into(),
            changed_files: vec!["a.rs".into()],
        };
        state.apply_event(&id, TaskEvent::Completed { result });
        assert_eq!(state.tasks[&id].task.status, TaskStatus::AwaitingReview);
        assert!(state.tasks[&id].task.result.is_some());
        assert!(state.workers["idle"].running.is_empty());

        state.apply_event(
            &id,
            TaskEvent::Pushed {
                branch: format!("lgtm/{id}"),
            },
        );
        assert_eq!(state.tasks[&id].task.status, TaskStatus::Approved);
        assert_eq!(state.tasks[&id].events.len(), 3);
    }

    #[test]
    fn terminal_status_survives_late_events() {
        let mut state = State::default();
        worker(&mut state, "idle", vec![Executor::Claude], &[]);
        let id = state.create_task(spec(Executor::Claude, None)).unwrap().id;

        state.apply_event(&id, TaskEvent::Cancelled);
        assert_eq!(state.tasks[&id].task.status, TaskStatus::Cancelled);

        state.apply_event(
            &id,
            TaskEvent::Failed {
                error: "worker disconnected".into(),
            },
        );
        assert_eq!(state.tasks[&id].task.status, TaskStatus::Cancelled);
        assert!(state.tasks[&id].task.error.is_none());
        assert_eq!(state.tasks[&id].events.len(), 2);
    }
}
