//! One JSON file per task under `<data_dir>/tasks`, one per batch under
//! `<data_dir>/batches`, one per memory under `<data_dir>/memories`, one per
//! goal under `<data_dir>/goals`, one per todo under `<data_dir>/todos`, and
//! one per session under `<data_dir>/sessions`.
//!
//! A task's events are append-only: rewriting `<id>.json` on every event
//! meant copying the whole history back to disk each time, so events live in
//! a sibling `<id>.events.jsonl` instead and the task file holds only the
//! `Task`.

use std::io::Write;
use std::path::{Path, PathBuf};

use lgtm_protocol::{Batch, Goal, Memory, Overlap, Session, StoredEvent, Task, TaskId, Todo};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::state::TaskRecord;

/// On-disk shape of a task before the event log split, and the body of
/// `GET /api/tasks/:id`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stored {
    pub task: Task,
    pub events: Vec<StoredEvent>,
    /// What other live tasks touched too. It is about the other tasks and
    /// only true right now, so the handler fills it and nothing writes it.
    #[serde(default)]
    pub overlaps: Vec<Overlap>,
}

/// One thing to write. The writer owns every directory it writes into.
pub enum Persist {
    Task(Box<Task>),
    Event {
        task_id: TaskId,
        event: StoredEvent,
    },
    Batch(Batch),
    Memory(Memory),
    RemoveMemory(String),
    Goal(Goal),
    Todo(Todo),
    RemoveTodo(String),
    Session(Session),
    /// The whole users store; users are few and change rarely, so the file
    /// is rewritten rather than kept per-id.
    Users(Vec<crate::users::UserRecord>),
    /// An artefact's bytes, split off the event that carried them: the event
    /// log is text a person reads, and these are binaries.
    Artefact {
        task_id: TaskId,
        name: String,
        bytes: Vec<u8>,
    },
    /// Reads an artefact back. The writer owns the data directory, so a
    /// request handler asks it rather than holding a path of its own.
    ReadArtefact {
        task_id: TaskId,
        name: String,
        reply: oneshot::Sender<Option<Vec<u8>>>,
    },
}

impl From<&TaskRecord> for Stored {
    fn from(rec: &TaskRecord) -> Self {
        Self {
            task: rec.task.clone(),
            events: rec.events.clone(),
            overlaps: Vec::new(),
        }
    }
}

/// Owns the data directory so no request handler ever holds it, and keeps
/// writes for a task in the order its events arrived. `dir` is the data
/// directory; `tasks`, `batches`, `memories` and `goals` under it must
/// already exist.
pub async fn writer(dir: PathBuf, mut rx: mpsc::UnboundedReceiver<Persist>) {
    let tasks = dir.join("tasks");
    let batches = dir.join("batches");
    let memories = dir.join("memories");
    let goals = dir.join("goals");
    let todos = dir.join("todos");
    let sessions = dir.join("sessions");
    let artefacts = dir.join("artefacts");
    while let Some(item) = rx.recv().await {
        match item {
            Persist::Task(task) => save(&tasks, &task),
            Persist::Event { task_id, event } => append_event(&tasks, &task_id, &event),
            Persist::Artefact {
                task_id,
                name,
                bytes,
            } => write_artefact(&artefacts, &task_id, &name, &bytes),
            Persist::ReadArtefact {
                task_id,
                name,
                reply,
            } => {
                let _ = reply.send(read_artefact(&artefacts, &task_id, &name));
            }
            Persist::Batch(batch) => save_batch(&batches, &batch),
            Persist::Memory(memory) => save_memory(&memories, &memory),
            Persist::RemoveMemory(id) => remove_by_id(&memories, "memory", &id),
            Persist::Goal(goal) => save_goal(&goals, &goal),
            Persist::Todo(todo) => save_todo(&todos, &todo),
            Persist::RemoveTodo(id) => remove_by_id(&todos, "todo", &id),
            Persist::Session(session) => save_session(&sessions, &session),
            Persist::Users(users) => save_users(&dir, &users),
        }
    }
}

/// Ids are eight hex chars (see `State::new_id`). Round-tripping through an
/// integer means the file name is derived from a number, never from the raw
/// string, so a tampered id can only ever produce another plain hex name.
fn file_stem(id: &str) -> Option<String> {
    if id.len() != 8 {
        return None;
    }
    u32::from_str_radix(id, 16).ok().map(|n| format!("{n:08x}"))
}

/// Writes `value` to `<dir>/<stem>.json`, through a temporary file so a reader
/// never sees a half-written record.
fn write_json<T: Serialize>(dir: &Path, stem: &str, value: &T) -> std::io::Result<()> {
    let final_path = dir.join(format!("{stem}.json"));
    let tmp_path = dir.join(format!("{stem}.json.tmp"));
    serde_json::to_vec_pretty(value)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&tmp_path, bytes))
        .and_then(|()| std::fs::rename(&tmp_path, &final_path))
}

/// `kind` names the record in the log; everything else is the same however
/// the record is shaped.
fn save_by_id<T: Serialize>(dir: &Path, kind: &str, id: &str, value: &T) {
    let Some(stem) = file_stem(id) else {
        tracing::error!(kind, id, "refusing to persist record with unsafe id");
        return;
    };
    if let Err(err) = write_json(dir, &stem, value) {
        tracing::error!(kind, id, %err, "failed to persist record");
    }
}

pub fn save(dir: &Path, task: &Task) {
    save_by_id(dir, "task", &task.id, task);
}

/// Appends one JSON line to `<id>.events.jsonl`; a task's events are never
/// rewritten, only ever added to.
fn append_event(dir: &Path, task_id: &str, event: &StoredEvent) {
    let Some(stem) = file_stem(task_id) else {
        tracing::error!(
            kind = "event",
            id = task_id,
            "refusing to persist record with unsafe id"
        );
        return;
    };
    if let Err(err) = append_json_line(dir, &stem, event) {
        tracing::error!(kind = "event", id = task_id, %err, "failed to persist record");
    }
}

/// `<dir>/<task>/<name>`, or `None` for an id or a name that has no business
/// being part of a path.
fn artefact_path(dir: &Path, task_id: &str, name: &str) -> Option<PathBuf> {
    let stem = file_stem(task_id)?;
    let safe = lgtm_protocol::artefact_name(name)?;
    (safe == name).then(|| dir.join(stem).join(safe))
}

pub fn write_artefact(dir: &Path, task_id: &str, name: &str, bytes: &[u8]) {
    let Some(path) = artefact_path(dir, task_id, name) else {
        tracing::error!(
            kind = "artefact",
            id = task_id,
            name,
            "refusing to persist record with unsafe name"
        );
        return;
    };
    let written = std::fs::create_dir_all(path.parent().unwrap_or(dir))
        .and_then(|()| std::fs::write(&path, bytes));
    if let Err(err) = written {
        tracing::error!(kind = "artefact", id = task_id, name, %err, "failed to persist record");
    }
}

fn read_artefact(dir: &Path, task_id: &str, name: &str) -> Option<Vec<u8>> {
    std::fs::read(artefact_path(dir, task_id, name)?).ok()
}

fn append_json_line<T: Serialize>(dir: &Path, stem: &str, value: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    line.push(b'\n');
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(dir.join(format!("{stem}.events.jsonl")))
        .and_then(|mut file| file.write_all(&line))
}

pub fn save_batch(dir: &Path, batch: &Batch) {
    save_by_id(dir, "batch", &batch.id, batch);
}

pub fn save_goal(dir: &Path, goal: &Goal) {
    save_by_id(dir, "goal", &goal.id, goal);
}

pub fn save_memory(dir: &Path, memory: &Memory) {
    save_by_id(dir, "memory", &memory.id, memory);
}

pub fn save_todo(dir: &Path, todo: &Todo) {
    save_by_id(dir, "todo", &todo.id, todo);
}

pub fn save_session(dir: &Path, session: &Session) {
    save_by_id(dir, "session", &session.id, session);
}

/// `<data_dir>/users.json`, owner-readable only: it holds tokens. The
/// temporary file is created 0600 before a byte is written, so no rename or
/// crash window ever leaves the tokens world-readable.
pub fn save_users(dir: &Path, users: &[crate::users::UserRecord]) {
    let write = || -> std::io::Result<()> {
        let tmp = dir.join("users.json.tmp");
        let bytes = serde_json::to_vec_pretty(users).map_err(std::io::Error::other)?;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp)?;
        file.write_all(&bytes)?;
        std::fs::rename(&tmp, dir.join("users.json"))
    };
    if let Err(err) = write() {
        tracing::error!(kind = "users", %err, "failed to persist record");
    }
}

pub fn load_users(dir: &Path) -> Vec<crate::users::UserRecord> {
    let path = dir.join("users.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    match serde_json::from_slice(&bytes) {
        Ok(users) => users,
        Err(err) => {
            tracing::error!(kind = "users", %err, "failed to load record");
            Vec::new()
        }
    }
}

/// `kind` names the record in the log; everything else is the same however
/// the record is stored.
fn remove_by_id(dir: &Path, kind: &str, id: &str) {
    let Some(stem) = file_stem(id) else {
        tracing::error!(kind, id, "refusing to remove record with unsafe id");
        return;
    };
    match std::fs::remove_file(dir.join(format!("{stem}.json"))) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::error!(kind, id, %err, "failed to remove record"),
    }
}

/// Every `<dir>/*.json` that parses as a `T` and whose id is a safe file name.
fn load_dir<T: DeserializeOwned>(dir: &Path, id: impl Fn(&T) -> &str) -> Vec<T> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::error!(dir = %dir.display(), %err, "failed to read directory");
            return out;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        match std::fs::read(&path)
            .map_err(std::io::Error::other)
            .and_then(|bytes| serde_json::from_slice::<T>(&bytes).map_err(std::io::Error::other))
        {
            Ok(value) if file_stem(id(&value)).is_none() => {
                tracing::error!(id = %id(&value), "refusing to load record with unsafe id");
            }
            Ok(value) => out.push(value),
            Err(err) => tracing::error!(path = %path.display(), %err, "failed to load record"),
        }
    }
    out
}

/// Every `<dir>/<id>.json`, task and events reassembled into a `Stored`. A
/// file in the old layout (task and events together) is migrated to the new
/// one as it is read, so the slow path is taken at most once per task.
pub fn load_all(dir: &Path) -> Vec<Stored> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::error!(dir = %dir.display(), %err, "failed to read directory");
            return out;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        out.extend(load_task_file(dir, &path));
    }
    out
}

fn load_task_file(dir: &Path, path: &Path) -> Option<Stored> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(path = %path.display(), %err, "failed to load record");
            return None;
        }
    };
    if let Ok(stored) = serde_json::from_slice::<Stored>(&bytes) {
        return migrate(dir, stored);
    }
    let task = match serde_json::from_slice::<Task>(&bytes) {
        Ok(task) => task,
        Err(err) => {
            tracing::error!(path = %path.display(), %err, "failed to load record");
            return None;
        }
    };
    if file_stem(&task.id).is_none() {
        tracing::error!(id = %task.id, "refusing to load record with unsafe id");
        return None;
    }
    let events = load_events(dir, &task.id);
    Some(Stored {
        task,
        events,
        overlaps: Vec::new(),
    })
}

/// Rewrites an old-layout file (task and events together) as a task file
/// plus an events file, so this task never takes the slow path again.
fn migrate(dir: &Path, stored: Stored) -> Option<Stored> {
    if file_stem(&stored.task.id).is_none() {
        tracing::error!(id = %stored.task.id, "refusing to load record with unsafe id");
        return None;
    }
    tracing::info!(task = %stored.task.id, "migrating task file to the split event log");
    save(dir, &stored.task);
    for event in &stored.events {
        append_event(dir, &stored.task.id, event);
    }
    Some(stored)
}

fn load_events(dir: &Path, task_id: &str) -> Vec<StoredEvent> {
    let Some(stem) = file_stem(task_id) else {
        return Vec::new();
    };
    let path = dir.join(format!("{stem}.events.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    text.lines()
        .filter_map(|line| parse_event(&path, line))
        .collect()
}

fn parse_event(path: &Path, line: &str) -> Option<StoredEvent> {
    match serde_json::from_str(line) {
        Ok(event) => Some(event),
        Err(err) => {
            tracing::error!(path = %path.display(), %err, "skipping unparsable event line");
            None
        }
    }
}

pub fn load_all_batches(dir: &Path) -> Vec<Batch> {
    load_dir(dir, |batch: &Batch| batch.id.as_str())
}

pub fn load_all_memories(dir: &Path) -> Vec<Memory> {
    load_dir(dir, |memory: &Memory| memory.id.as_str())
}

pub fn load_all_goals(dir: &Path) -> Vec<Goal> {
    load_dir(dir, |goal: &Goal| goal.id.as_str())
}

pub fn load_all_todos(dir: &Path) -> Vec<Todo> {
    load_dir(dir, |todo: &Todo| todo.id.as_str())
}

pub fn load_all_sessions(dir: &Path) -> Vec<Session> {
    load_dir(dir, |session: &Session| session.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskEvent, TaskKind, TaskSpec, TaskStatus};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn only_hex_ids_become_file_names() {
        assert_eq!(file_stem("0123abcd"), Some("0123abcd".into()));
        assert_eq!(file_stem("0123ABCD"), Some("0123abcd".into()));
        assert_eq!(file_stem(""), None);
        assert_eq!(file_stem("../x"), None);
        assert_eq!(file_stem("a/b"), None);
        assert_eq!(file_stem("0123ABCDE"), None);
        assert_eq!(file_stem("zzzzzzzz"), None);
    }

    /// A fresh directory per test, so tests never see one another's files.
    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lgtm-persist-test-{name}-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn task(id: &str) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: "https://example.com/repo.git".into(),
                base_branch: "main".into(),
                prompt: "do it".into(),
                executor: Executor::Claude,
                runner: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: Vec::new(),
                depends_on_condition: Default::default(),
                batch: None,
                sandbox: None,
                requirements: Vec::new(),
                review_executor: None,
                model: None,
                goal: None,
                allowed_hosts: Vec::new(),
                session: None,
                created_by: None,
            },
            status: TaskStatus::Queued,
            runner: None,
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            pr_review: None,
            executions: Vec::new(),
            scratchpad: String::new(),
            files: Vec::new(),
            workspace: None,
            created_by: None,
        }
    }

    fn event(at: u64) -> StoredEvent {
        StoredEvent {
            at,
            event: TaskEvent::Started { model: None },
        }
    }

    #[test]
    fn round_trips_the_new_layout() {
        let dir = temp_dir("round-trip");
        let task = task("0123abcd");
        save(&dir, &task);
        append_event(&dir, &task.id, &event(1));
        append_event(&dir, &task.id, &event(2));

        let loaded = load_all(&dir);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].task.id, task.id);
        assert_eq!(loaded[0].events.len(), 2);
    }

    #[test]
    fn old_layout_loads_and_migrates() {
        let dir = temp_dir("migrate");
        let stored = Stored {
            task: task("0123abcd"),
            events: vec![event(1), event(2)],
            overlaps: Vec::new(),
        };
        write_json(&dir, "0123abcd", &stored).unwrap();

        let loaded = load_all(&dir);

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].events.len(), 2);
        assert!(dir.join("0123abcd.events.jsonl").exists());
        // The migrated layout must itself be readable, not just written.
        assert_eq!(load_all(&dir)[0].events.len(), 2);
    }

    #[test]
    fn an_artefact_round_trips_through_a_file_of_its_own() {
        let dir = temp_dir("artefact");

        write_artefact(&dir, "0123abcd", "shot.png", b"Man");
        write_artefact(&dir, "0123abcd", "../shot.png", b"Man");

        assert_eq!(read_artefact(&dir, "0123abcd", "shot.png").unwrap(), b"Man");
        assert!(read_artefact(&dir, "0123abcd", "../shot.png").is_none());
        assert!(read_artefact(&dir, "0123abcd", "missing.png").is_none());
    }

    #[test]
    fn a_corrupt_event_line_is_skipped() {
        let dir = temp_dir("corrupt-event");
        save(&dir, &task("0123abcd"));
        let mut good = serde_json::to_vec(&event(1)).unwrap();
        good.push(b'\n');
        let bytes = [b"not json\n".as_slice(), &good].concat();
        std::fs::write(dir.join("0123abcd.events.jsonl"), bytes).unwrap();

        let loaded = load_all(&dir);

        assert_eq!(loaded[0].events.len(), 1);
    }
}
