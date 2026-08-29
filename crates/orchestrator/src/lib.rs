//! Orchestrator: one HTTP API for developers, one WebSocket per worker agent.

mod api;
mod github;
mod persist;
mod state;
mod worker_ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;
use lgtm_protocol::{TaskStatus, WORKER_WS_PATH};

use crate::state::{App, State, TaskRecord};

/// Binds `bind` and serves until the process exits. Installing a tracing
/// subscriber is the caller's job.
pub async fn serve(bind: SocketAddr, token: String, data_dir: PathBuf) -> anyhow::Result<()> {
    let tasks_dir = data_dir.join("tasks");
    std::fs::create_dir_all(&tasks_dir)?;

    let mut state = State::default();
    for stored in persist::load_all(&tasks_dir) {
        let mut task = stored.task;
        // No worker process survived the restart, so anything running is lost.
        // Queued tasks are schedulable again once their stale assignment is
        // cleared; the scheduler only looks at unassigned ones.
        let interrupted = matches!(task.status, TaskStatus::Running);
        let changed = interrupted || (task.status == TaskStatus::Queued && task.worker.is_some());
        if interrupted {
            task.status = TaskStatus::Failed;
            task.error = Some("orchestrator restarted".into());
        } else if changed {
            task.worker = None;
        }
        let rec = TaskRecord::new(task, stored.events);
        if changed {
            persist::save(&tasks_dir, &persist::Stored::from(&rec));
        }
        state.tasks.insert(rec.task.id.clone(), rec);
    }
    tracing::info!(tasks = state.tasks.len(), "loaded tasks");

    let (persist_tx, persist_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(persist::writer(tasks_dir, persist_rx));

    let github = lgtm_github::GitHub::from_env();
    tracing::info!(enabled = github.is_some(), "github integration");
    let app = Arc::new(App {
        token,
        state: Mutex::new(state),
        persist: persist_tx,
        github,
    });
    github::resume_ci_polls(&app);
    let router = Router::new()
        .nest("/api", api::router(app.clone()))
        .route(WORKER_WS_PATH, get(worker_ws::handler))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "orchestrator listening");
    axum::serve(listener, router).await?;
    Ok(())
}
