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

/// Ids become file names, so a tampered file on disk must not be able to
/// send the next save anywhere but the tasks directory.
fn is_safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub fn save(dir: &Path, rec: &TaskRecord) {
    if !is_safe_id(&rec.task.id) {
        tracing::error!(task = %rec.task.id, "refusing to persist task with unsafe id");
        return;
    }
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
            Ok(stored) if !is_safe_id(&stored.task.id) => {
                tracing::error!(task = %stored.task.id, "refusing to load task with unsafe id");
            }
            Ok(stored) => out.push(stored),
            Err(err) => tracing::error!(path = %path.display(), %err, "failed to load task"),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::is_safe_id;

    #[test]
    fn safe_ids_are_file_names() {
        assert!(is_safe_id("0123abcd"));
        assert!(!is_safe_id(""));
        assert!(!is_safe_id("../x"));
        assert!(!is_safe_id("a/b"));
        assert!(!is_safe_id(&"a".repeat(33)));
    }
}
