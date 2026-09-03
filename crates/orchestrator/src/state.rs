//! Shared state and every status transition, kept free of I/O so it can be
//! tested without sockets or files.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use lgtm_protocol::{
    first_line_title, goal_status, Batch, Goal, GoalSummary, Memory, MemorySource,
    OrchestratorMessage, Project, Scratchpad, Session, StoredEvent, Task, TaskEvent, TaskId,
    TaskSpec, TaskStatus, Todo, TodoComment, TodoStatus, Verification,
};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::commands::RetryInto;
use crate::persist::Persist;
pub use crate::runner::{Conn, RunnerConn};

const LIVE_CAPACITY: usize = 1024;
/// Terminal output kept per task, in bytes. Enough for someone attaching late
/// to see the screen they walked in on; it is a pipe, not history, so nothing
/// older is worth the memory.
const SCROLLBACK_MAX: usize = 64 * 1024;

pub struct App {
    pub token: String,
    pub state: Mutex<State>,
    /// Writes go to a task that owns the directory; a request handler never
    /// holds a path of its own.
    pub persist: mpsc::UnboundedSender<Persist>,
    /// `None` when no `GITHUB_TOKEN` was set, which turns the pull request,
    /// CI and merge routes off.
    pub github: Option<lgtm_github::GitHub>,
    /// `None` when no `LINEAR_API_KEY` was set, which turns the from-linear
    /// route and every issue sync off.
    pub linear: Option<lgtm_linear::Linear>,
    /// Where events a person would want to see are POSTed; `None` posts none.
    pub webhook: Option<String>,
    /// Model that decides the next step for a goal, off unless
    /// `serve --orchestrate` named one.
    pub orchestrate: Option<lgtm_protocol::Executor>,
    /// Where the orchestration loop's own tools reach this orchestrator.
    pub base_url: String,
    /// Goals whose loop is running right now, so the API can say so.
    pub orchestrating: Mutex<HashSet<String>>,
    /// Each ask holds an agent subprocess for up to five minutes, so a
    /// couple at a time is plenty and anything more is refused, not queued.
    pub asking: tokio::sync::Semaphore,
    /// Utility inference calls waiting on a runner's answer, by id. Not part
    /// of `State`: nothing about them is persisted or scheduled.
    pub inferring: Mutex<HashMap<String, oneshot::Sender<Result<String, String>>>>,
}

impl App {
    /// Rewrites the task for each id a transition reported as changed, and
    /// appends any events on it that have not been written yet.
    pub fn persist_ids(&self, state: &mut State, ids: &[TaskId]) {
        for id in ids {
            let Some(rec) = state.tasks.get_mut(id) else {
                continue;
            };
            let _ = self.persist.send(Persist::Task(Box::new(rec.task.clone())));
            for event in &rec.events[rec.written..] {
                let _ = self.persist.send(Persist::Event {
                    task_id: rec.task.id.clone(),
                    event: event.clone(),
                });
            }
            rec.written = rec.events.len();
            for (name, bytes) in std::mem::take(&mut rec.artefacts) {
                let _ = self.persist.send(Persist::Artefact {
                    task_id: rec.task.id.clone(),
                    name,
                    bytes,
                });
            }
        }
        for id in std::mem::take(&mut state.dirty_goals) {
            if let Some(goal) = state.goals.get(&id) {
                self.persist_goal(goal);
            }
        }
    }

    /// The goals whose loop is running, for [`State::goal_summary`].
    pub fn running_goals(&self) -> HashSet<String> {
        self.orchestrating.lock().unwrap().clone()
    }

    /// A bearer token for a one-time push, or `None` to leave it to the
    /// runner's own git credentials.
    ///
    /// The workspace's own credential comes first: it names who is pushing,
    /// where the orchestrator-wide token only says which machine is. The
    /// static token stays as the fallback so an orchestrator with no
    /// credentials configured behaves exactly as it did before.
    /// Takes the state it reads rather than locking: every caller already
    /// holds the lock, and re-entering it here deadlocks the orchestrator on
    /// the auto-approve path.
    pub fn push_token(&self, state: &State, task: &Task) -> Option<String> {
        state
            .credentials
            .resolve(task.workspace.as_deref(), task.created_by.as_deref(), &[])
            .token
            .or_else(|| push_token(self.github.as_ref(), task))
    }

    /// Fetches the installation token for `task`'s repository before anyone
    /// approves, so `push_token` finds one in the cache and stays sync. Does
    /// nothing unless a GitHub App is configured.
    pub fn warm_push_token(&self, task: &Task) {
        let Some(github) = self.github.clone() else {
            return;
        };
        if !github.has_app() {
            return;
        }
        let Some(repo) = lgtm_github::parse_repo(&task.spec.repository) else {
            return;
        };
        tokio::spawn(async move {
            if let Err(err) = github.installation_token(&repo).await {
                tracing::warn!(?err, "fetching a github app installation token");
            }
        });
    }

    pub fn persist_credentials(&self, state: &State) {
        let _ = self
            .persist
            .send(Persist::Credentials(Box::new(state.credentials.clone())));
    }

    pub fn persist_batch(&self, batch: &Batch) {
        let _ = self.persist.send(Persist::Batch(batch.clone()));
    }

    pub fn persist_memory(&self, memory: &Memory) {
        let _ = self.persist.send(Persist::Memory(memory.clone()));
    }

    pub fn forget_memory(&self, id: &str) {
        let _ = self.persist.send(Persist::RemoveMemory(id.to_string()));
    }

    pub fn persist_goal(&self, goal: &Goal) {
        let _ = self.persist.send(Persist::Goal(goal.clone()));
    }

    pub fn persist_session(&self, session: &Session) {
        let _ = self.persist.send(Persist::Session(session.clone()));
    }

    pub fn remove_session_record(&self, id: &str) {
        let _ = self.persist.send(Persist::RemoveSession(id.to_string()));
    }

    pub fn persist_users(&self, state: &State) {
        let _ = self
            .persist
            .send(Persist::Users(state.users.values().cloned().collect()));
    }

    pub fn persist_todo(&self, todo: &Todo) {
        let _ = self.persist.send(Persist::Todo(todo.clone()));
    }

    pub fn forget_todo(&self, id: &str) {
        let _ = self.persist.send(Persist::RemoveTodo(id.to_string()));
    }

    pub fn persist_todo_comment(&self, comment: &TodoComment) {
        let _ = self.persist.send(Persist::TodoComment(comment.clone()));
    }

    pub fn forget_todo_comment(&self, id: &str) {
        let _ = self
            .persist
            .send(Persist::RemoveTodoComment(id.to_string()));
    }

    pub fn persist_scratchpad(&self, scratchpad: &Scratchpad) {
        let _ = self.persist.send(Persist::Scratchpad(scratchpad.clone()));
    }

    pub fn persist_project(&self, project: &Project) {
        let _ = self.persist.send(Persist::Project(project.clone()));
    }

    /// Writes every project a request created or took a number from.
    pub fn persist_projects(&self, state: &mut State) {
        for id in std::mem::take(&mut state.dirty_projects) {
            if let Some(project) = state.projects.get(&id) {
                let _ = self.persist.send(Persist::Project(project.clone()));
            }
        }
    }

    pub fn forget_scratchpad(&self, id: &str) {
        let _ = self.persist.send(Persist::RemoveScratchpad(id.to_string()));
    }
}

/// Everyone besides the person who raised the task who has worked on it, in
/// the order they first did. A follow-up is how a second person joins a task,
/// so its sender is what earns their agent a co-author trailer.
fn contributors(created_by: Option<&str>, events: &[StoredEvent]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for event in events {
        let TaskEvent::Message { by: Some(by), .. } = &event.event else {
            continue;
        };
        if Some(by.as_str()) != created_by && !out.contains(by) {
            out.push(by.clone());
        }
    }
    out
}

/// Shared by `App::push_token` and `orchestrate::approve`, which only holds
/// the state lock and not `App` itself.
pub(crate) fn push_token(github: Option<&lgtm_github::GitHub>, task: &Task) -> Option<String> {
    let github = github?;
    let repo = lgtm_github::parse_repo(&task.spec.repository)?;
    if let Some(token) = github.cached_installation_token(&repo) {
        tracing::debug!(repo = %task.spec.repository, "pushing with an installation token");
        return Some(token);
    }
    tracing::debug!(repo = %task.spec.repository, "pushing with the static token");
    Some(github.token().to_string())
}

#[derive(Default)]
pub struct State {
    pub runners: HashMap<String, RunnerConn>,
    pub tasks: HashMap<TaskId, TaskRecord>,
    /// Backlog imports, by id. A task points back with `spec.batch`.
    pub batches: HashMap<String, Batch>,
    /// Durable facts every run in a repository is told, by id.
    pub memories: HashMap<String, Memory>,
    /// Goals, by id. A task points back with `spec.goal`.
    pub goals: HashMap<String, Goal>,
    /// Chat threads, by id. A task points back with `spec.session`.
    pub sessions: HashMap<String, Session>,
    /// Lightweight notes about work to do, by id.
    pub todos: HashMap<String, Todo>,
    /// Comments on todos, by id. A comment points back with `todo`.
    pub todo_comments: HashMap<String, TodoComment>,
    /// Standalone markdown documents, by id.
    pub scratchpads: HashMap<String, Scratchpad>,
    /// One per repository todos are filed against, by id; it holds the prefix
    /// and the next number.
    pub projects: HashMap<String, Project>,
    /// Projects whose stored copy is behind, drained by [`App::persist_projects`].
    pub(crate) dirty_projects: Vec<String>,
    /// Accept tasks no connected runner can run, because provisioning is on
    /// and a runner for them is a queue away.
    pub queue_without_runners: bool,
    /// Model to run a task of each kind on (`plan`, `run`) when its spec
    /// names none.
    pub models: HashMap<String, String>,
    /// Goals whose stored copy is behind, drained by [`App::persist_ids`].
    pub(crate) dirty_goals: Vec<String>,
    /// How `candidate` breaks a free-slot tie.
    pub prefer: crate::Prefer,
    /// Stamped on everything this orchestrator creates; one per orchestrator
    /// until teams exist.
    pub workspace: Option<String>,
    /// People with tokens of their own, by user id.
    pub users: HashMap<String, crate::users::UserRecord>,
    /// Who this workspace's commits are attributed to, and what pushes them.
    pub credentials: crate::credentials::CredentialStore,
}

pub struct TaskRecord {
    pub task: Task,
    pub events: Vec<StoredEvent>,
    pub live: broadcast::Sender<StoredEvent>,
    /// Output from the task's shell; `None` means the shell closed.
    pub terminal: broadcast::Sender<Option<String>>,
    scrollback: VecDeque<String>,
    scrollback_len: usize,
    /// `events[..written]` are already on disk. A freshly created task has
    /// none yet; a task loaded from disk brings its whole history as already
    /// written, so `persist_ids` only ever appends what is new.
    written: usize,
    /// Artefact bytes taken off their events, waiting for the same
    /// `persist_ids` that writes the events they came with.
    artefacts: Vec<(String, Vec<u8>)>,
}

impl TaskRecord {
    pub fn new(task: Task, events: Vec<StoredEvent>) -> Self {
        let (live, _) = broadcast::channel(LIVE_CAPACITY);
        let (terminal, _) = broadcast::channel(LIVE_CAPACITY);
        let written = events.len();
        Self {
            task,
            events,
            live,
            terminal,
            scrollback: VecDeque::new(),
            scrollback_len: 0,
            written,
            artefacts: Vec::new(),
        }
    }

    /// Records one chunk of terminal output and hands it to whoever is attached.
    pub fn push_terminal(&mut self, data: String) {
        let _ = self.terminal.send(Some(data.clone()));
        self.scrollback_len += data.len();
        self.scrollback.push_back(data);
        while self.scrollback_len > SCROLLBACK_MAX {
            let Some(dropped) = self.scrollback.pop_front() else {
                break;
            };
            self.scrollback_len -= dropped.len();
        }
    }

    /// The recent terminal output, for a client that just attached.
    pub fn scrollback(&self) -> String {
        self.scrollback.iter().map(String::as_str).collect()
    }

    /// Head of the pushed branch, from the last `Pushed` event that carried
    /// one. Runners before phase 5 pushed without a sha.
    pub fn pushed_sha(&self) -> Option<String> {
        self.events
            .iter()
            .rev()
            .find_map(|stored| match &stored.event {
                TaskEvent::Pushed { sha, .. } if !sha.is_empty() => Some(sha.clone()),
                _ => None,
            })
    }

    /// The last decision policy recorded for `action`, so a caller polling in
    /// a loop can log a change rather than one line per poll.
    pub fn last_policy_decision(&self, action: &str) -> Option<&TaskEvent> {
        self.events.iter().rev().map(|stored| &stored.event).find(
            |event| matches!(event, TaskEvent::PolicyDecision { action: a, .. } if a == action),
        )
    }
}

/// Everything needed to open one pull request, resolved under the lock so the
/// GitHub call itself can run without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrPlan {
    pub pull: lgtm_github::NewPull,
    /// Head sha to poll checks for, empty when the runner did not report one.
    pub sha: String,
}

pub(crate) const TITLE_MAX: usize = 72;

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

/// How `[policy] models` names a kind.
pub fn kind_key(kind: lgtm_protocol::TaskKind) -> &'static str {
    match kind {
        lgtm_protocol::TaskKind::Plan => "plan",
        lgtm_protocol::TaskKind::Run => "run",
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Eight lowercase hex characters, which `persist::file_stem` accepts.
pub(crate) fn random_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

impl State {
    /// Whose names go on this task's commits. Resolved here because the
    /// credentials it reads never leave the orchestrator.
    pub fn authorship(&self, task: &Task) -> lgtm_protocol::Authorship {
        let events = self
            .tasks
            .get(&task.id)
            .map(|rec| rec.events.as_slice())
            .unwrap_or_default();
        let others = contributors(task.created_by.as_deref(), events);
        self.credentials
            .resolve(
                task.workspace.as_deref(),
                task.created_by.as_deref(),
                &others,
            )
            .authorship
    }

    pub(crate) fn new_id(&self) -> TaskId {
        std::iter::repeat_with(random_id)
            .find(|id| !self.tasks.contains_key(id))
            .unwrap_or_default()
    }

    pub(crate) fn new_batch_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.batches.contains_key(id))
            .unwrap_or_default()
    }

    fn new_memory_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.memories.contains_key(id))
            .unwrap_or_default()
    }

    pub fn create_memory(
        &mut self,
        repository: Option<String>,
        content: String,
        source: MemorySource,
        proposed_by: Option<TaskId>,
        created_by: Option<String>,
    ) -> Memory {
        // An agent cannot write what every later run is told, whatever
        // verification the request claims: its source forces the state.
        let verification = match source {
            MemorySource::Agent => Verification::AgentProposed,
            MemorySource::User => Verification::UserApproved,
        };
        let memory = Memory {
            id: self.new_memory_id(),
            repository,
            content,
            created_at: now_ms(),
            source,
            verification,
            proposed_by,
            workspace: self.workspace.clone(),
            created_by,
        };
        self.memories.insert(memory.id.clone(), memory.clone());
        memory
    }

    /// Marks a proposal as told from now on. `None` if there is no such
    /// memory.
    pub fn approve_memory(&mut self, id: &str) -> Option<Memory> {
        let memory = self.memories.get_mut(id)?;
        memory.verification = Verification::UserApproved;
        Some(memory.clone())
    }

    /// Rewrites a memory's content. `None` if there is no such memory.
    ///
    /// An approved memory stays approved, but rewriting an agent's proposal
    /// approves it: putting the words in yourself is a stronger sign-off than
    /// pressing approve.
    pub fn edit_memory(&mut self, id: &str, content: String) -> Option<Memory> {
        let memory = self.memories.get_mut(id)?;
        memory.content = content;
        memory.verification = Verification::UserApproved;
        Some(memory.clone())
    }

    pub fn remove_memory(&mut self, id: &str) -> bool {
        self.memories.remove(id).is_some()
    }

    /// What a run in `repository` is told, oldest first.
    /// Whether an object stamped `workspace` belongs to this orchestrator.
    /// Scoping is deliberately read-side only for now — lists and the
    /// memories agents are told. By-id reads, stats, and the scheduler stay
    /// workspace-blind until teams give two workspaces a reason to share
    /// one data directory.
    pub(crate) fn in_workspace(&self, workspace: Option<&str>) -> bool {
        lgtm_protocol::same_workspace(workspace, self.workspace.as_deref())
    }

    /// A memory a run is told must also be one the operator can see in the
    /// list, so a foreign-workspace memory never steers agents invisibly.
    pub(crate) fn memories_for(&self, repository: &str) -> Vec<Memory> {
        let mut out: Vec<Memory> = self
            .memories
            .values()
            .filter(|memory| {
                memory.is_told_to(repository) && self.in_workspace(memory.workspace.as_deref())
            })
            .cloned()
            .collect();
        out.sort_by_key(|memory| memory.created_at);
        out
    }

    pub(crate) fn new_goal_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.goals.contains_key(id))
            .unwrap_or_default()
    }

    pub fn create_goal(
        &mut self,
        objective: String,
        repository: String,
        created_by: Option<String>,
    ) -> Goal {
        let goal = Goal {
            id: self.new_goal_id(),
            objective,
            repository,
            created_at: now_ms(),
            attention: None,
            workspace: self.workspace.clone(),
            created_by,
        };
        tracing::info!(goal = %goal.id, "goal created");
        self.goals.insert(goal.id.clone(), goal.clone());
        goal
    }

    /// Records why the loop stopped for a person, or clears it with `None`.
    /// Returns whether the goal exists.
    pub fn set_attention(&mut self, id: &str, reason: Option<String>) -> bool {
        let Some(goal) = self.goals.get_mut(id) else {
            return false;
        };
        if goal.attention != reason {
            goal.attention = reason;
            self.dirty_goals.push(id.to_string());
        }
        true
    }

    /// The goal's tasks, oldest first. `spec.goal` is the only record of
    /// membership, so nothing has to be kept in step with it.
    pub fn goal_tasks(&self, id: &str) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self
            .tasks
            .values()
            .map(|rec| &rec.task)
            .filter(|task| task.spec.goal.as_deref() == Some(id))
            .collect();
        tasks.sort_by_key(|task| task.created_at);
        tasks
    }

    pub(crate) fn new_session_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.sessions.contains_key(id))
            .unwrap_or_default()
    }

    pub fn create_session(
        &mut self,
        repository: String,
        base_branch: String,
        title: String,
        created_by: Option<String>,
    ) -> Session {
        let session = Session {
            id: self.new_session_id(),
            repository,
            base_branch,
            title,
            created_at: now_ms(),
            workspace: self.workspace.clone(),
            created_by,
            archived: false,
        };
        tracing::info!(session = %session.id, "session created");
        self.sessions.insert(session.id.clone(), session.clone());
        session
    }

    /// Renames a thread, archives it, or both. `None` when there is no such
    /// session. An empty or whitespace title is rejected by the caller, not
    /// here.
    pub fn update_session(
        &mut self,
        id: &str,
        title: Option<String>,
        archived: Option<bool>,
    ) -> Option<Session> {
        let session = self.sessions.get_mut(id)?;
        if let Some(title) = title {
            session.title = title;
        }
        if let Some(archived) = archived {
            session.archived = archived;
        }
        Some(session.clone())
    }

    /// Forgets a thread. The tasks it produced are left alone: deleting a
    /// thread must not delete work that has already run.
    pub fn remove_session(&mut self, id: &str) -> Option<Session> {
        self.sessions.remove(id)
    }

    /// The session's tasks, oldest first. `spec.session` is the only record
    /// of membership, so nothing has to be kept in step with it.
    pub fn session_tasks(&self, id: &str) -> Vec<&Task> {
        let mut tasks: Vec<&Task> = self
            .tasks
            .values()
            .map(|rec| &rec.task)
            .filter(|task| task.spec.session.as_deref() == Some(id))
            .collect();
        tasks.sort_by_key(|task| task.created_at);
        tasks
    }

    /// Only the first message names a session, so a title is set once.
    /// Returns the updated session to persist, or `None` if it already had one.
    pub fn fill_session_title(&mut self, id: &str, text: &str) -> Option<Session> {
        let session = self.sessions.get_mut(id)?;
        if !session.title.is_empty() {
            return None;
        }
        session.title = first_line_title(text);
        Some(session.clone())
    }

    fn new_todo_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.todos.contains_key(id))
            .unwrap_or_default()
    }

    pub fn create_todo(
        &mut self,
        repository: Option<String>,
        title: String,
        description: String,
        created_by: Option<String>,
    ) -> Todo {
        let todo = Todo {
            id: self.new_todo_id(),
            number: self.take_number(repository.as_deref()),
            repository,
            title,
            description,
            status: TodoStatus::Open,
            created_at: now_ms(),
            task: None,
            priority: lgtm_protocol::Priority::default(),
            assignee: None,
            blockers: Vec::new(),
            tags: Vec::new(),
            workspace: self.workspace.clone(),
            created_by,
        };
        tracing::info!(todo = %todo.id, "todo added");
        self.todos.insert(todo.id.clone(), todo.clone());
        todo
    }

    /// Forgets a todo and its thread. `None` when there is no such todo;
    /// otherwise the ids of the comments that went with it, for the caller to
    /// forget on disk too.
    pub fn remove_todo(&mut self, id: &str) -> Option<Vec<String>> {
        self.todos.remove(id)?;
        let comments: Vec<String> = self
            .todo_comments
            .values()
            .filter(|comment| comment.todo == id)
            .map(|comment| comment.id.clone())
            .collect();
        for comment in &comments {
            self.todo_comments.remove(comment);
        }
        Some(comments)
    }

    fn new_todo_comment_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.todo_comments.contains_key(id))
            .unwrap_or_default()
    }

    /// `None` when there is no such todo: a comment on nothing is a 404, not
    /// an orphan.
    pub fn create_todo_comment(
        &mut self,
        todo: &str,
        body: String,
        author: Option<String>,
    ) -> Option<TodoComment> {
        if !self.todos.contains_key(todo) {
            return None;
        }
        let comment = TodoComment {
            id: self.new_todo_comment_id(),
            todo: todo.to_string(),
            author,
            body,
            created_at: now_ms(),
        };
        self.todo_comments
            .insert(comment.id.clone(), comment.clone());
        Some(comment)
    }

    /// A todo's thread, oldest first: a thread reads downward.
    pub fn todo_comments(&self, todo: &str) -> Vec<TodoComment> {
        let mut out: Vec<TodoComment> = self
            .todo_comments
            .values()
            .filter(|comment| comment.todo == todo)
            .cloned()
            .collect();
        out.sort_by_key(|comment| comment.created_at);
        out
    }

    fn new_scratchpad_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.scratchpads.contains_key(id))
            .unwrap_or_default()
    }

    pub fn create_scratchpad(
        &mut self,
        repository: Option<String>,
        content: String,
        tags: Vec<String>,
        created_by: Option<String>,
    ) -> Scratchpad {
        let now = now_ms();
        let scratchpad = Scratchpad {
            id: self.new_scratchpad_id(),
            repository,
            content,
            created_at: now,
            updated_at: now,
            archived: false,
            tags,
            workspace: self.workspace.clone(),
            created_by,
        };
        tracing::info!(scratchpad = %scratchpad.id, "scratchpad created");
        self.scratchpads
            .insert(scratchpad.id.clone(), scratchpad.clone());
        scratchpad
    }

    /// Rewrites a document, archives it, or both. `None` when there is no such
    /// scratchpad. `updated_at` tracks the content alone, so archiving does
    /// not make a document look freshly written.
    pub fn update_scratchpad(
        &mut self,
        id: &str,
        content: Option<String>,
        archived: Option<bool>,
        tags: Option<Vec<String>>,
    ) -> Option<Scratchpad> {
        let scratchpad = self.scratchpads.get_mut(id)?;
        if let Some(content) = content.filter(|content| *content != scratchpad.content) {
            scratchpad.content = content;
            scratchpad.updated_at = now_ms();
        }
        if let Some(archived) = archived {
            scratchpad.archived = archived;
        }
        if let Some(tags) = tags {
            scratchpad.tags = tags;
        }
        Some(scratchpad.clone())
    }

    pub fn remove_scratchpad(&mut self, id: &str) -> bool {
        self.scratchpads.remove(id).is_some()
    }

    /// Marks a todo done, whatever its current status.
    pub fn finish_todo(&mut self, id: &str) -> Option<Todo> {
        let todo = self.todos.get_mut(id)?;
        todo.status = TodoStatus::Done;
        Some(todo.clone())
    }

    /// `running` names the goals whose orchestration loop is mid-step; those
    /// read as `Planning` because the next task may not exist yet.
    pub fn goal_summary(&self, id: &str, running: &HashSet<String>) -> Option<GoalSummary> {
        let goal = self.goals.get(id)?;
        let tasks = self.goal_tasks(id);
        let status = match running.contains(id) && goal.attention.is_none() {
            true => lgtm_protocol::GoalStatus::Planning,
            false => goal_status(goal, &tasks),
        };
        Some(GoalSummary {
            status,
            tasks: crate::backlog::summary(&tasks, self),
            goal: goal.clone(),
        })
    }

    /// Whether a task could ever run, so one that cannot is refused instead of
    /// queued forever. `Err` holds the 409 message.
    pub fn check_eligible(&self, spec: &TaskSpec) -> Result<(), String> {
        for id in &spec.depends_on {
            if !self.tasks.contains_key(id) {
                return Err(format!("unknown dependency {id}"));
            }
        }
        if let Some(id) = spec
            .goal
            .as_ref()
            .filter(|id| !self.goals.contains_key(*id))
        {
            return Err(format!("unknown goal {id}"));
        }
        if let Some(id) = spec
            .session
            .as_ref()
            .filter(|id| !self.sessions.contains_key(*id))
        {
            return Err(format!("unknown session {id}"));
        }
        if let Some(name) = &spec.runner {
            let runner = self
                .runners
                .get(name)
                .filter(|runner| runner.is_connected())
                .ok_or_else(|| format!("runner {name} is not connected"))?;
            if !runner.info.executors.contains(&spec.executor) {
                let executor = spec.executor.binary();
                return Err(format!("runner {name} does not have {executor}"));
            }
            if let Some(missing) = spec
                .requirements
                .iter()
                .find(|r| !runner.info.capabilities.contains(r))
            {
                return Err(format!("runner {name} lacks {missing}"));
            }
            return Ok(());
        }
        let any = self.queue_without_runners
            || self.runners.values().any(|runner| {
                runner.is_connected()
                    && runner.info.executors.contains(&spec.executor)
                    && runner.info.has_all(&spec.requirements)
            });
        any.then_some(()).ok_or_else(|| "no eligible runner".into())
    }

    /// Connected runner with the most free slots that can run `spec`, ties
    /// broken by the lowest name.
    fn candidate(&self, spec: &TaskSpec) -> Option<String> {
        self.candidate_excluding(spec, None)
    }

    /// `candidate`, but never `exclude` — the runner a lost or failed task is
    /// being moved off of.
    fn candidate_excluding(&self, spec: &TaskSpec, exclude: Option<&str>) -> Option<String> {
        self.runners
            .values()
            .filter(|runner| {
                runner.can_run(spec)
                    && spec.pins(&runner.info.name)
                    && Some(runner.info.name.as_str()) != exclude
            })
            .max_by(|a, b| {
                a.free_slots()
                    .cmp(&b.free_slots())
                    .then_with(|| self.free_slot_tie_break(spec, a, b))
            })
            .map(|runner| runner.info.name.clone())
    }

    /// Breaks a free-slot tie between `a` and `b`: under `Prefer::Fastest`,
    /// the lower median duration for `spec`'s repository wins, a runner with
    /// no history sorting last; both fall back to the lowest name.
    fn free_slot_tie_break(&self, spec: &TaskSpec, a: &RunnerConn, b: &RunnerConn) -> Ordering {
        if self.prefer == crate::Prefer::Fastest {
            let a_ms = self.median_for(&a.info.name, &spec.repository);
            let b_ms = self.median_for(&b.info.name, &spec.repository);
            match (a_ms, b_ms) {
                (Some(a_ms), Some(b_ms)) => return b_ms.cmp(&a_ms),
                (Some(_), None) => return Ordering::Greater,
                (None, Some(_)) => return Ordering::Less,
                (None, None) => {}
            }
        }
        b.info.name.cmp(&a.info.name)
    }

    /// Median duration, in ms, of `runner`'s finished executions for tasks in
    /// `repository` created in the last 7 days. `None` with no such
    /// execution, so an unknown runner can sort last rather than at zero.
    pub fn median_for(&self, runner: &str, repository: &str) -> Option<u64> {
        let since = now_ms().saturating_sub(7 * 24 * 60 * 60 * 1000);
        let mut ms: Vec<u64> = self
            .tasks
            .values()
            .map(|rec| &rec.task)
            .filter(|task| task.spec.repository == repository && task.created_at >= since)
            .flat_map(|task| task.executions.iter())
            .filter(|e| e.runner == runner)
            .filter_map(|e| e.finished_at.map(|f| f.saturating_sub(e.started_at)))
            .collect();
        (!ms.is_empty()).then(|| crate::stats::median(&mut ms))
    }

    /// Sum of `result.cost_usd` over `repository`'s tasks created in the
    /// last 24h.
    pub fn spent_last_day(&self, repository: &str) -> f64 {
        let since = now_ms().saturating_sub(24 * 60 * 60 * 1000);
        self.tasks
            .values()
            .map(|rec| &rec.task)
            .filter(|task| task.spec.repository == repository && task.created_at >= since)
            .filter_map(|task| task.result.as_ref())
            .map(|result| result.cost_usd)
            .sum()
    }

    /// The `[policy] budget_daily_usd` the repository's most recently
    /// completed task declared, if any. The setting lives in the
    /// repository's own `.lgtm/config.toml`, not the orchestrator, so the
    /// last run's report of it is the only copy the orchestrator ever sees.
    fn declared_daily_budget(&self, repository: &str) -> Option<f64> {
        self.tasks
            .values()
            .map(|rec| &rec.task)
            .filter(|task| task.spec.repository == repository && task.result.is_some())
            .max_by_key(|task| task.created_at)
            .and_then(|task| task.result.as_ref())
            .and_then(|result| result.policy.as_ref())
            .and_then(|policy| policy.budget_daily_usd)
    }

    /// `None` when `id` is clear to schedule. `Some` when its repository has
    /// spent past the daily budget its last completed task declared; the
    /// ids inside are the ones to persist, non-empty only the first time,
    /// the same way `github::record_ci` logs a policy decision only when it
    /// changed rather than on every poll.
    fn over_daily_budget(&mut self, id: &TaskId) -> Option<Vec<TaskId>> {
        let repository = self.tasks.get(id)?.task.spec.repository.clone();
        let budget = self.declared_daily_budget(&repository)?;
        let spent = self.spent_last_day(&repository);
        if spent <= budget {
            return None;
        }
        let event = TaskEvent::PolicyDecision {
            action: "schedule".into(),
            allowed: false,
            reasons: vec![format!("daily budget ${spent:.2} over ${budget:.2}")],
        };
        let repeat = Some(&event) == self.tasks[id].last_policy_decision("schedule");
        Some(if repeat {
            Vec::new()
        } else {
            self.apply_event(id, event)
        })
    }

    /// Queued, unassigned, and not waiting on any dependency.
    pub fn is_ready(&self, task: &Task) -> bool {
        task.status == TaskStatus::Queued && task.runner.is_none() && self.deps_met(&task.spec)
    }

    /// Assigns unassigned queued tasks, oldest first, and starts them.
    /// Returns the ids to persist: every task it assigned, plus any it
    /// recorded a daily-budget refusal on.
    pub fn schedule(&mut self) -> Vec<TaskId> {
        let mut queued: Vec<(u64, TaskId)> = self
            .tasks
            .values()
            .filter(|rec| self.is_ready(&rec.task))
            .map(|rec| (rec.task.created_at, rec.task.id.clone()))
            .collect();
        queued.sort();
        let mut changed = Vec::new();
        for (_, id) in queued {
            if let Some(refused) = self.over_daily_budget(&id) {
                changed.extend(refused);
                continue;
            }
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
            rec.task.runner = Some(name.clone());
            let task = rec.task.clone();
            let memories = self.memories_for(&task.spec.repository);
            let authorship = self.authorship(&task);
            if let Some(runner) = self.runners.get_mut(&name) {
                runner.running.insert(id.clone());
                runner.send(OrchestratorMessage::Start {
                    task: Box::new(task),
                    memories,
                    authorship,
                });
            }
            tracing::info!(task = %id, runner = %name, "task assigned");
            changed.push(id);
        }
        changed
    }

    /// Queues a task and schedules it. Returns the task and the ids to persist.
    pub fn create_task(&mut self, mut spec: TaskSpec) -> Result<(Task, Vec<TaskId>), String> {
        self.check_eligible(&spec)?;
        if spec.model.is_none() {
            spec.model = self.models.get(kind_key(spec.kind)).cloned();
        }
        // Work arriving is the answer the loop was waiting for.
        if let Some(goal) = spec.goal.clone() {
            self.set_attention(&goal, None);
        }
        let created_by = spec.created_by.clone();
        let task = Task {
            id: self.new_id(),
            title: None,
            spec,
            status: TaskStatus::Queued,
            runner: None,
            created_at: now_ms(),
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            pr_review: None,
            executions: Vec::new(),
            scratchpad: String::new(),
            files: Vec::new(),
            workspace: self.workspace.clone(),
            created_by,
        };
        let id = task.id.clone();
        tracing::info!(task = %id, "task created");
        self.tasks
            .insert(id.clone(), TaskRecord::new(task, Vec::new()));
        let mut changed = vec![id.clone()];
        changed.extend(self.schedule());
        Ok((self.tasks[&id].task.clone(), changed))
    }

    /// Stores the model-written title; a task that finished or vanished while
    /// the model thought is simply left as it was.
    pub fn set_task_title(&mut self, id: &TaskId, title: String) -> Vec<TaskId> {
        let Some(rec) = self.tasks.get_mut(id) else {
            return Vec::new();
        };
        rec.task.title = Some(title);
        vec![id.clone()]
    }

    pub(crate) fn lose_unfinished(&mut self, id: &TaskId) -> Vec<TaskId> {
        match self.tasks.get(id) {
            Some(rec) if !rec.task.status.is_terminal() => {
                self.apply_event(id, TaskEvent::RunnerLost)
            }
            _ => Vec::new(),
        }
    }

    /// Records a runner event, applies its status transition, and reschedules
    /// if it freed a slot. Returns the ids to persist.
    ///
    /// An artefact's bytes are taken off the event first, so what is stored,
    /// broadcast and served back is the name and the size.
    pub fn apply_event(&mut self, task_id: &str, event: TaskEvent) -> Vec<TaskId> {
        let Some(rec) = self.tasks.get_mut(task_id) else {
            tracing::warn!(task = %task_id, "event for unknown task, ignoring");
            return Vec::new();
        };
        let stored = StoredEvent {
            at: now_ms(),
            event: split_artefact(&mut rec.artefacts, event),
        };
        rec.events.push(stored.clone());
        let terminal = rec.task.status.is_terminal();
        crate::execution::record(&mut rec.task, &stored.event, stored.at);
        let finished = transition(&mut rec.task, &stored.event);
        let status = rec.task.status;
        let runner = rec.task.runner.clone();
        let _ = rec.live.send(stored);
        tracing::debug!(task = %task_id, ?status, "task event applied");
        let freed = finished
            && runner
                .and_then(|name| self.runners.get_mut(&name))
                .is_some_and(|runner| runner.running.remove(task_id));
        let mut changed = vec![task_id.to_string()];
        if freed {
            changed.extend(self.schedule());
        }
        // Only the transition itself, never a late event repeating it.
        if !terminal {
            match status {
                TaskStatus::Failed | TaskStatus::TimedOut | TaskStatus::RunnerLost => {
                    changed.extend(self.fail_dependents(task_id));
                    changed.extend(self.maybe_reassign(task_id, status));
                }
                TaskStatus::Cancelled | TaskStatus::Rejected => {
                    changed.extend(self.fail_dependents(task_id));
                }
                // Approved unblocks an Approved/Merged dependency; AwaitingReview
                // unblocks a Completed one.
                TaskStatus::Approved | TaskStatus::AwaitingReview => {
                    changed.extend(self.schedule())
                }
                _ => {}
            }
        }
        changed
    }

    /// Puts a task that just ended badly back in the queue, off the runner
    /// it was on, when the repository's `reassign` policy allows one more
    /// try. A lost runner is not the task's fault, so it always gets one
    /// attempt even with no policy on record; a failed or timed-out run only
    /// gets one when a completed run declared `reassign > 0`.
    fn maybe_reassign(&mut self, task_id: &str, status: TaskStatus) -> Vec<TaskId> {
        let Some(rec) = self.tasks.get(task_id) else {
            return Vec::new();
        };
        let declared = rec
            .task
            .result
            .as_ref()
            .and_then(|result| result.policy.as_ref())
            .map(|policy| policy.reassign);
        let reassign = match status {
            TaskStatus::RunnerLost => declared.unwrap_or(1),
            TaskStatus::Failed | TaskStatus::TimedOut => match declared {
                Some(n) if n > 0 => n,
                _ => return Vec::new(),
            },
            _ => return Vec::new(),
        };
        let requeued = rec
            .events
            .iter()
            .filter(|stored| matches!(stored.event, TaskEvent::Requeued { .. }))
            .count() as u32;
        if requeued >= reassign {
            return Vec::new();
        }
        let current = rec.task.runner.clone();
        let other = self.candidate_excluding(&rec.task.spec, current.as_deref());
        tracing::info!(task = %task_id, runner = ?other, ?status, "reassigning after a bad run");
        let into = RetryInto {
            runner: other,
            executor: None,
        };
        self.retry(task_id, into)
            .map_or(Vec::new(), |(_, changed)| changed)
    }
}

/// Takes an artefact's payload off the event and queues the bytes for the
/// next `persist_ids`. A payload that is not base64 is dropped: the event
/// still records that the run produced the file.
fn split_artefact(queued: &mut Vec<(String, Vec<u8>)>, mut event: TaskEvent) -> TaskEvent {
    let TaskEvent::Artefact {
        name, bytes_base64, ..
    } = &mut event
    else {
        return event;
    };
    let payload = std::mem::take(bytes_base64);
    if payload.is_empty() {
        return event;
    }
    match lgtm_protocol::decode_base64(&payload) {
        Some(bytes) => queued.push((name.clone(), bytes)),
        None => tracing::error!(name, "artefact payload is not base64"),
    }
    event
}

/// Moves the task's status for `event` and says whether the run ended. A
/// runner that reconnects may keep reporting on a task we already failed;
/// the event is kept but a terminal status is left alone.
fn transition(task: &mut Task, event: &TaskEvent) -> bool {
    let terminal = task.status.is_terminal();
    let (status, finished) = match event {
        // A new attempt starts from nothing, so the live file list a reader
        // sees is this run's, never the one before it left behind.
        TaskEvent::Started { .. } => {
            task.files.clear();
            (Some(TaskStatus::Running), false)
        }
        TaskEvent::Completed { result } => {
            if !terminal {
                task.result = Some(result.clone());
            }
            (Some(TaskStatus::AwaitingReview), true)
        }
        TaskEvent::Failed { error } => {
            if !terminal {
                task.error = Some(error.clone());
            }
            (Some(TaskStatus::Failed), true)
        }
        // Notes are not status: a task that already ended keeps the last ones.
        TaskEvent::Scratchpad { content } => {
            task.scratchpad = content.clone();
            (None, false)
        }
        // What the run has touched so far, so an overlap with another running
        // task shows before either of them completes.
        TaskEvent::FileChanged { path } => {
            if !task.files.contains(path) {
                task.files.push(path.clone());
            }
            (None, false)
        }
        TaskEvent::TimedOut { .. } => (Some(TaskStatus::TimedOut), true),
        TaskEvent::RunnerLost => (Some(TaskStatus::RunnerLost), true),
        TaskEvent::Cancelled => (Some(TaskStatus::Cancelled), true),
        TaskEvent::Pushed { .. } => (Some(TaskStatus::Approved), false),
        // A push runs outside an agent run, so no slot was taken to free.
        TaskEvent::Conflicted { .. } => (Some(TaskStatus::Conflicted), false),
        TaskEvent::Discarded => (Some(TaskStatus::Rejected), false),
        TaskEvent::Message { .. } => (Some(TaskStatus::ChangesRequested), false),
        // `retry` sets the status itself: this is the one move out of a
        // terminal status, which the rule below would otherwise refuse.
        TaskEvent::Requeued { .. } => (None, false),
        // Retry and the policy notes are for the reader, not the status; a
        // run in progress stays exactly where it was.
        TaskEvent::Output { .. }
        | TaskEvent::Command { .. }
        | TaskEvent::Progress { .. }
        | TaskEvent::Validating { .. }
        | TaskEvent::NetworkDenied { .. }
        | TaskEvent::PermissionRequested { .. }
        | TaskEvent::HostAllowed { .. }
        | TaskEvent::Retry { .. }
        | TaskEvent::Artefact { .. }
        | TaskEvent::PolicyDecision { .. }
        | TaskEvent::Orchestrated { .. }
        | TaskEvent::AutoApproved
        | TaskEvent::AutoMerged
        | TaskEvent::PrReviewed { .. } => (None, false),
    };
    if let (Some(status), false) = (status, terminal) {
        task.status = status;
    }
    finished
}

#[cfg(test)]
#[path = "state_tests.rs"]
pub(crate) mod tests;
