//! One JSON file per task under `<data_dir>/tasks`.

use std::path::Path;

use lgtm_protocol::{StoredEvent, Task};
use serde::{Deserialize, Serialize};

use crate::state::TaskRecord;

/// On-disk shape of a task, and the body of `GET /api/tasks/:id`.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stored {
    pub task: Task,
    pub events: Vec<StoredEvent>,
}

pub fn save(dir: &Path, rec: &TaskRecord) {
    let stored = Stored {
        task: rec.task.clone(),
        events: rec.events.clone(),
    };
    let final_path = dir.join(format!("{}.json", rec.task.id));
    let tmp_path = dir.join(format!("{}.json.tmp", rec.task.id));
    let write = serde_json::to_vec_pretty(&stored)
        .map_err(std::io::Error::other)
        .and_then(|bytes| std::fs::write(&tmp_path, bytes))
        .and_then(|()| std::fs::rename(&tmp_path, &final_path));
    if let Err(err) = write {
        tracing::error!(task = %rec.task.id, %err, "failed to persist task");
    }
}

pub fn load_all(dir: &Path) -> Vec<Stored> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::error!(%err, "failed to read tasks directory");
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
            .and_then(|bytes| {
                serde_json::from_slice::<Stored>(&bytes).map_err(std::io::Error::other)
            }) {
            Ok(stored) => out.push(stored),
            Err(err) => tracing::error!(path = %path.display(), %err, "failed to load task"),
        }
    }
    out
}
