//! Orchestrator: one HTTP API for developers, one WebSocket per worker agent.

mod api;
mod backlog;
mod github;
mod linear;
mod persist;
mod plan;
mod policy;
mod provision;
mod state;
mod worker_ws;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;
use lgtm_protocol::{TaskStatus, WORKER_WS_PATH};

use crate::state::{App, State, TaskRecord};

pub struct ServeOptions {
    pub bind: SocketAddr,
    pub token: String,
    pub data_dir: PathBuf,
    /// PEM certificate and key. Plain HTTP when `None`.
    pub tls: Option<(PathBuf, PathBuf)>,
    /// Command to bring an ephemeral worker up when the queue needs one.
    pub provision: Option<ProvisionOptions>,
}

pub struct ProvisionOptions {
    /// Run through `sh -c`.
    pub command: String,
    /// Ceiling on connected ephemeral workers.
    pub max: u32,
    /// Where the worker it starts should connect back to.
    pub public_url: String,
}

/// Plain HTTP, no provisioning.
pub async fn serve_plain(bind: SocketAddr, token: String, data_dir: PathBuf) -> anyhow::Result<()> {
    serve(ServeOptions {
        bind,
        token,
        data_dir,
        tls: None,
        provision: None,
    })
    .await
}

/// Binds `opts.bind` and serves until the process exits. Installing a tracing
/// subscriber is the caller's job.
pub async fn serve(opts: ServeOptions) -> anyhow::Result<()> {
    let bind = opts.bind;
    let tasks_dir = opts.data_dir.join("tasks");
    let batches_dir = opts.data_dir.join("batches");
    std::fs::create_dir_all(&tasks_dir)?;
    std::fs::create_dir_all(&batches_dir)?;

    let mut state = State {
        queue_without_workers: opts.provision.is_some(),
        ..State::default()
    };
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
    for batch in persist::load_all_batches(&batches_dir) {
        state.batches.insert(batch.id.clone(), batch);
    }
    tracing::info!(
        tasks = state.tasks.len(),
        batches = state.batches.len(),
        "loaded tasks",
    );

    let (persist_tx, persist_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(persist::writer(opts.data_dir, persist_rx));

    let github = lgtm_github::GitHub::from_env();
    tracing::info!(enabled = github.is_some(), "github integration");
    let linear = lgtm_linear::Linear::from_env();
    tracing::info!(enabled = linear.is_some(), "linear integration");
    let app = Arc::new(App {
        token: opts.token,
        state: Mutex::new(state),
        persist: persist_tx,
        github,
        linear,
    });
    github::resume_ci_polls(&app);
    if let Some(provision) = opts.provision {
        tracing::info!(max = provision.max, "provisioning enabled");
        tokio::spawn(provision::run(app.clone(), provision, app.token.clone()));
    }
    let router = Router::new()
        .nest("/api", api::router(app.clone()))
        .route(WORKER_WS_PATH, get(worker_ws::handler))
        .with_state(app);

    match opts.tls {
        Some((cert, key)) => {
            // axum-server brings no crypto provider; the lib must not rely on
            // the binary having installed one.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
            tracing::info!(%bind, "orchestrator listening over https");
            axum_server::bind_rustls(bind, config)
                .serve(router.into_make_service())
                .await?;
        }
        None => {
            let listener = tokio::net::TcpListener::bind(bind).await?;
            tracing::info!(%bind, "orchestrator listening over http");
            axum::serve(listener, router).await?;
        }
    }
    Ok(())
}
