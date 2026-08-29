//! One JSON file per task under `<data_dir>/tasks`, one per batch under
//! `<data_dir>/batches`, one per memory under `<data_dir>/memories`, and one
//! per goal under `<data_dir>/goals`.

use std::path::{Path, PathBuf};

use lgtm_protocol::{Batch, Goal, Memory, StoredEvent, Task};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::state::TaskRecord;

/// On-disk shape of a task, and the body of `GET /api/tasks/:id`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stored {
    pub task: Task,
    pub events: Vec<StoredEvent>,
}

/// One thing to write. The writer owns every directory it writes into.
pub enum Persist {
    Task(Box<Stored>),
    Batch(Batch),
    Memory(Memory),
    RemoveMemory(String),
    Goal(Goal),
}

impl From<&TaskRecord> for Stored {
    fn from(rec: &TaskRecord) -> Self {
        Self {
            task: rec.task.clone(),
            events: rec.events.clone(),
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
    while let Some(item) = rx.recv().await {
        match item {
            Persist::Task(stored) => save(&tasks, &stored),
            Persist::Batch(batch) => save_batch(&batches, &batch),
            Persist::Memory(memory) => save_memory(&memories, &memory),
            Persist::RemoveMemory(id) => remove_memory(&memories, &id),
            Persist::Goal(goal) => save_goal(&goals, &goal),
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

pub fn save(dir: &Path, stored: &Stored) {
    save_by_id(dir, "task", &stored.task.id, stored);
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

pub fn remove_memory(dir: &Path, id: &str) {
    let Some(stem) = file_stem(id) else {
        tracing::error!(memory = %id, "refusing to remove memory with unsafe id");
        return;
    };
    match std::fs::remove_file(dir.join(format!("{stem}.json"))) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => tracing::error!(memory = %id, %err, "failed to remove memory"),
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

pub fn load_all(dir: &Path) -> Vec<Stored> {
    load_dir(dir, |stored: &Stored| stored.task.id.as_str())
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

#[cfg(test)]
mod tests {
    use super::file_stem;

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
}
