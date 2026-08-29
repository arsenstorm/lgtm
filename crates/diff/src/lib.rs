//! Parsing and layout for unified diffs.
//!
//! `parse_patch` turns `git diff` output into files, hunks and typed lines.
//! `layout` arranges one file's lines into unified or split rows for rendering,
//! and [`Anchor`] addresses a single line on one side of a file so comments and
//! marks can hang off it.

mod intraline;
mod parse;
pub mod tree;

use serde::{Deserialize, Serialize};

pub use parse::parse_patch;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Binary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    Deletions,
    Additions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiffStyle {
    Unified,
    Split,
}

/// A run of a line's text that is either changed or unchanged relative to the
/// line it is paired with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub text: String,
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// Line content without the leading `+`/`-`/space and without the newline.
    pub text: String,
    /// The whole line as one unchanged segment unless an intra-line diff applies.
    pub segments: Vec<Segment>,
    pub no_newline: bool,
}

impl Line {
    fn new(kind: LineKind, text: &str) -> Self {
        Line {
            kind,
            old_no: None,
            new_no: None,
            text: text.to_string(),
            segments: vec![Segment {
                text: text.to_string(),
                changed: false,
            }],
            no_newline: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// The text after the second `@@`, trimmed.
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    /// The new path, or the old path when the file was deleted.
    pub name: String,
    /// The old path, set when the file was renamed.
    pub prev_name: Option<String>,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<Hunk>,
}

/// Where a comment or mark attaches: a line number on one side of a file.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Anchor {
    pub file: String,
    pub side: Side,
    pub line: u32,
}

impl Line {
    /// The anchor this line can carry, if it has a line number on its side.
    pub fn anchor(&self, file: &str) -> Option<Anchor> {
        let (side, line) = match self.kind {
            LineKind::Addition | LineKind::Context => (Side::Additions, self.new_no?),
            LineKind::Deletion => (Side::Deletions, self.old_no?),
        };
        Some(Anchor {
            file: file.to_string(),
            side,
            line,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row<'a> {
    Hunk(&'a Hunk),
    Unified(&'a Line),
    Split {
        left: Option<&'a Line>,
        right: Option<&'a Line>,
    },
}

/// Rows for one file, ready to render.
///
/// Unified: a hunk row followed by one row per line. Split: a hunk row, then
/// context lines paired with themselves and each run of deletions paired
/// index-wise with the additions that follow it.
pub fn layout(file: &FileDiff, style: DiffStyle) -> Vec<Row<'_>> {
    let mut rows = Vec::new();
    for hunk in &file.hunks {
        rows.push(Row::Hunk(hunk));
        match style {
            DiffStyle::Unified => rows.extend(hunk.lines.iter().map(Row::Unified)),
            DiffStyle::Split => split_rows(&hunk.lines, &mut rows),
        }
    }
    rows
}

fn split_rows<'a>(lines: &'a [Line], rows: &mut Vec<Row<'a>>) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind == LineKind::Context {
            rows.push(Row::Split {
                left: Some(&lines[i]),
                right: Some(&lines[i]),
            });
            i += 1;
            continue;
        }
        let (add_start, end) = change_run(lines, i);
        let dels = add_start - i;
        let adds = end - add_start;
        for k in 0..dels.max(adds) {
            rows.push(Row::Split {
                left: (k < dels).then(|| &lines[i + k]),
                right: (k < adds).then(|| &lines[add_start + k]),
            });
        }
        i = end;
    }
}

/// The run of deletions starting at `start` and the additions after it:
/// `(first addition, first line past the run)`.
fn change_run(lines: &[Line], start: usize) -> (usize, usize) {
    let mut i = start;
    while i < lines.len() && lines[i].kind == LineKind::Deletion {
        i += 1;
    }
    let add_start = i;
    while i < lines.len() && lines[i].kind == LineKind::Addition {
        i += 1;
    }
    (add_start, i)
}
