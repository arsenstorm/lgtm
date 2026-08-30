//! Runner connections: who is connected, what each one is running, and what
//! happens to those tasks when the socket goes away.

use std::collections::HashSet;

use lgtm_protocol::{OrchestratorMessage, RunnerInfo, TaskId, TaskSpec, TaskStatus};
use tokio::sync::mpsc;

use crate::state::State;

/// A live runner socket.
pub struct Conn {
    pub tx: mpsc::UnboundedSender<OrchestratorMessage>,
    /// Identifies the socket that registered this entry. A reconnecting runner
    /// replaces the entry under the same name, and the old socket's cleanup
    /// must not disconnect the new registration.
    pub conn_id: u64,
}

pub struct RunnerConn {
    pub info: RunnerInfo,
    pub running: HashSet<TaskId>,
    /// `None` while the runner is gone but still inside its grace period.
    pub conn: Option<Conn>,
    /// Bumped on every connect and disconnect, so a grace timer only expires
    /// the disconnect it was started for.
    pub generation: u64,
}

impl RunnerConn {
    pub fn is_connected(&self) -> bool {
        self.conn.is_some()
    }

    pub(crate) fn free_slots(&self) -> u32 {
        let running = u32::try_from(self.running.len()).unwrap_or(u32::MAX);
        self.info.slots.saturating_sub(running)
    }

    /// Connected, with a free slot, the executor `spec` asks for, and every
    /// capability it requires.
    pub(crate) fn can_run(&self, spec: &TaskSpec) -> bool {
        self.is_connected()
            && self.free_slots() > 0
            && self.info.executors.contains(&spec.executor)
            && self.info.has_all(&spec.requirements)
    }

    pub(crate) fn send(&self, msg: OrchestratorMessage) {
        if let Some(conn) = &self.conn {
            let _ = conn.tx.send(msg);
        }
    }
}

impl State {
    /// Registers a connection under `info.name`, restoring the tasks the runner
    /// says it is still running. Returns the ids to persist.
    pub fn runner_hello(
        &mut self,
        info: RunnerInfo,
        running: Vec<TaskId>,
        conn: Conn,
    ) -> Vec<TaskId> {
        let name = info.name.clone();
        let restored: HashSet<TaskId> = running
            .into_iter()
            .filter(|id| self.still_running(id, &name))
            .collect();
        let previous = match self.runners.get_mut(&name) {
            Some(runner) => {
                runner.info = info;
                runner.conn = Some(conn);
                runner.generation += 1;
                std::mem::replace(&mut runner.running, restored.clone())
            }
            None => {
                self.runners.insert(
                    name.clone(),
                    RunnerConn {
                        info,
                        running: restored.clone(),
                        conn: Some(conn),
                        generation: 1,
                    },
                );
                HashSet::new()
            }
        };
        tracing::info!(runner = %name, tasks = restored.len(), "runner connected");
        let mut changed = Vec::new();
        for id in previous.difference(&restored) {
            changed.extend(self.lose_unfinished(id));
        }
        changed.extend(self.schedule());
        changed
    }

    /// Whether a task a reconnecting runner reports is still its to run.
    fn still_running(&self, id: &str, runner: &str) -> bool {
        self.tasks.get(id).is_some_and(|rec| {
            rec.task.status == TaskStatus::Running
                || (rec.task.status == TaskStatus::Queued
                    && rec.task.runner.as_deref() == Some(runner))
        })
    }

    /// Drops the socket but keeps the runner's tasks. Returns the generation
    /// the grace timer should expire, or `None` if a newer socket owns the name.
    pub fn disconnect(&mut self, name: &str, conn_id: u64) -> Option<u64> {
        let runner = self.runners.get_mut(name)?;
        if runner
            .conn
            .as_ref()
            .is_none_or(|conn| conn.conn_id != conn_id)
        {
            return None;
        }
        runner.conn = None;
        runner.generation += 1;
        tracing::info!(runner = %name, tasks = runner.running.len(), "runner disconnected");
        Some(runner.generation)
    }

    /// End of the grace period: the runner never came back, so its tasks are
    /// lost and the entry goes away. A no-op if it reconnected since.
    pub fn expire_runner(&mut self, name: &str, generation: u64) -> Vec<TaskId> {
        let Some(runner) = self.runners.get(name) else {
            return Vec::new();
        };
        if runner.is_connected() || runner.generation != generation {
            return Vec::new();
        }
        tracing::info!(runner = %name, "runner grace period expired");
        self.remove_runner(name)
    }

    /// The runner said it is exiting on purpose, so there is nothing to wait
    /// for: it goes away now. A no-op if a newer socket owns the name.
    pub fn runner_goodbye(&mut self, name: &str, conn_id: u64) -> Vec<TaskId> {
        let runner = self.runners.get(name).filter(|runner| {
            runner
                .conn
                .as_ref()
                .is_some_and(|conn| conn.conn_id == conn_id)
        });
        if runner.is_none() {
            return Vec::new();
        }
        tracing::info!(runner = %name, "runner said goodbye");
        self.remove_runner(name)
    }

    /// Forgets the runner and loses whatever it still had running.
    fn remove_runner(&mut self, name: &str) -> Vec<TaskId> {
        let Some(runner) = self.runners.remove(name) else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for id in runner.running {
            changed.extend(self.lose_unfinished(&id));
        }
        changed
    }
}
