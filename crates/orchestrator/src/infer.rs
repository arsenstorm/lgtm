//! The utility inference lane: one-shot model calls for the orchestrator's
//! own features. A connected runner does the work, so the serve host needs
//! neither the executor binaries nor their credentials; a host with no runner
//! runs the executor itself, which keeps a laptop-only setup working.

use std::sync::Arc;

use lgtm_protocol::{Executor, OrchestratorMessage};
use tokio::sync::oneshot;

use crate::runner::RunnerConn;
use crate::state::{random_id, App, State};

/// Longer than the runner's own 90s cap, so a runner that answers at the last
/// moment still beats this.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Debug)]
pub enum InferError {
    /// No connected runner advertises the executor and this host has none
    /// either, so nothing can serve the call.
    Unavailable,
    /// It ran and did not produce an answer.
    Failed(String),
}

/// One model call: on a runner if one can take it, otherwise here.
pub async fn infer(
    app: &Arc<App>,
    executor: Executor,
    system: &str,
    prompt: &str,
) -> Result<String, InferError> {
    let id = random_id();
    let Some(rx) = send(app, &id, executor, system, prompt) else {
        return local(executor, system, prompt).await;
    };
    match tokio::time::timeout(TIMEOUT, rx).await {
        Ok(Ok(answer)) => answer.map_err(InferError::Failed),
        // The socket died with the call outstanding.
        Ok(Err(_)) => Err(InferError::Failed("the runner went away".into())),
        Err(_) => {
            app.inferring.lock().unwrap().remove(&id);
            Err(InferError::Failed(format!(
                "no runner answered in {}s",
                TIMEOUT.as_secs()
            )))
        }
    }
}

/// The runner answered. An unknown id is not an error: its caller timed out
/// and is already gone.
pub fn complete(app: &App, id: &str, text: String, error: Option<String>) {
    let Some(tx) = app.inferring.lock().unwrap().remove(id) else {
        return;
    };
    let _ = tx.send(error.map_or(Ok(text), Err));
}

/// Registers the call and hands it to a runner, or `None` when none can take
/// it. Not async: the state lock never crosses the await in `infer`.
fn send(
    app: &App,
    id: &str,
    executor: Executor,
    system: &str,
    prompt: &str,
) -> Option<oneshot::Receiver<Result<String, String>>> {
    let state = app.state.lock().unwrap();
    let runner = pick_runner(&state, executor)?;
    let (tx, rx) = oneshot::channel();
    app.inferring.lock().unwrap().insert(id.to_string(), tx);
    runner.send(OrchestratorMessage::Infer {
        id: id.to_string(),
        executor,
        system: system.to_string(),
        prompt: prompt.to_string(),
    });
    Some(rx)
}

/// Connected, advertises `executor`, and running the fewest tasks: inference
/// is a guest on a runner, so it goes to the idlest one. Ties by name, so the
/// choice is repeatable.
fn pick_runner(state: &State, executor: Executor) -> Option<&RunnerConn> {
    state
        .runners
        .values()
        .filter(|runner| runner.is_connected() && runner.info.executors.contains(&executor))
        .min_by(|a, b| {
            a.running
                .len()
                .cmp(&b.running.len())
                .then_with(|| a.info.name.cmp(&b.info.name))
        })
}

async fn local(executor: Executor, system: &str, prompt: &str) -> Result<String, InferError> {
    if !crate::orchestrate::on_path(executor.binary()) {
        return Err(InferError::Unavailable);
    }
    crate::orchestrate::one_shot(executor, system, prompt)
        .await
        .map_err(InferError::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{RunnerInfo, TaskId};
    use tokio::sync::mpsc;

    fn app() -> Arc<App> {
        let (persist, _rx) = mpsc::unbounded_channel();
        Arc::new(App {
            token: "tok".into(),
            state: std::sync::Mutex::new(State::default()),
            persist,
            github: None,
            linear: None,
            webhook: None,
            orchestrate: None,
            base_url: "http://127.0.0.1:1".into(),
            orchestrating: std::sync::Mutex::new(Default::default()),
            asking: tokio::sync::Semaphore::new(crate::ASK_SLOTS),
            inferring: std::sync::Mutex::new(Default::default()),
        })
    }

    /// A connected runner with `executors`, already running `running` tasks.
    /// The receiver is returned so the frames it is sent can be read back.
    fn connect(
        app: &App,
        name: &str,
        executors: Vec<Executor>,
        running: &[&str],
    ) -> mpsc::UnboundedReceiver<OrchestratorMessage> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut state = app.state.lock().unwrap();
        state.runner_hello(
            RunnerInfo {
                name: name.into(),
                os: "linux".into(),
                arch: "x86_64".into(),
                executors,
                slots: 4,
                ephemeral: false,
                capabilities: Vec::new(),
                cpu_cores: 0,
                memory_mb: 0,
            },
            Vec::new(),
            crate::state::Conn { tx, conn_id: 1 },
        );
        let runner = state.runners.get_mut(name).unwrap();
        runner.running = running.iter().map(|id| TaskId::from(*id)).collect();
        rx
    }

    #[test]
    fn the_idlest_runner_with_the_right_executor_takes_the_call() {
        let app = app();
        let _busy = connect(&app, "busy", vec![Executor::Claude], &["t1", "t2"]);
        let _idle = connect(&app, "idle", vec![Executor::Claude], &["t3"]);
        let _wrong = connect(&app, "wrong", vec![Executor::Codex], &[]);
        let state = app.state.lock().unwrap();
        let picked = pick_runner(&state, Executor::Claude).unwrap();
        assert_eq!(picked.info.name, "idle");
        assert_eq!(
            pick_runner(&state, Executor::Codex).unwrap().info.name,
            "wrong"
        );
    }

    #[test]
    fn a_runner_without_the_executor_is_no_candidate() {
        let app = app();
        let _codex = connect(&app, "codex-only", vec![Executor::Codex], &[]);
        let state = app.state.lock().unwrap();
        assert!(pick_runner(&state, Executor::Claude).is_none());
    }

    #[tokio::test]
    async fn a_runners_answer_completes_the_call() {
        let app = app();
        let mut rx = connect(&app, "r", vec![Executor::Claude], &[]);
        let asked = app.clone();
        let call =
            tokio::spawn(async move { infer(&asked, Executor::Claude, "system", "prompt").await });
        let Some(OrchestratorMessage::Infer { id, prompt, .. }) = rx.recv().await else {
            panic!("expected an infer frame");
        };
        assert_eq!(prompt, "prompt");
        complete(&app, &id, "rewritten".into(), None);
        assert_eq!(call.await.unwrap().unwrap(), "rewritten");
        assert!(app.inferring.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn a_runner_that_never_answers_times_out_and_is_forgotten() {
        let app = app();
        let _rx = connect(&app, "r", vec![Executor::Claude], &[]);
        let err = infer(&app, Executor::Claude, "system", "prompt")
            .await
            .unwrap_err();
        let InferError::Failed(note) = err else {
            panic!("expected a failure, got {err:?}");
        };
        assert!(note.contains("no runner answered"), "{note}");
        assert!(app.inferring.lock().unwrap().is_empty());
    }
}
