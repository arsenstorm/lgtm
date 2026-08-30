//! Files a run leaves for whoever reviews it: screenshots, generated output,
//! anything the diff cannot show.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use lgtm_protocol::artefact_name;

/// Where an agent puts them, relative to the worktree.
pub const DIR: &str = ".lgtm/artefacts";

/// A reviewer looks at a handful of files; the rest is somebody dumping a
/// build directory, and every byte travels over the runner socket.
const MAX_FILES: usize = 20;
const MAX_TOTAL: u64 = 5 * 1024 * 1024;

/// Regular files directly under `<worktree>/.lgtm/artefacts`, with their
/// contents. A file that does not fit the caps is left behind with a warning.
pub async fn collect(worktree: &Path) -> Vec<(String, Vec<u8>)> {
    let dir = worktree.join(DIR);
    let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
        return Vec::new();
    };
    let mut found = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let Ok(meta) = entry.metadata().await else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if let (true, Some(name)) = (meta.is_file(), artefact_name(&name)) {
            found.push((name, entry.path(), meta.len()));
        }
    }
    let (kept, skipped) = pick(found.iter().map(|(name, _, len)| (name.clone(), *len)));
    if !skipped.is_empty() {
        tracing::warn!(files = %skipped.join(", "), "artefacts too large to send, skipping");
    }
    let mut out = Vec::new();
    for (name, path, _) in found {
        if !kept.contains(&name) {
            continue;
        }
        match tokio::fs::read(&path).await {
            Ok(bytes) => out.push((name, bytes)),
            Err(err) => tracing::warn!(%err, path = %path.display(), "artefact unreadable"),
        }
    }
    out
}

/// Smallest first, so one huge file cannot crowd out everything else.
/// Returns the names to send and the names that did not fit.
fn pick(files: impl Iterator<Item = (String, u64)>) -> (Vec<String>, Vec<String>) {
    let mut files: Vec<(String, u64)> = files.collect();
    files.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut total = 0;
    let (mut kept, mut skipped) = (Vec::new(), Vec::new());
    for (name, size) in files {
        if kept.len() < MAX_FILES && total + size <= MAX_TOTAL {
            total += size;
            kept.push(name);
        } else {
            skipped.push(name);
        }
    }
    (kept, skipped)
}

/// Whether this file is worth another event: a run that keeps a screenshot
/// around unchanged should not resend it after every follow-up.
pub fn changed(seen: &mut Vec<(String, u64)>, name: &str, bytes: &[u8]) -> bool {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    let hash = hasher.finish();
    match seen.iter_mut().find(|(known, _)| known == name) {
        Some(entry) if entry.1 == hash => false,
        Some(entry) => {
            entry.1 = hash;
            true
        }
        None => {
            seen.push((name.to_string(), hash));
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_budget_takes_the_small_files_first() {
        let files = vec![
            ("big.png".to_string(), MAX_TOTAL),
            ("small.png".to_string(), 10),
            ("rest.png".to_string(), MAX_TOTAL),
        ];
        let (kept, skipped) = pick(files.into_iter());
        assert_eq!(kept, vec!["small.png"]);
        assert_eq!(skipped, vec!["big.png", "rest.png"]);
    }

    #[test]
    fn the_budget_counts_files_too() {
        let files = (0..MAX_FILES + 2).map(|n| (format!("{n:02}.png"), 1));
        let (kept, skipped) = pick(files);
        assert_eq!(kept.len(), MAX_FILES);
        assert_eq!(skipped, vec!["20.png", "21.png"]);
    }

    #[test]
    fn only_a_changed_file_is_sent_again() {
        let mut seen = Vec::new();
        assert!(changed(&mut seen, "a.png", b"one"));
        assert!(!changed(&mut seen, "a.png", b"one"));
        assert!(changed(&mut seen, "a.png", b"two"));
        assert!(changed(&mut seen, "b.png", b"one"));
        assert_eq!(seen.len(), 2);
    }
}
