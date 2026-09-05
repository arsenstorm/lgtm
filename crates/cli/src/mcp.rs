//! `lgtm mcp`: a stdio MCP server that hands one agent run the task's
//! context — the repository's memories, todos and scratchpads, the tasks
//! beside it, and the task's own notes. Every object has an
//! `lgtm://<kind>/<id>` link; a person pastes one into a prompt and the agent
//! opens it. The runner registers the server with claude and codex for every
//! run.
//!
//! With `LGTM_GOAL_ID` set it is the orchestration loop's server instead, and
//! carries the tools that inspect and act on a whole goal. Every one of those
//! goes through the endpoints a person uses, so LGTM validates them the same
//! way, and every call lands on the ended task's event log. The loop may read
//! the whole workspace but may only act on the goal that woke it.
//!
//! With `LGTM_ASK` set it answers `lgtm ask`, which reads and never writes.

use std::collections::HashMap;

use anyhow::Result;
use lgtm_client::{Client, Orchestrated, Retry, ScratchpadPatch};
use lgtm_protocol::{Task, TaskKind, TaskSpec, TodoPatch, TodoStatus, Verification};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

const PROTOCOL: &str = "2024-11-05";
/// How much of a task's chatter `task_inspect` carries.
const RECENT_EVENTS: usize = 20;
/// How much of any list a model reads before it drowns in it.
const ROWS: usize = 50;
/// What the MCP spec has a server say when a resource URI names nothing.
const RESOURCE_NOT_FOUND: i64 = -32002;
/// The reads every mode gets. A run's are held to its repository; the loop
/// and `lgtm ask` read the whole workspace.
const READ_TOOLS: [&str; 11] = [
    "open",
    "search",
    "memories_list",
    "memory_open",
    "todos_list",
    "todo_open",
    "scratchpads_list",
    "scratchpad_open",
    "task_inspect",
    "goal_inspect",
    "session_open",
];
/// Reads over the whole workspace, which a plain run does not get: what
/// else is running, who started it, and where two tasks are about to collide.
const WORKSPACE_TOOLS: [&str; 5] = [
    "tasks_list",
    "goals_list",
    "sessions_list",
    "activity",
    "runner_list",
];
/// The tools only a task's run (and the loop, which runs as one) has: its
/// writes, and the notes that live with the task.
const RUN_TOOLS: [&str; 11] = [
    "memory_propose",
    "todo_create",
    "todo_finish",
    "todo_comment",
    "todo_update",
    "scratchpad_read",
    "scratchpad_write",
    "scratchpad_create",
    "scratchpad_update",
    "scratchpad_archive",
    "request_network",
];

/// Which run this server answers for. The harness spawns it with no
/// arguments of its own, so the runner passes this in the environment.
pub enum Env {
    /// One agent run inside a task.
    Run { task_id: String, repository: String },
    /// The orchestration loop for one goal: run tools plus goal and
    /// workspace tools.
    Orchestrate {
        task_id: String,
        repository: String,
        goal_id: String,
    },
    /// `lgtm ask`: workspace reads only, so a question can never create,
    /// message, or approve work.
    Ask,
}

impl Env {
    /// The repository a run's reads are held to. `None` reads everything.
    fn scope(&self) -> Option<&str> {
        match self {
            Env::Run { repository, .. } => Some(repository),
            _ => None,
        }
    }

    fn goal(&self) -> Option<&str> {
        match self {
            Env::Orchestrate { goal_id, .. } => Some(goal_id),
            _ => None,
        }
    }
}

pub async fn serve(client: &Client) -> Result<i32> {
    let env = require_env();
    // An orchestration pass names its goal on every call, so the orchestrator
    // — not just `under_goal` below — holds it to that goal's tasks.
    let client = &match &env {
        Env::Orchestrate { goal_id, .. } => client.clone().scoped_to_goal(goal_id.clone()),
        _ => client.clone(),
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(reply) = handle(&request, client, &env).await else {
            continue;
        };
        stdout.write_all(format!("{reply}\n").as_bytes()).await?;
        stdout.flush().await?;
    }
    Ok(0)
}

fn require_env() -> Env {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    match (var("LGTM_TASK_ID"), var("LGTM_REPOSITORY")) {
        // The task vars outrank LGTM_ASK: a stray exported LGTM_ASK must
        // never strip a real run or loop pass down to the ask reads.
        (Some(task_id), Some(repository)) => match var("LGTM_GOAL_ID") {
            Some(goal_id) => Env::Orchestrate {
                task_id,
                repository,
                goal_id,
            },
            None => Env::Run {
                task_id,
                repository,
            },
        },
        _ if var("LGTM_ASK").is_some() => Env::Ask,
        _ => {
            eprintln!("lgtm mcp needs LGTM_TASK_ID and LGTM_REPOSITORY; the runner sets them");
            std::process::exit(2);
        }
    }
}

/// One JSON-RPC request in, one response out — or `None` for a notification,
/// which must not be answered.
async fn handle(request: &Value, client: &Client, env: &Env) -> Option<Value> {
    let id = request.get("id")?.clone();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    Some(match reply(method, &params, client, env).await {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
    })
}

async fn reply(method: &str, params: &Value, client: &Client, env: &Env) -> Result<Value, Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL,
            "capabilities": { "tools": {}, "resources": {} },
            "serverInfo": { "name": "lgtm", "version": env!("CARGO_PKG_VERSION") },
        })),
        "tools/list" => Ok(json!({ "tools": tools(env) })),
        "ping" => Ok(json!({})),
        "tools/call" => Ok(match called(params, client, env).await {
            Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
            Err(err) => json!({
                "content": [{ "type": "text", "text": format!("{err:#}") }],
                "isError": true,
            }),
        }),
        "resources/list" => resources(client, env.scope())
            .await
            .map_err(|err| json!({ "code": RESOURCE_NOT_FOUND, "message": format!("{err:#}") })),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            match open(client, uri, env.scope()).await {
                Ok(text) => Ok(json!({
                    "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": text }],
                })),
                Err(err) => {
                    Err(json!({ "code": RESOURCE_NOT_FOUND, "message": format!("{err:#}") }))
                }
            }
        }
        _ => Err(json!({ "code": -32601, "message": format!("no such method: {method}") })),
    }
}

/// The call, plus the record of it a person can read on the ended task. The
/// record is best-effort: a step that worked is not undone because logging it
/// failed.
async fn called(params: &Value, client: &Client, env: &Env) -> Result<String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let done = call(name, &args, client, env).await;
    if let Env::Orchestrate { task_id, .. } = env {
        let outcome = match &done {
            Ok(text) => text.clone(),
            Err(err) => format!("{err:#}"),
        };
        let _ = client
            .orchestrated(
                task_id,
                &Orchestrated {
                    action: name,
                    reason: text(&args, "reason"),
                    applied: done.is_ok(),
                    note: first_line(&outcome),
                },
            )
            .await;
    }
    done
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

async fn call(name: &str, args: &Value, client: &Client, env: &Env) -> Result<String> {
    if READ_TOOLS.contains(&name) {
        return read_call(name, args, client, env.scope(), env.goal()).await;
    }
    match env {
        Env::Ask if WORKSPACE_TOOLS.contains(&name) => workspace_call(name, args, client).await,
        Env::Ask => anyhow::bail!("no such tool: {name}"),
        Env::Run {
            task_id,
            repository,
        } => run_call(name, args, client, task_id, repository).await,
        Env::Orchestrate {
            task_id,
            repository,
            goal_id,
        } => {
            if RUN_TOOLS.contains(&name) {
                run_call(name, args, client, task_id, repository).await
            } else if WORKSPACE_TOOLS.contains(&name) {
                workspace_call(name, args, client).await
            } else {
                orchestration_call(name, args, client, goal_id).await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Links

/// The kinds of object a link can name, in the order `search` lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Task,
    Todo,
    Goal,
    Memory,
    Session,
    Scratchpad,
}

impl Kind {
    const ALL: [Kind; 6] = [
        Kind::Task,
        Kind::Todo,
        Kind::Goal,
        Kind::Memory,
        Kind::Session,
        Kind::Scratchpad,
    ];

    /// The path segment: the web app's route for the kind, so a link reads
    /// like the page it stands for.
    fn path(self) -> &'static str {
        match self {
            Kind::Task => "tasks",
            Kind::Todo => "todos",
            Kind::Goal => "goals",
            Kind::Memory => "memories",
            Kind::Session => "sessions",
            Kind::Scratchpad => "scratchpads",
        }
    }

    fn from_path(path: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.path() == path)
    }
}

fn link(kind: Kind, id: &str) -> String {
    format!("lgtm://{}/{id}", kind.path())
}

/// `lgtm://<kind>/<id>`. A scratchpad link from the web app may carry an
/// encoded repository between the two; ids are unique on their own, so only
/// the last segment is resolved and the repository is a hint for people.
fn parse_link(value: &str) -> Result<(Kind, &str)> {
    let rest = value
        .strip_prefix("lgtm://")
        .ok_or_else(|| anyhow::anyhow!("not an lgtm:// link: {value}"))?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let kind = parts.next().and_then(Kind::from_path).ok_or_else(|| {
        anyhow::anyhow!(
            "{value} names no kind; links are lgtm://tasks/<id>, todos, goals, memories, sessions or scratchpads"
        )
    })?;
    let id = parts
        .next_back()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{value} names no id"))?;
    Ok((kind, id))
}

/// A kind's own tool takes a bare id or a link of that kind.
fn id_of(kind: Kind, value: &str) -> Result<&str> {
    if !value.starts_with("lgtm://") {
        return Ok(value);
    }
    let (found, id) = parse_link(value)?;
    if found != kind {
        anyhow::bail!(
            "{value} is a {} link, not a {} one",
            found.path(),
            kind.path()
        );
    }
    Ok(id)
}

/// A run reads its repository's objects and the ones that apply to every
/// repository; a link from another repository is refused, not resolved.
fn in_scope(scope: Option<&str>, repository: Option<&str>, what: &str) -> Result<()> {
    match (scope, repository) {
        (Some(scope), Some(repository)) if scope != repository => {
            anyhow::bail!("{what} belongs to another repository")
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Reads

async fn read_call(
    name: &str,
    args: &Value,
    client: &Client,
    scope: Option<&str>,
    goal: Option<&str>,
) -> Result<String> {
    match name {
        "open" => open(client, string(args, "link")?, scope).await,
        "search" => search(client, string(args, "query")?, scope).await,
        "memories_list" => memories_list(client, scope).await,
        "memory_open" => {
            memory_open(client, id_of(Kind::Memory, string(args, "id")?)?, scope).await
        }
        "todos_list" => todos_list(client, scope).await,
        "todo_open" => todo_open(client, id_of(Kind::Todo, string(args, "id")?)?, scope).await,
        "scratchpads_list" => scratchpads_list(client, scope).await,
        "scratchpad_open" => {
            scratchpad_open(client, id_of(Kind::Scratchpad, string(args, "id")?)?, scope).await
        }
        "task_inspect" => {
            task_inspect(client, id_of(Kind::Task, string(args, "task_id")?)?, scope).await
        }
        // The loop's own goal when it names none; anyone else has to say which.
        "goal_inspect" => {
            let id = match args.get("id").and_then(Value::as_str) {
                Some(id) => id_of(Kind::Goal, id)?,
                None => goal.ok_or_else(|| anyhow::anyhow!("goal_inspect needs an id"))?,
            };
            goal_inspect(client, id, scope).await
        }
        "session_open" => {
            session_open(client, id_of(Kind::Session, string(args, "id")?)?, scope).await
        }
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

async fn open(client: &Client, value: &str, scope: Option<&str>) -> Result<String> {
    let (kind, id) = parse_link(value)?;
    match kind {
        Kind::Task => task_inspect(client, id, scope).await,
        Kind::Todo => todo_open(client, id, scope).await,
        Kind::Goal => goal_inspect(client, id, scope).await,
        Kind::Memory => memory_open(client, id, scope).await,
        Kind::Session => session_open(client, id, scope).await,
        Kind::Scratchpad => scratchpad_open(client, id, scope).await,
    }
}

/// A case-insensitive substring over what a model would know from a prompt:
/// titles, prompts, objectives, contents. Ranking would be a guess.
async fn search(client: &Client, query: &str, scope: Option<&str>) -> Result<String> {
    let query = query.to_lowercase();
    let hit = |texts: &[&str]| texts.iter().any(|t| t.to_lowercase().contains(&query));
    let mut rows = Vec::new();
    for task in client.tasks().await? {
        if in_scope(scope, Some(&task.spec.repository), "").is_ok() && hit(&[&task.spec.prompt]) {
            rows.push(format!(
                "{}  \"{}\"",
                link(Kind::Task, &task.id),
                first_line(&task.spec.prompt)
            ));
        }
    }
    for todo in client.todos(scope).await? {
        if hit(&[&todo.title, &todo.description]) {
            rows.push(format!("{}  {}", link(Kind::Todo, &todo.id), todo.title));
        }
    }
    for summary in client.goals().await? {
        let goal = summary.goal;
        if in_scope(scope, Some(&goal.repository), "").is_ok() && hit(&[&goal.objective]) {
            rows.push(format!(
                "{}  \"{}\"",
                link(Kind::Goal, &goal.id),
                first_line(&goal.objective)
            ));
        }
    }
    for memory in client.memories(scope, false).await? {
        if hit(&[&memory.content]) {
            rows.push(format!(
                "{}  {}",
                link(Kind::Memory, &memory.id),
                memory.content
            ));
        }
    }
    for session in client.sessions(scope).await? {
        if hit(&[&session.title]) {
            rows.push(format!(
                "{}  {}",
                link(Kind::Session, &session.id),
                session.title
            ));
        }
    }
    for pad in client.scratchpads(scope).await? {
        if !pad.archived && hit(&[&pad.title, &pad.content]) {
            rows.push(format!(
                "{}  {}",
                link(Kind::Scratchpad, &pad.id),
                pad.title
            ));
        }
    }
    Ok(capped(rows))
}

async fn memories_list(client: &Client, scope: Option<&str>) -> Result<String> {
    Ok(capped(
        client
            .memories(scope, false)
            .await?
            .iter()
            .map(|m| format!("{}  {}", m.id, m.content))
            .collect(),
    ))
}

/// There is no endpoint for one memory: the list is short, so it is scanned.
async fn memory_open(client: &Client, id: &str, scope: Option<&str>) -> Result<String> {
    let memory = client
        .memories(None, false)
        .await?
        .into_iter()
        .find(|memory| memory.id == id)
        .ok_or_else(|| anyhow::anyhow!("memory {id} not found"))?;
    in_scope(scope, memory.repository.as_deref(), "the memory")?;
    let standing = match memory.verification {
        Verification::UserApproved => "approved",
        Verification::AgentProposed => "proposed, awaiting a person",
    };
    Ok(format!("{} ({standing})\n{}", memory.id, memory.content))
}

/// Open todos, newest first. `created_by` is an id; a person reads names.
async fn todos_list(client: &Client, scope: Option<&str>) -> Result<String> {
    let owners = owners(client).await?;
    let mut todos = client.todos(scope).await?;
    todos.retain(|todo| todo.status == TodoStatus::Open);
    todos.sort_by_key(|todo| std::cmp::Reverse(todo.created_at));
    Ok(capped(
        todos
            .iter()
            .map(|todo| {
                format!(
                    "{} {} {} {}",
                    todo.id,
                    owner(&owners, todo.created_by.as_deref()),
                    todo.repository.as_deref().map(repo_short).unwrap_or("-"),
                    todo.title,
                )
            })
            .collect(),
    ))
}

async fn todo_open(client: &Client, id: &str, scope: Option<&str>) -> Result<String> {
    let detail = client.todo(id).await?;
    let todo = &detail.todo;
    in_scope(scope, todo.repository.as_deref(), "the todo")?;
    let owners = owners(client).await?;
    let mut out = format!(
        "{} {} {} {}\n",
        todo.id,
        status_word(todo.status),
        status_word(todo.priority),
        todo.title
    );
    if !todo.description.is_empty() {
        out.push_str(&format!("{}\n", todo.description));
    }
    if let Some(assignee) = &todo.assignee {
        out.push_str(&format!("assignee: {}\n", owner(&owners, Some(assignee))));
    }
    if !todo.blockers.is_empty() {
        out.push_str(&format!("blocked by: {}\n", todo.blockers.join(", ")));
    }
    if let Some(task) = &todo.task {
        out.push_str(&format!("task: {task}\n"));
    }
    if !todo.tags.is_empty() {
        out.push_str(&format!("tags: {}\n", todo.tags.join(", ")));
    }
    for comment in &detail.comments {
        out.push_str(&format!(
            "- {}: {}\n",
            owner(&owners, comment.author.as_deref()),
            comment.body
        ));
    }
    Ok(out)
}

async fn scratchpads_list(client: &Client, scope: Option<&str>) -> Result<String> {
    Ok(capped(
        client
            .scratchpads(scope)
            .await?
            .iter()
            .filter(|pad| !pad.archived)
            .map(|pad| format!("{}  {}", pad.id, pad.title))
            .collect(),
    ))
}

/// The same text the web app's "Copy as Markdown" produces, so a pasted
/// document and an opened link read alike.
async fn scratchpad_open(client: &Client, id: &str, scope: Option<&str>) -> Result<String> {
    let pad = client.scratchpad(id).await?;
    in_scope(scope, pad.repository.as_deref(), "the scratchpad")?;
    Ok(format!("# {}\n\n{}", pad.title, pad.content))
}

async fn task_inspect(client: &Client, id: &str, scope: Option<&str>) -> Result<String> {
    let detail = client.task(id).await?;
    let task = &detail.task;
    in_scope(scope, Some(&task.spec.repository), "the task")?;
    let mut out = format!(
        "{} {}\n{}\n",
        task.id,
        status_word(task.status),
        task.spec.prompt
    );
    for exec in &task.executions {
        out.push_str(&format!(
            "attempt {} {} {}\n",
            exec.attempt,
            status_word(exec.status),
            exec.error.as_deref().unwrap_or("")
        ));
    }
    out.push_str(&result_block(task));
    // Only a plan task has versions; the request is skipped rather than
    // answered with an empty list for every other task.
    if task.spec.kind == TaskKind::Plan {
        for version in client.task_plans(id).await? {
            out.push_str(&format!(
                "plan v{} {}: {}\n",
                version.version,
                status_word(version.status),
                version
                    .plan
                    .steps
                    .iter()
                    .map(|step| step.title.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    out.push_str(&recent(&detail.events));
    if !task.scratchpad.is_empty() {
        out.push_str(&format!("notes:\n{}\n", task.scratchpad));
    }
    Ok(out)
}

async fn goal_inspect(client: &Client, id: &str, scope: Option<&str>) -> Result<String> {
    let detail = client.goal(id).await?;
    in_scope(scope, Some(&detail.summary.goal.repository), "the goal")?;
    let head = format!(
        "{}\nstatus: {}\n",
        detail.summary.goal.objective,
        status_word(detail.summary.status)
    );
    Ok(head + &joined(detail.tasks.iter().map(task_line)))
}

async fn session_open(client: &Client, id: &str, scope: Option<&str>) -> Result<String> {
    let detail = client.session(id).await?;
    in_scope(scope, Some(&detail.session.repository), "the session")?;
    let head = format!(
        "{} {} \"{}\"\n",
        detail.session.id,
        repo_short(&detail.session.repository),
        detail.session.title
    );
    Ok(head + &joined(detail.tasks.iter().map(task_line)))
}

/// What a person can attach from a prompt box: the documents and the open
/// work. Everything else is reachable by URI, but listing it would bury these.
async fn resources(client: &Client, scope: Option<&str>) -> Result<Value> {
    let resource = |kind: Kind, id: &str, name: &str| json!({ "uri": link(kind, id), "name": name, "mimeType": "text/markdown" });
    let mut list = Vec::new();
    for pad in client.scratchpads(scope).await? {
        if !pad.archived {
            list.push(resource(Kind::Scratchpad, &pad.id, &pad.title));
        }
    }
    for todo in client.todos(scope).await? {
        if todo.status == TodoStatus::Open {
            list.push(resource(Kind::Todo, &todo.id, &todo.title));
        }
    }
    Ok(json!({ "resources": list }))
}

/// A tail line says what was cut, so a model that needs the rest searches
/// instead of assuming the list was complete.
fn capped(rows: Vec<String>) -> String {
    let more = rows.len().saturating_sub(ROWS);
    let mut out = joined(rows.into_iter().take(ROWS));
    if more > 0 {
        out.push_str(&format!("\n…and {more} more"));
    }
    out
}

// ---------------------------------------------------------------------------
// A run's writes

async fn run_call(
    name: &str,
    args: &Value,
    client: &Client,
    task_id: &str,
    repository: &str,
) -> Result<String> {
    let scope = Some(repository);
    match name {
        "memory_propose" => propose(client, scope, task_id, args).await,
        "todo_create" => {
            let todo = client
                .create_todo(
                    scope,
                    string(args, "title")?,
                    text(args, "description"),
                    lgtm_protocol::Priority::default(),
                    None,
                    &[],
                )
                .await?;
            Ok(todo.id)
        }
        "todo_finish" => {
            let id = own_todo(client, args, repository).await?;
            client.finish_todo(id).await?;
            Ok("done".to_string())
        }
        "todo_comment" => {
            let id = own_todo(client, args, repository).await?;
            client.comment_todo(id, string(args, "body")?).await?;
            Ok("commented".to_string())
        }
        "todo_update" => {
            let id = own_todo(client, args, repository).await?;
            let patch = TodoPatch {
                title: optional(args, "title"),
                description: optional(args, "description"),
                priority: serde_json::from_value(args.get("priority").cloned().unwrap_or_default())
                    .ok(),
                ..TodoPatch::default()
            };
            client.update_todo(id, &patch).await?;
            Ok("todo saved".to_string())
        }
        "scratchpad_read" => Ok(client.task(task_id).await?.task.scratchpad),
        "scratchpad_write" => {
            client
                .set_scratchpad(task_id, string(args, "content")?)
                .await?;
            Ok("notes saved".to_string())
        }
        "scratchpad_create" => {
            let pad = client
                .create_scratchpad(
                    scope,
                    string(args, "title")?,
                    text(args, "content"),
                    &tags(args),
                )
                .await?;
            Ok(link(Kind::Scratchpad, &pad.id))
        }
        "scratchpad_update" => {
            let patch = ScratchpadPatch {
                title: optional(args, "title"),
                content: optional(args, "content"),
                archived: None,
                tags: args.get("tags").map(|_| tags(args)),
            };
            client
                .update_scratchpad(id_of(Kind::Scratchpad, string(args, "id")?)?, &patch)
                .await?;
            Ok("scratchpad saved".to_string())
        }
        "scratchpad_archive" => {
            let patch = ScratchpadPatch {
                archived: Some(true),
                ..ScratchpadPatch::default()
            };
            client
                .update_scratchpad(id_of(Kind::Scratchpad, string(args, "id")?)?, &patch)
                .await?;
            Ok("scratchpad archived".to_string())
        }
        "request_network" => request_network(client, task_id, args).await,
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

/// A run changes only the todos of its repository, or the ones that apply to
/// every repository; the check reads the todo first, so a wrong link fails
/// before anything is written.
async fn own_todo<'a>(client: &Client, args: &'a Value, repository: &str) -> Result<&'a str> {
    let id = id_of(Kind::Todo, string(args, "id")?)?;
    let detail = client.todo(id).await?;
    in_scope(
        Some(repository),
        detail.todo.repository.as_deref(),
        "the todo",
    )?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// The loop's writes

/// Only reachable with `LGTM_GOAL_ID`, so a plain agent run cannot drive the
/// goal it is one task of.
async fn orchestration_call(
    name: &str,
    args: &Value,
    client: &Client,
    goal: &str,
) -> Result<String> {
    match name {
        "task_create" => task_create(client, goal, args).await,
        "task_message" => {
            let task = string(args, "task_id")?;
            under_goal(client, task, goal).await?;
            client.tell(task, string(args, "text")?).await?;
            Ok("sent".to_string())
        }
        "task_retry" => {
            let into = Retry {
                runner: args.get("runner").and_then(Value::as_str).map(String::from),
                executor: serde_json::from_value(args.get("executor").cloned().unwrap_or_default())
                    .ok(),
            };
            let task = string(args, "task_id")?;
            under_goal(client, task, goal).await?;
            client.retry(task, &into).await?;
            Ok("requeued".to_string())
        }
        "task_approve" => {
            let task = string(args, "task_id")?;
            under_goal(client, task, goal).await?;
            client.approve_as_orchestrator(task).await?;
            Ok("approved".to_string())
        }
        "wait" => {
            client
                .set_attention(goal, Some(string(args, "reason")?))
                .await?;
            Ok("recorded".to_string())
        }
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

/// The pass was woken by one goal and may only act on that goal's tasks;
/// reading another goal's task is what the workspace tools are for. The
/// orchestrator enforces the same rule from the goal header; this check is
/// here to fail the model faster, and with a sentence it can act on.
async fn under_goal(client: &Client, task_id: &str, goal: &str) -> Result<()> {
    let detail = client.task(task_id).await?;
    if detail.task.spec.goal.as_deref() != Some(goal) {
        anyhow::bail!(
            "task {task_id} is under another goal; this pass may only act on tasks under goal {goal}"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Workspace reads

async fn workspace_call(name: &str, args: &Value, client: &Client) -> Result<String> {
    match name {
        "tasks_list" => tasks_list(client).await,
        "goals_list" => goals_list(client).await,
        "sessions_list" => sessions_list(client).await,
        "activity" => activity(client, args).await,
        "runner_list" => runner_list(client).await,
        _ => anyhow::bail!("no such tool: {name}"),
    }
}

async fn runner_list(client: &Client) -> Result<String> {
    Ok(joined(client.runners().await?.iter().map(|runner| {
        let executors: Vec<&str> = runner.info.executors.iter().map(|e| e.binary()).collect();
        format!(
            "{} {} running {}/{}",
            runner.info.name,
            executors.join(","),
            runner.running.len(),
            runner.info.slots,
        )
    })))
}

/// `created_by` is an id; a person reads names, so every workspace line
/// resolves it once per call.
async fn owners(client: &Client) -> Result<HashMap<String, String>> {
    Ok(client
        .users()
        .await?
        .into_iter()
        .map(|user| (user.id, user.name))
        .collect())
}

fn owner<'a>(owners: &'a HashMap<String, String>, id: Option<&'a str>) -> &'a str {
    match id {
        Some(id) => owners.get(id).map(String::as_str).unwrap_or(id),
        None => "-",
    }
}

/// The clone URL is too long for a line that already carries a prompt.
fn repo_short(url: &str) -> &str {
    let last = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last)
}

/// Compact enough to prefix every activity line: minutes, then hours, then
/// days.
fn age(now: u64, at: u64) -> String {
    let minutes = now.saturating_sub(at) / 60_000;
    match minutes {
        0..=59 => format!("{minutes}m"),
        60..=2879 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

async fn goals_list(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut goals = client.goals().await?;
    goals.sort_by_key(|summary| std::cmp::Reverse(summary.goal.created_at));
    Ok(capped(
        goals
            .iter()
            .map(|summary| {
                let goal = &summary.goal;
                let line = format!(
                    "{} {} {} {} \"{}\"",
                    goal.id,
                    status_word(summary.status),
                    owner(&owners, goal.created_by.as_deref()),
                    summary.tasks.total(),
                    first_line(&goal.objective),
                );
                match &goal.attention {
                    Some(why) => format!("{line}\n  needs a person: {why}"),
                    None => line,
                }
            })
            .collect(),
    ))
}

async fn sessions_list(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut sessions = client.sessions(None).await?;
    sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
    Ok(capped(
        sessions
            .iter()
            .map(|session| {
                let title = match session.title.is_empty() {
                    true => "-",
                    false => &session.title,
                };
                format!(
                    "{} {} {} \"{title}\"",
                    session.id,
                    owner(&owners, session.created_by.as_deref()),
                    repo_short(&session.repository),
                )
            })
            .collect(),
    ))
}

async fn tasks_list(client: &Client) -> Result<String> {
    let owners = owners(client).await?;
    let mut tasks = client.tasks().await?;
    tasks.sort_by_key(|task| std::cmp::Reverse(task.created_at));
    let all: Vec<&Task> = tasks.iter().collect();
    Ok(capped(
        tasks
            .iter()
            .map(|task| {
                let mut line = format!(
                    "{} {} {} {} \"{}\"",
                    task.id,
                    status_word(task.status),
                    owner(&owners, task.created_by.as_deref()),
                    repo_short(&task.spec.repository),
                    first_line(&task.spec.prompt),
                );
                if !task.status.is_terminal() {
                    for overlap in lgtm_protocol::overlaps(task, &all) {
                        line.push_str(&format!(
                            " [overlaps {}: {} files]",
                            overlap.task,
                            overlap.files.len()
                        ));
                    }
                }
                line
            })
            .collect(),
    ))
}

async fn activity(client: &Client, args: &Value) -> Result<String> {
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(30) as u32;
    let now = crate::now_ms();
    Ok(joined(client.activity(limit).await?.iter().map(|line| {
        let detail = match line.detail.is_empty() {
            true => String::new(),
            false => format!(": {}", line.detail),
        };
        format!(
            "{} {} {} {} {}{detail}",
            age(now, line.at),
            line.task,
            line.owner.as_deref().unwrap_or("-"),
            repo_short(&line.repository),
            line.event,
        )
    })))
}

fn task_line(task: &Task) -> String {
    let error = match &task.error {
        Some(error) => format!(" [error: {}]", first_line(error)),
        None => String::new(),
    };
    format!(
        "{} {} {}@{} \"{}\"{error}",
        task.id,
        status_word(task.status),
        task.spec.executor.binary(),
        task.runner.as_deref().unwrap_or("-"),
        first_line(&task.spec.prompt),
    )
}

/// The wire spelling, so the model reads the same words the API returns.
fn status_word(status: impl serde::Serialize) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn result_block(task: &Task) -> String {
    let Some(result) = &task.result else {
        return String::new();
    };
    let checks: String = result
        .validation
        .iter()
        .map(|check| {
            let word = if check.ok { "passed" } else { "failed" };
            format!("check {} {word}\n", check.name)
        })
        .collect();
    let findings: String = result
        .review
        .iter()
        .flat_map(|review| &review.findings)
        .filter(|finding| finding.severity == lgtm_protocol::Severity::Blocking)
        .map(|finding| format!("blocking: {}\n", finding.message))
        .collect();
    format!(
        "{checks}{findings}changed files: {}\n",
        result.changed_files.join(", ")
    )
}

/// The last few things the agent said or ran, oldest first.
fn recent(events: &[lgtm_protocol::StoredEvent]) -> String {
    let mut lines: Vec<String> = events
        .iter()
        .rev()
        .filter_map(|stored| match &stored.event {
            lgtm_protocol::TaskEvent::Progress { text } => Some(text.clone()),
            lgtm_protocol::TaskEvent::Command { command } => Some(format!("$ {command}")),
            _ => None,
        })
        .take(RECENT_EVENTS)
        .collect();
    lines.reverse();
    match lines.is_empty() {
        true => String::new(),
        false => format!("recent:\n{}\n", lines.join("\n")),
    }
}

/// Repository and base branch come from the goal, never from the model: a
/// task it invents must land where the goal's work already is.
async fn task_create(client: &Client, goal: &str, args: &Value) -> Result<String> {
    let detail = client.goal(goal).await?;
    let first = detail
        .tasks
        .first()
        .ok_or_else(|| anyhow::anyhow!("the goal has no task to copy a base branch from"))?;
    let spec = TaskSpec {
        repository: detail.summary.goal.repository.clone(),
        base_branch: first.spec.base_branch.clone(),
        prompt: format!("{}\n\n{}", string(args, "title")?, string(args, "prompt")?),
        executor: first.spec.executor,
        runner: None,
        issue: None,
        linear: None,
        kind: TaskKind::Run,
        parent: None,
        depends_on: serde_json::from_value(args.get("depends_on").cloned().unwrap_or(json!([])))?,
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: first.spec.sandbox,
        requirements: first.spec.requirements.clone(),
        review_executor: None,
        model: None,
        reasoning_effort: None,
        goal: Some(goal.to_string()),
        allowed_hosts: Vec::new(),
        session: None,
        created_by: None,
    };
    Ok(client.create_task(&spec).await?.id)
}

/// An agent cannot write what every later run is told: the memory it
/// proposes waits unapproved until a person runs `lgtm memory approve`.
async fn propose(
    client: &Client,
    repository: Option<&str>,
    task_id: &str,
    args: &Value,
) -> Result<String> {
    let memory = client
        .propose_memory(repository, string(args, "content")?, task_id)
        .await?;
    Ok(proposed_reply(&memory.id))
}

fn proposed_reply(id: &str) -> String {
    format!("proposed {id}; a person approves it with: lgtm memory approve {id}")
}

/// A run can't be paused mid-flight to ask a person, so the request is only
/// recorded; `lgtm allow` answers it before the task's next run.
async fn request_network(client: &Client, task_id: &str, args: &Value) -> Result<String> {
    let host = string(args, "host")?;
    client
        .request_permission(task_id, "network", host, string(args, "reason")?)
        .await?;
    Ok(format!(
        "recorded; a person can allow it with: lgtm allow {task_id} {host}"
    ))
}

// ---------------------------------------------------------------------------
// Arguments

fn string<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} is required"))
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or("")
}

fn optional(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn tags(args: &Value) -> Vec<String> {
    args.get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn joined(lines: impl Iterator<Item = String>) -> String {
    lines.collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// Tool schemas

fn tools(env: &Env) -> Value {
    let mut tools = read_tools();
    match env {
        Env::Run { .. } => tools.extend(run_tools()),
        Env::Orchestrate { .. } => {
            tools.extend(run_tools());
            tools.extend(goal_tools());
            tools.extend(workspace_tools());
        }
        Env::Ask => tools.extend(workspace_tools()),
    }
    Value::Array(tools)
}

fn string_schema(about: &str) -> Value {
    json!({ "type": "string", "description": about })
}

fn id_schema(kind: Kind) -> Value {
    json!({ "id": string_schema(&format!("Its id, or its lgtm://{}/<id> link.", kind.path())) })
}

fn read_tools() -> Vec<Value> {
    vec![
        tool("open", "Read any workspace object by its lgtm:// link: a task, todo, goal, memory, session or scratchpad. A prompt, todo or message that carries a link means: open it before acting.", json!({ "link": string_schema("An lgtm://<kind>/<id> link.") }), &["link"]),
        tool("search", "Find tasks, todos, goals, memories, sessions and scratchpads whose text contains a phrase. Rows start with the link that opens them.", json!({ "query": string_schema("A word or phrase; case does not matter.") }), &["query"]),
        tool("memories_list", "Facts recorded for this repository that every agent run is told: id, then the fact.", json!({}), &[]),
        tool("memory_open", "One memory: the fact and whether a person has approved it.", id_schema(Kind::Memory), &["id"]),
        tool("todos_list", "Open todos: id, owner, repository, title.", json!({}), &[]),
        tool("todo_open", "One todo in full: status, priority, description, assignee, blockers, its task, and the comment thread.", id_schema(Kind::Todo), &["id"]),
        tool("scratchpads_list", "Shared scratchpads: id, then title.", json!({}), &[]),
        tool("scratchpad_open", "One shared scratchpad as markdown, its title as the heading.", id_schema(Kind::Scratchpad), &["id"]),
        tool("task_inspect", "Everything recorded for one task: its prompt, attempts, checks, blocking findings, changed files, plan versions, recent activity and notes. On a retry, read your own task first.", json!({ "task_id": string_schema("The task's id, or its lgtm://tasks/<id> link.") }), &["task_id"]),
        tool("goal_inspect", "A goal's objective, its status, and one line per task under it. The orchestration loop may omit the id for its own goal.", id_schema(Kind::Goal), &[]),
        tool("session_open", "A chat session: its title, repository, and one line per task it produced.", id_schema(Kind::Session), &["id"]),
    ]
}

fn run_tools() -> Vec<Value> {
    let string = string_schema;
    vec![
        tool("memory_propose", "Propose a fact worth telling every later run. It waits as a pending memory until a person approves it.", json!({ "content": string("The fact, in one sentence.") }), &["content"]),
        tool("todo_create", "Note work that should happen but is not part of this task.", json!({ "title": string("One line."), "description": string("Optional detail.") }), &["title"]),
        tool("todo_finish", "Mark a todo done, once its work is in this task.", id_schema(Kind::Todo), &["id"]),
        tool("todo_comment", "Leave a note on a todo: what was found, or why it could not be done.", json!({ "id": string("The todo's id, or its lgtm://todos/<id> link."), "body": string("The note, in markdown.") }), &["id", "body"]),
        tool("todo_update", "Change a todo's title, description or priority.", json!({ "id": string("The todo's id, or its lgtm://todos/<id> link."), "title": string("The new title."), "description": string("The new description."), "priority": { "type": "string", "enum": ["low", "medium", "high"] } }), &["id"]),
        tool("scratchpad_read", "This task's own working notes, private to it.", json!({}), &[]),
        tool("scratchpad_write", "Replace this task's own working notes.", json!({ "content": string("The full notes, in markdown.") }), &["content"]),
        tool("scratchpad_create", "Start a shared scratchpad for this repository. Returns its link.", json!({ "title": string("One line."), "content": string("The document, in markdown."), "tags": { "type": "array", "items": { "type": "string" } } }), &["title"]),
        tool("scratchpad_update", "Replace a shared scratchpad's title, content or tags.", json!({ "id": string("The scratchpad's id, or its lgtm://scratchpads/<id> link."), "title": string("The new title."), "content": string("The full document, in markdown."), "tags": { "type": "array", "items": { "type": "string" } } }), &["id"]),
        tool("scratchpad_archive", "Archive a shared scratchpad. A person can restore it.", id_schema(Kind::Scratchpad), &["id"]),
        tool("request_network", "Ask a person to allow this task to reach a host its sandbox refused. Recorded for the task's next run, not this one.", json!({ "host": string("The host to allow, e.g. registry.internal."), "reason": string("Why the run needs it.") }), &["host", "reason"]),
    ]
}

fn workspace_tools() -> Vec<Value> {
    vec![
        tool("tasks_list", "Every task in the workspace: id, status, owner, repository, prompt — and which unmerged tasks changed the same files.", json!({}), &[]),
        tool("goals_list", "Every goal in the workspace: id, status, owner, task count, objective.", json!({}), &[]),
        tool("sessions_list", "Every chat session in the workspace: id, owner, repository, title.", json!({}), &[]),
        tool("activity", "The most recent events across every task: who did what, where.", json!({ "limit": { "type": "integer", "description": "How many lines, default 30." } }), &[]),
        tool("runner_list", "The connected runners and what they are running.", json!({}), &[]),
    ]
}

fn goal_tools() -> Vec<Value> {
    let string = string_schema;
    vec![
        tool("task_create", "Add a task the goal needs. It runs in the goal's repository, off the same base branch.", json!({
            "title": string("One line."),
            "prompt": string("Full instructions for a coding agent."),
            "depends_on": { "type": "array", "items": { "type": "string" }, "description": "Ids of tasks under this goal that must finish first." },
            "reason": string("Why the goal needs it."),
        }), &["title", "prompt"]),
        tool("task_message", "Send a follow-up to a task so its agent fixes something itself.", json!({ "task_id": string("Id of a task under this goal."), "text": string("What to tell the agent."), "reason": string("Why.") }), &["task_id", "text"]),
        tool("task_retry", "Requeue a task that crashed or timed out.", json!({ "task_id": string("Id of a task under this goal."), "runner": string("Optional runner to move it to."), "executor": string("Optional executor to swap to: claude or codex."), "reason": string("Why.") }), &["task_id"]),
        tool("task_approve", "Approve and push a task. Refused unless the checks passed and no blocking review finding is left.", json!({ "task_id": string("Id of a task under this goal."), "reason": string("Why.") }), &["task_id"]),
        tool("wait", "Stop and leave the goal to a person. The next task or message under the goal clears it.", json!({ "reason": string("What a person has to decide or do.") }), &["reason"]),
    ]
}

fn tool(name: &str, about: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": about,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(goal_id: Option<&str>) -> Env {
        let task_id = "t1".to_string();
        let repository = "https://example.com/r.git".to_string();
        match goal_id {
            Some(goal_id) => Env::Orchestrate {
                task_id,
                repository,
                goal_id: goal_id.to_string(),
            },
            None => Env::Run {
                task_id,
                repository,
            },
        }
    }

    async fn answer_in(request: Value, env: Env) -> Option<Value> {
        let client = Client::new("http://127.0.0.1:1", "tok");
        handle(&request, &client, &env).await
    }

    async fn answer(request: Value, goal_id: Option<&str>) -> Option<Value> {
        answer_in(request, env(goal_id)).await
    }

    async fn names_in(env: Env) -> Vec<String> {
        let reply = answer_in(
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            env,
        )
        .await
        .unwrap();
        reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    async fn tool_names(goal_id: Option<&str>) -> Vec<String> {
        names_in(env(goal_id)).await
    }

    fn names(tools: Vec<Value>) -> Vec<String> {
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn the_tool_names_and_their_schemas_cannot_drift() {
        assert_eq!(names(read_tools()), READ_TOOLS);
        assert_eq!(names(run_tools()), RUN_TOOLS);
        assert_eq!(names(workspace_tools()), WORKSPACE_TOOLS);
    }

    #[test]
    fn proposed_reply_names_the_approve_command() {
        assert_eq!(
            proposed_reply("m1"),
            "proposed m1; a person approves it with: lgtm memory approve m1"
        );
    }

    #[tokio::test]
    async fn initialize_names_the_protocol_the_server_and_both_capabilities() {
        let reply = answer(
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL);
        assert_eq!(reply["result"]["serverInfo"]["name"], "lgtm");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        assert!(reply["result"]["capabilities"]["resources"].is_object());
    }

    #[tokio::test]
    async fn a_run_without_a_goal_gets_the_reads_and_its_own_writes() {
        let expected: Vec<&str> = READ_TOOLS.iter().chain(RUN_TOOLS.iter()).copied().collect();
        assert_eq!(tool_names(None).await, expected);
    }

    #[tokio::test]
    async fn a_goal_id_unlocks_the_orchestration_tools() {
        let names = tool_names(Some("g1")).await;
        for name in [
            "task_create",
            "task_message",
            "task_retry",
            "task_approve",
            "wait",
        ] {
            assert!(
                names.contains(&name.to_string()),
                "{name} missing: {names:?}"
            );
        }
    }

    #[tokio::test]
    async fn orchestrate_mode_adds_the_workspace_tools() {
        let with_goal = tool_names(Some("g1")).await;
        let plain = tool_names(None).await;
        for name in WORKSPACE_TOOLS {
            assert!(
                with_goal.contains(&name.to_string()),
                "{name} missing: {with_goal:?}"
            );
            assert!(!plain.contains(&name.to_string()), "{name} leaked to a run");
        }
    }

    #[tokio::test]
    async fn ask_mode_serves_only_reads() {
        let expected: Vec<&str> = READ_TOOLS
            .iter()
            .chain(WORKSPACE_TOOLS.iter())
            .copied()
            .collect();
        assert_eq!(names_in(Env::Ask).await, expected);
    }

    #[tokio::test]
    async fn a_write_tool_is_refused_in_ask_mode() {
        for name in RUN_TOOLS
            .iter()
            .chain(["task_create", "task_approve"].iter())
        {
            let call = json!({
                "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                "params": { "name": name, "arguments": { "task_id": "t2", "id": "x" } },
            });
            let reply = answer_in(call, Env::Ask).await.unwrap();
            assert_eq!(reply["result"]["isError"], true, "{name}");
            assert!(
                reply["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("no such tool"),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn a_workspace_tool_is_refused_in_a_run() {
        let call = json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": { "name": "tasks_list", "arguments": {} },
        });
        let reply = answer(call, None).await.unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("no such tool"));
    }

    #[test]
    fn every_kind_has_a_link_that_parses_back() {
        for kind in Kind::ALL {
            assert_eq!(
                parse_link(&link(kind, "ab12cd34")).unwrap(),
                (kind, "ab12cd34")
            );
        }
    }

    #[test]
    fn a_scratchpad_link_may_carry_the_repository_and_a_trailing_slash() {
        for value in [
            "lgtm://scratchpads/https%3A%2F%2Fexample.com%2Fr.git/sp1",
            "lgtm://scratchpads/sp1",
            "lgtm://scratchpads/sp1/",
        ] {
            assert_eq!(
                parse_link(value).unwrap(),
                (Kind::Scratchpad, "sp1"),
                "{value}"
            );
        }
    }

    #[test]
    fn a_link_without_a_kind_or_an_id_is_refused_by_name() {
        let err = parse_link("lgtm://widgets/w1").unwrap_err().to_string();
        assert!(err.contains("names no kind"), "{err}");
        let err = parse_link("lgtm://todos/").unwrap_err().to_string();
        assert!(err.contains("names no id"), "{err}");
        let err = parse_link("t1").unwrap_err().to_string();
        assert!(err.contains("not an lgtm:// link"), "{err}");
    }

    #[test]
    fn a_kinds_tool_takes_a_bare_id_or_its_own_link_only() {
        assert_eq!(id_of(Kind::Todo, "td1").unwrap(), "td1");
        assert_eq!(id_of(Kind::Todo, "lgtm://todos/td1").unwrap(), "td1");
        let err = id_of(Kind::Todo, "lgtm://tasks/t1")
            .unwrap_err()
            .to_string();
        assert!(err.contains("is a tasks link, not a todos one"), "{err}");
    }

    #[test]
    fn a_run_reads_its_repository_and_the_global_ones() {
        let repo = Some("https://example.com/r.git");
        assert!(in_scope(repo, repo, "the todo").is_ok());
        assert!(in_scope(repo, None, "the todo").is_ok());
        assert!(in_scope(None, Some("https://example.com/other.git"), "the todo").is_ok());
        let err = in_scope(repo, Some("https://example.com/other.git"), "the todo")
            .unwrap_err()
            .to_string();
        assert_eq!(err, "the todo belongs to another repository");
    }

    #[test]
    fn lists_stop_at_fifty_rows_and_say_what_was_cut() {
        let rows: Vec<String> = (1..=ROWS + 1).map(|n| n.to_string()).collect();
        let out = capped(rows.clone());
        assert_eq!(out.lines().count(), ROWS + 1);
        assert!(out.ends_with("50\n…and 1 more"), "{out}");
        assert_eq!(capped(rows[..ROWS].to_vec()).lines().count(), ROWS);
        assert!(!capped(rows[..ROWS].to_vec()).contains("more"));
    }

    #[tokio::test]
    async fn a_resource_uri_of_no_kind_is_a_not_found_error() {
        let reply = answer(
            json!({"jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": { "uri": "lgtm://widgets/w1" }}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(reply["error"]["code"], RESOURCE_NOT_FOUND);
        assert!(reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("names no kind"));
    }

    #[test]
    fn repo_short_keeps_the_repository_name() {
        assert_eq!(repo_short("https://github.com/o/lgtm.git"), "lgtm");
        assert_eq!(repo_short("https://github.com/o/lgtm"), "lgtm");
        assert_eq!(repo_short("lgtm"), "lgtm");
    }

    #[test]
    fn age_counts_minutes_then_hours_then_days() {
        let now = 10 * 24 * 60 * 60_000;
        assert_eq!(age(now, now), "0m");
        assert_eq!(age(now, now - 59 * 60_000), "59m");
        assert_eq!(age(now, now - 60 * 60_000), "1h");
        assert_eq!(age(now, now - 47 * 60 * 60_000), "47h");
        assert_eq!(age(now, now - 48 * 60 * 60_000), "2d");
    }

    #[tokio::test]
    async fn an_orchestration_tool_is_refused_without_a_goal() {
        let call = json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "task_create", "arguments": { "title": "t", "prompt": "p" } },
        });
        let reply = answer(call, None).await.unwrap();
        assert_eq!(reply["result"]["isError"], true);
        assert!(reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("no such tool"));
    }

    #[tokio::test]
    async fn an_unknown_method_is_an_error_reply() {
        let reply = answer(
            json!({"jsonrpc": "2.0", "id": 3, "method": "prompts/list"}),
            None,
        )
        .await
        .unwrap();
        assert_eq!(reply["error"]["code"], -32601);
        assert!(reply.get("result").is_none());
    }

    #[tokio::test]
    async fn a_notification_gets_no_reply() {
        assert!(answer(
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            None
        )
        .await
        .is_none());
    }
}
