//! Orchestrator: one HTTP API for developers, one WebSocket per runner agent.

mod api;
mod backlog;
mod commands;
mod credentials;
mod execution;
mod github;
mod infer;
mod linear;
pub mod local;
mod notify;
mod orchestrate;
mod persist;
mod plan;
mod policy;
mod project;
mod provenance;
mod provision;
mod runner;
mod runner_ws;
mod state;
mod stats;
mod todo;
pub mod token;
mod users;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;
use lgtm_protocol::{TaskStatus, LEGACY_WORKER_WS_PATH, RUNNER_WS_PATH};

pub use crate::orchestrate::Choice;
use crate::state::{App, State, TaskRecord};

pub struct ServeOptions {
    pub bind: SocketAddr,
    pub token: String,
    pub data_dir: PathBuf,
    /// PEM certificate and key. Plain HTTP when `None`.
    pub tls: Option<(PathBuf, PathBuf)>,
    /// Command to bring an ephemeral runner up when the queue needs one.
    pub provision: Option<ProvisionOptions>,
    /// URL every event a person would want to see is POSTed to.
    pub webhook: Option<String>,
    /// Model that drives a goal after one of its tasks ends. `None` leaves
    /// every goal to its people.
    pub orchestrate: Option<orchestrate::Choice>,
    /// Model to run a task of each kind on (`plan`, `run`) when its spec
    /// names none.
    pub models: Vec<(String, String)>,
    /// How the scheduler breaks a free-slot tie between candidate runners.
    pub prefer: Prefer,
    /// The workspace this orchestrator records on everything it creates;
    /// one per orchestrator until teams exist.
    pub workspace: Option<String>,
}

/// How `State::candidate` breaks a tie between runners with the same number
/// of free slots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Prefer {
    /// Lowest runner name wins.
    #[default]
    Slots,
    /// The runner with the lower median duration for the task's repository
    /// over the last 7 days wins; a runner with no history sorts last.
    Fastest,
}

pub struct ProvisionOptions {
    /// Run through `sh -c`.
    pub command: String,
    /// Ceiling on connected ephemeral runners.
    pub max: u32,
    /// Where the runner it starts should connect back to.
    pub public_url: String,
}

/// Concurrent `lgtm ask` passes allowed before new ones are refused.
pub(crate) const ASK_SLOTS: usize = 2;

/// Plain HTTP, no provisioning.
pub async fn serve_plain(bind: SocketAddr, token: String, data_dir: PathBuf) -> anyhow::Result<()> {
    serve(ServeOptions {
        bind,
        token,
        data_dir,
        tls: None,
        provision: None,
        webhook: None,
        orchestrate: None,
        models: Vec::new(),
        prefer: Prefer::default(),
        workspace: None,
    })
    .await
}

/// Binds `opts.bind` and serves until the process exits. Installing a tracing
/// subscriber is the caller's job.
pub async fn serve(opts: ServeOptions) -> anyhow::Result<()> {
    let mut state = load_state(&opts.data_dir, opts.provision.is_some())?;
    state.models = opts.models.into_iter().collect();
    state.prefer = opts.prefer;
    state.workspace = opts.workspace;
    let (persist_tx, persist_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(persist::writer(opts.data_dir, persist_rx));

    let github_app = lgtm_github::GithubApp::from_env();
    tracing::info!(enabled = github_app.is_some(), "github app");
    let github = lgtm_github::GitHub::from_env().map(|gh| gh.with_app(github_app));
    tracing::info!(enabled = github.is_some(), "github integration");
    let linear = lgtm_linear::Linear::from_env();
    tracing::info!(enabled = linear.is_some(), "linear integration");
    tracing::info!(enabled = opts.webhook.is_some(), "webhook");
    let scheme = if opts.tls.is_some() { "https" } else { "http" };
    let app = Arc::new(App {
        token: opts.token,
        state: Mutex::new(state),
        persist: persist_tx,
        github,
        linear,
        webhook: opts.webhook,
        orchestrate: opts.orchestrate.and_then(orchestrate::pick),
        // The loop runs beside the server, so it dials it back on loopback.
        base_url: format!("{scheme}://127.0.0.1:{}", opts.bind.port()),
        orchestrating: Mutex::new(Default::default()),
        asking: tokio::sync::Semaphore::new(ASK_SLOTS),
        inferring: Mutex::new(Default::default()),
    });
    github::resume_ci_polls(&app);
    if let Some(provision) = opts.provision {
        tracing::info!(max = provision.max, "provisioning enabled");
        tokio::spawn(provision::run(app.clone(), provision, app.token.clone()));
    }
    let router = Router::new()
        .nest("/api", api::router(app.clone()))
        .route(RUNNER_WS_PATH, get(runner_ws::handler))
        // One release's grace so a runner built before the rename still connects.
        .route(LEGACY_WORKER_WS_PATH, get(runner_ws::handler))
        .with_state(app);
    listen(router, opts.bind, opts.tls).await
}

/// Reads every stored task, batch and memory back, repairing what the restart
/// Reads every stored task, batch and goal back, repairing what the restart
/// broke.
fn load_state(data_dir: &std::path::Path, queue_without_runners: bool) -> anyhow::Result<State> {
    let tasks_dir = data_dir.join("tasks");
    let batches_dir = data_dir.join("batches");
    let memories_dir = data_dir.join("memories");
    std::fs::create_dir_all(&tasks_dir)?;
    std::fs::create_dir_all(&batches_dir)?;
    std::fs::create_dir_all(&memories_dir)?;
    let goals_dir = data_dir.join("goals");
    std::fs::create_dir_all(&goals_dir)?;
    let todos_dir = data_dir.join("todos");
    std::fs::create_dir_all(&todos_dir)?;
    let todo_comments_dir = data_dir.join("todo_comments");
    std::fs::create_dir_all(&todo_comments_dir)?;
    let scratchpads_dir = data_dir.join("scratchpads");
    std::fs::create_dir_all(&scratchpads_dir)?;
    let projects_dir = data_dir.join("projects");
    std::fs::create_dir_all(&projects_dir)?;
    let sessions_dir = data_dir.join("sessions");
    std::fs::create_dir_all(&sessions_dir)?;
    let mut state = State {
        queue_without_runners,
        ..State::default()
    };
    for stored in persist::load_all(&tasks_dir) {
        let (task, changed) = restore(stored.task);
        let rec = TaskRecord::new(task, stored.events);
        if changed {
            persist::save(&tasks_dir, &rec.task);
        }
        state.tasks.insert(rec.task.id.clone(), rec);
    }
    for batch in persist::load_all_batches(&batches_dir) {
        state.batches.insert(batch.id.clone(), batch);
    }
    for memory in persist::load_all_memories(&memories_dir) {
        state.memories.insert(memory.id.clone(), memory);
    }
    for goal in persist::load_all_goals(&goals_dir) {
        state.goals.insert(goal.id.clone(), goal);
    }
    for todo in persist::load_all_todos(&todos_dir) {
        state.todos.insert(todo.id.clone(), todo);
    }
    for comment in persist::load_all_todo_comments(&todo_comments_dir) {
        state.todo_comments.insert(comment.id.clone(), comment);
    }
    for scratchpad in persist::load_all_scratchpads(&scratchpads_dir) {
        state.scratchpads.insert(scratchpad.id.clone(), scratchpad);
    }
    for project in persist::load_all_projects(&projects_dir) {
        state.projects.insert(project.id.clone(), project);
    }
    // Todos written before numbering get theirs here, once: the pass leaves
    // an already-numbered todo alone, so a later restart writes nothing.
    for id in state.number_legacy_todos() {
        persist::save_todo(&todos_dir, &state.todos[&id]);
    }
    for id in std::mem::take(&mut state.dirty_projects) {
        persist::save_project(&projects_dir, &state.projects[&id]);
    }
    for session in persist::load_all_sessions(&sessions_dir) {
        state.sessions.insert(session.id.clone(), session);
    }
    for rec in persist::load_users(data_dir) {
        state.users.insert(rec.user.id.clone(), rec);
    }
    state.credentials = persist::load_credentials(data_dir);
    tracing::info!(
        tasks = state.tasks.len(),
        batches = state.batches.len(),
        memories = state.memories.len(),
        goals = state.goals.len(),
        todos = state.todos.len(),
        sessions = state.sessions.len(),
        "loaded tasks",
    );
    Ok(state)
}

/// No runner process survived the restart, so anything running is lost.
/// Queued tasks are schedulable again once their stale assignment is cleared;
/// the scheduler only looks at unassigned ones. Returns whether it changed.
fn restore(mut task: lgtm_protocol::Task) -> (lgtm_protocol::Task, bool) {
    if task.status == TaskStatus::Running {
        let error = "orchestrator restarted".to_string();
        let event = lgtm_protocol::TaskEvent::Failed {
            error: error.clone(),
        };
        execution::record(&mut task, &event, state::now_ms());
        task.status = TaskStatus::Failed;
        task.error = Some(error);
        return (task, true);
    }
    if task.status == TaskStatus::Queued && task.runner.is_some() {
        task.runner = None;
        return (task, true);
    }
    (task, false)
}

async fn listen(
    router: Router,
    bind: std::net::SocketAddr,
    tls: Option<(std::path::PathBuf, std::path::PathBuf)>,
) -> anyhow::Result<()> {
    let Some((cert, key)) = tls else {
        let listener = tokio::net::TcpListener::bind(bind).await?;
        tracing::info!(%bind, "orchestrator listening over http");
        return Ok(axum::serve(listener, router).await?);
    };
    // axum-server brings no crypto provider; the lib must not rely on the
    // binary having installed one.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
    tracing::info!(%bind, "orchestrator listening over https");
    axum_server::bind_rustls(bind, config)
        .serve(router.into_make_service())
        .await?;
    Ok(())
}
