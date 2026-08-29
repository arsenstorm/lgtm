//! Ephemeral workers on demand: when the queue holds work no connected worker
//! can take, run a command that is expected to bring one up.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lgtm_protocol::TaskStatus;

use crate::state::{App, State};
use crate::ProvisionOptions;

/// How often the queue is looked at.
const PROVISION_INTERVAL: Duration = Duration::from_secs(30);
/// How long a launched worker gets to connect before another one is allowed.
const PROVISION_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Connected workers that go away by themselves.
fn ephemeral_count(state: &State) -> u32 {
    let count = state
        .workers
        .values()
        .filter(|worker| worker.is_connected() && worker.info.ephemeral)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn queued_count(state: &State) -> u32 {
    let count = state
        .tasks
        .values()
        .filter(|rec| rec.task.status == TaskStatus::Queued)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Whether a worker should be started: some ready task has nowhere to run, we
/// are under the cap, and nothing we started is still on its way.
pub fn needs_provision(state: &State, max: u32, in_flight: bool) -> bool {
    if in_flight || ephemeral_count(state) >= max {
        return false;
    }
    state.tasks.values().any(|rec| {
        let spec = &rec.task.spec;
        rec.task.status == TaskStatus::Queued
            && rec.task.worker.is_none()
            && state.deps_met(spec)
            && !state.workers.values().any(|worker| {
                worker.is_connected()
                    && worker.free_slots() > 0
                    && worker.info.executors.contains(&spec.executor)
            })
    })
}

pub async fn run(app: Arc<App>, opts: ProvisionOptions, token: String) {
    // Instant of the launch and the ephemeral count it saw, so the next tick
    // can tell whether the worker arrived.
    let mut in_flight: Option<(Instant, u32)> = None;
    loop {
        tokio::time::sleep(PROVISION_INTERVAL).await;
        let launch = {
            let state = app.state.lock().unwrap();
            if in_flight.is_some_and(|(at, count)| {
                at.elapsed() >= PROVISION_TIMEOUT || ephemeral_count(&state) > count
            }) {
                in_flight = None;
            }
            needs_provision(&state, opts.max, in_flight.is_some())
                .then(|| (ephemeral_count(&state), queued_count(&state)))
        };
        let Some((ephemeral, queued)) = launch else {
            continue;
        };
        in_flight = Some((Instant::now(), ephemeral));
        spawn_worker(&opts, &token, queued);
    }
}

fn spawn_worker(opts: &ProvisionOptions, token: &str, queued: u32) {
    tracing::info!(command = %opts.command, queued, "provisioning a worker");
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(&opts.command)
        .env("LGTM_ORCHESTRATOR_URL", &opts.public_url)
        .env("LGTM_TOKEN", token)
        .env("LGTM_QUEUED", queued.to_string())
        .stdin(std::process::Stdio::null());
    tokio::spawn(async move {
        match cmd.output().await {
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                tracing::info!(status = %out.status, stderr = %stderr.trim(), "provision command finished");
            }
            Err(err) => tracing::warn!(%err, "provision command failed to start"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Conn, TaskRecord, WorkerConn};
    use lgtm_protocol::{Executor, Task, TaskId, TaskKind, TaskSpec, WorkerInfo};
    use std::collections::HashSet;

    /// Registers a connected worker. Nothing is ever sent to it, so the
    /// receiver going away here does not matter.
    fn connect(state: &mut State, name: &str, slots: u32, ephemeral: bool) {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        state.workers.insert(
            name.to_string(),
            WorkerConn {
                info: WorkerInfo {
                    name: name.to_string(),
                    os: "linux".into(),
                    arch: "x86_64".into(),
                    executors: vec![Executor::Claude],
                    slots,
                    ephemeral,
                },
                running: HashSet::new(),
                conn: Some(Conn { tx, conn_id: 1 }),
                generation: 1,
            },
        );
    }

    fn task(
        state: &mut State,
        status: TaskStatus,
        worker: Option<&str>,
        deps: Vec<TaskId>,
    ) -> TaskId {
        let id = state.new_id();
        let task = Task {
            id: id.clone(),
            spec: TaskSpec {
                repository: "https://example.com/repo.git".into(),
                base_branch: "main".into(),
                prompt: "do the thing".into(),
                executor: Executor::Claude,
                worker: worker.map(str::to_string),
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: deps,
            },
            status,
            worker: None,
            created_at: 1,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
        };
        state
            .tasks
            .insert(id.clone(), TaskRecord::new(task, Vec::new()));
        id
    }

    #[test]
    fn queued_task_with_nowhere_to_run_provisions() {
        let mut state = State::default();
        assert!(!needs_provision(&state, 2, false), "empty queue");
        task(&mut state, TaskStatus::Queued, None, Vec::new());
        assert!(needs_provision(&state, 2, false));
        // A launch is already on its way.
        assert!(!needs_provision(&state, 2, true));
    }

    #[test]
    fn a_worker_that_could_take_it_is_enough() {
        let mut state = State::default();
        task(&mut state, TaskStatus::Queued, None, Vec::new());
        connect(&mut state, "a", 1, false);
        assert!(!needs_provision(&state, 2, false));
        // Its only slot is taken, so the task still has nowhere to go.
        state.workers.get_mut("a").unwrap().running = ["busy".to_string()].into_iter().collect();
        assert!(needs_provision(&state, 2, false));
    }

    #[test]
    fn a_pinned_task_still_needs_a_worker() {
        let mut state = State::default();
        task(&mut state, TaskStatus::Queued, Some("gone"), Vec::new());
        assert!(needs_provision(&state, 2, false));
    }

    #[test]
    fn stops_at_the_cap() {
        let mut state = State::default();
        task(&mut state, TaskStatus::Queued, None, Vec::new());
        connect(&mut state, "e1", 0, true);
        assert!(needs_provision(&state, 2, false));
        connect(&mut state, "e2", 0, true);
        assert!(!needs_provision(&state, 2, false));
    }

    #[test]
    fn a_blocked_task_is_not_work_yet() {
        let mut state = State::default();
        let dep = task(&mut state, TaskStatus::Queued, None, Vec::new());
        state.tasks.get_mut(&dep).unwrap().task.status = TaskStatus::Running;
        task(&mut state, TaskStatus::Queued, None, vec![dep]);
        assert!(!needs_provision(&state, 2, false));
    }
}
