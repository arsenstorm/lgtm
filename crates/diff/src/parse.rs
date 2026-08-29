//! Turning `git diff` text into files, hunks, and typed lines.

use crate::intraline::mark_intraline;
use crate::{FileDiff, FileStatus, Hunk, Line, LineKind};

/// Parses `git diff` output containing one or more files.
///
/// Never panics: lines that make no sense in context are skipped.
pub fn parse_patch(text: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut cur: Option<Pending> = None;

    for raw in text.lines() {
        if let Some(p) = cur.as_mut() {
            if p.take_content(raw) {
                continue;
            }
        }
        if let Some(rest) = raw.strip_prefix("diff --git ") {
            if let Some(p) = cur.take() {
                files.push(p.finish());
            }
            cur = Some(Pending::from_git_header(rest));
            continue;
        }
        if cur.is_none() {
            // Tolerate a bare unified diff with no `diff --git` header.
            if raw.starts_with("--- ") {
                cur = Some(Pending::default());
            } else {
                continue;
            }
        }
        let p = cur.as_mut().expect("pending file present");
        p.take_header(raw);
    }

    if let Some(p) = cur.take() {
        files.push(p.finish());
    }
    files
}

#[derive(Default)]
struct Pending {
    a_path: Option<String>,
    b_path: Option<String>,
    old_path: Option<String>,
    new_path: Option<String>,
    prev_name: Option<String>,
    renamed: bool,
    added: bool,
    deleted: bool,
    binary: bool,
    hunks: Vec<Hunk>,
    old_no: u32,
    new_no: u32,
    remaining_old: u32,
    remaining_new: u32,
}

impl Pending {
    fn from_git_header(rest: &str) -> Self {
        let mut p = Self::default();
        // `a/x b/x`. Filenames containing " b/" are ambiguous here; the
        // `---`/`+++` lines override this guess whenever they are present.
        if let Some(split) = rest.find(" b/") {
            p.a_path = strip_side_prefix(&rest[..split]);
            p.b_path = strip_side_prefix(&rest[split + 1..]);
        }
        p
    }

    /// Consumes a line belonging to the open hunk. Returns false when the line
    /// is not hunk content, so the caller can treat it as a file header.
    fn take_content(&mut self, raw: &str) -> bool {
        let Some(hunk) = self.hunks.last_mut() else {
            return false;
        };
        if raw.starts_with('\\') {
            if let Some(last) = hunk.lines.last_mut() {
                last.no_newline = true;
                return true;
            }
            return false;
        }
        if self.remaining_old == 0 && self.remaining_new == 0 {
            return false;
        }
        let (kind, text) = match raw.chars().next() {
            Some('+') => (LineKind::Addition, &raw[1..]),
            Some('-') => (LineKind::Deletion, &raw[1..]),
            Some(' ') => (LineKind::Context, &raw[1..]),
            None => (LineKind::Context, raw),
            _ => return false,
        };
        let mut line = Line::new(kind, text);
        if kind != LineKind::Addition {
            line.old_no = Some(self.old_no);
            self.old_no += 1;
            self.remaining_old = self.remaining_old.saturating_sub(1);
        }
        if kind != LineKind::Deletion {
            line.new_no = Some(self.new_no);
            self.new_no += 1;
            self.remaining_new = self.remaining_new.saturating_sub(1);
        }
        hunk.lines.push(line);
        true
    }

    fn take_header(&mut self, raw: &str) {
        if raw.starts_with("@@") {
            self.start_hunk(raw);
        } else if raw.starts_with("new file mode") {
            self.added = true;
        } else if raw.starts_with("deleted file mode") {
            self.deleted = true;
        } else if let Some(from) = raw.strip_prefix("rename from ") {
            self.renamed = true;
            self.prev_name = Some(from.to_string());
        } else if let Some(to) = raw.strip_prefix("rename to ") {
            self.renamed = true;
            self.b_path = Some(to.to_string());
        } else if let Some(path) = raw.strip_prefix("--- ") {
            self.old_path = strip_side_prefix(path);
            self.added |= self.old_path.is_none();
        } else if let Some(path) = raw.strip_prefix("+++ ") {
            self.new_path = strip_side_prefix(path);
            self.deleted |= self.new_path.is_none();
        } else if raw.starts_with("Binary files ") || raw.starts_with("GIT binary patch") {
            self.binary = true;
        }
    }

    fn start_hunk(&mut self, raw: &str) {
        let Some(parsed) = parse_hunk_header(raw) else {
            return;
        };
        self.finish_hunk();
        let (old_start, old_lines, new_start, new_lines, header) = parsed;
        self.old_no = old_start;
        self.new_no = new_start;
        self.remaining_old = old_lines;
        self.remaining_new = new_lines;
        self.hunks.push(Hunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            header,
            lines: Vec::new(),
        });
    }

    fn finish_hunk(&mut self) {
        if let Some(hunk) = self.hunks.last_mut() {
            mark_intraline(&mut hunk.lines);
        }
    }

    fn status(&self) -> FileStatus {
        if self.binary {
            FileStatus::Binary
        } else if self.renamed {
            FileStatus::Renamed
        } else if self.added {
            FileStatus::Added
        } else if self.deleted {
            FileStatus::Deleted
        } else {
            FileStatus::Modified
        }
    }

    fn finish(mut self) -> FileDiff {
        self.finish_hunk();
        let status = self.status();
        let name = self
            .new_path
            .clone()
            .or_else(|| self.b_path.clone())
            .or_else(|| self.old_path.clone())
            .or_else(|| self.a_path.clone())
            .unwrap_or_default();
        let prev_name = if status == FileStatus::Renamed {
            self.prev_name
                .clone()
                .or_else(|| self.old_path.clone())
                .or_else(|| self.a_path.clone())
        } else {
            None
        };
        let lines = || self.hunks.iter().flat_map(|h| &h.lines);
        let additions = lines().filter(|l| l.kind == LineKind::Addition).count();
        let deletions = lines().filter(|l| l.kind == LineKind::Deletion).count();
        FileDiff {
            name,
            prev_name,
            status,
            additions,
            deletions,
            hunks: self.hunks,
        }
    }
}

/// `a/src/x.rs` -> `src/x.rs`; `/dev/null` -> `None`.
fn strip_side_prefix(path: &str) -> Option<String> {
    let path = path.split('\t').next().unwrap_or(path).trim_end();
    if path.is_empty() || path == "/dev/null" {
        return None;
    }
    let stripped = path
        .strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path);
    Some(stripped.to_string())
}

/// `@@ -1,2 +3,4 @@ header` -> starts, counts and the trimmed header text.
fn parse_hunk_header(raw: &str) -> Option<(u32, u32, u32, u32, String)> {
    let rest = raw.strip_prefix("@@")?;
    let end = rest.find("@@")?;
    let header = rest[end + 2..].trim().to_string();
    let mut ranges = rest[..end].split_whitespace();
    let (old_start, old_lines) = parse_range(ranges.next()?, '-')?;
    let (new_start, new_lines) = parse_range(ranges.next()?, '+')?;
    Some((old_start, old_lines, new_start, new_lines, header))
}

/// `-1,2` -> `(1, 2)`; `+7` -> `(7, 1)`.
fn parse_range(range: &str, sign: char) -> Option<(u32, u32)> {
    let range = range.strip_prefix(sign)?;
    let mut parts = range.split(',');
    let start = parts.next()?.parse().ok()?;
    let count = match parts.next() {
        Some(c) => c.parse().ok()?,
        None => 1,
    };
    Some((start, count))
}
