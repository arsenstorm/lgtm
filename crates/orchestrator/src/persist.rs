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

/// Ids are eight hex chars (see `State::new_id`). Round-tripping through an
/// integer means the file name is derived from a number, never from the raw
/// string, so a tampered id can only ever produce another plain hex name.
fn file_stem(id: &str) -> Option<String> {
    if id.len() != 8 {
        return None;
    }
    u32::from_str_radix(id, 16).ok().map(|n| format!("{n:08x}"))
}

pub fn save(dir: &Path, rec: &TaskRecord) {
    let Some(stem) = file_stem(&rec.task.id) else {
        tracing::error!(task = %rec.task.id, "refusing to persist task with unsafe id");
        return;
    };
    let stored = Stored {
        task: rec.task.clone(),
        events: rec.events.clone(),
    };
    let final_path = dir.join(format!("{stem}.json"));
    let tmp_path = dir.join(format!("{stem}.json.tmp"));
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
            Ok(stored) if file_stem(&stored.task.id).is_none() => {
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
    use super::file_stem;

    #[test]
    fn only_hex_ids_become_file_names() {
        assert_eq!(file_stem("0123abcd"), Some("0123abcd".into()));
        // Uppercase parses, and comes back out as the lowercase name.
        assert_eq!(file_stem("0123ABCD"), Some("0123abcd".into()));
        assert_eq!(file_stem(""), None);
        assert_eq!(file_stem("../x"), None);
        assert_eq!(file_stem("a/b"), None);
        assert_eq!(file_stem("0123ABCDE"), None);
        assert_eq!(file_stem("zzzzzzzz"), None);
    }
}
