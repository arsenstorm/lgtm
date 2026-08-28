//! Orchestrator: one HTTP API for developers, one WebSocket per worker agent.

mod api;
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
        // Nothing survived the restart, so anything mid-flight is lost.
        let interrupted = matches!(task.status, TaskStatus::Queued | TaskStatus::Running);
        if interrupted {
            task.status = TaskStatus::Failed;
            task.error = Some("orchestrator restarted".into());
        }
        let rec = TaskRecord::new(task, stored.events);
        if interrupted {
            persist::save(&tasks_dir, &rec);
        }
        state.tasks.insert(rec.task.id.clone(), rec);
    }
    tracing::info!(tasks = state.tasks.len(), "loaded tasks");

    let app = Arc::new(App {
        token,
        tasks_dir,
        state: Mutex::new(state),
    });
    let router = Router::new()
        .nest("/api", api::router(app.clone()))
        .route(WORKER_WS_PATH, get(worker_ws::handler))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "orchestrator listening");
    axum::serve(listener, router).await?;
    Ok(())
}
