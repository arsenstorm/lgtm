//! State behind the Changes tab: the parsed diff for the selected task, what
//! has been marked viewed, and the comments queued for a follow-up.

use gpui::{actions, Entity, ScrollHandle};
use gpui_component::input::InputState;
use lgtm_diff::tree::Tree;
use lgtm_diff::{Anchor, DiffStyle, FileDiff, Side};
use std::collections::HashSet;
use std::path::PathBuf;

actions!(review, [MarkViewed, NextFile, PrevFile, ToggleDiffStyle]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub anchor: Anchor,
    pub text: String,
}

pub struct ReviewState {
    pub style: DiffStyle,
    pub task_id: Option<String>,
    pub files: Vec<FileDiff>,
    pub tree: Option<Tree>,
    pub viewed: HashSet<String>,
    pub comments: Vec<Comment>,
    pub draft: Option<(Anchor, Entity<InputState>)>,
    pub current_file: usize,
    /// The file column scrolls as one list, so jumping to a file from the tree
    /// or with n/p is `scroll_to_top_of_item(file index)`.
    pub scroll: ScrollHandle,
    /// Kept so `load` can tell a plain re-render from a diff that grew.
    patch: String,
}

impl Default for ReviewState {
    fn default() -> Self {
        Self::new()
    }
}

impl ReviewState {
    pub fn new() -> Self {
        Self::blank(stored_style())
    }

    fn blank(style: DiffStyle) -> Self {
        Self {
            style,
            task_id: None,
            files: Vec::new(),
            tree: None,
            viewed: HashSet::new(),
            comments: Vec::new(),
            draft: None,
            current_file: 0,
            scroll: ScrollHandle::new(),
            patch: String::new(),
        }
    }

    /// Re-parses when the task or its diff changed; resets viewed/comments/draft
    /// when the task id changes.
    pub fn load(&mut self, task_id: &str, diff: &str) {
        let same_task = self.task_id.as_deref() == Some(task_id);
        if same_task && self.patch == diff {
            return;
        }
        if !same_task {
            self.task_id = Some(task_id.to_string());
            self.viewed.clear();
            self.comments.clear();
            self.draft = None;
            self.current_file = 0;
        }
        self.patch = diff.to_string();
        self.files = lgtm_diff::parse_patch(diff);
        let names: Vec<String> = self.files.iter().map(|file| file.name.clone()).collect();
        self.tree = Some(Tree::build(&names, true));
        self.current_file = self.current_file.min(self.files.len().saturating_sub(1));
    }

    pub fn set_style(&mut self, style: DiffStyle) {
        self.style = style;
        persist_style(style);
    }

    pub fn flip_style(&mut self) {
        let flipped = match self.style {
            DiffStyle::Unified => DiffStyle::Split,
            DiffStyle::Split => DiffStyle::Unified,
        };
        self.set_style(flipped);
    }

    pub fn comment_count(&self) -> usize {
        self.comments.len()
    }

    /// The follow-up text for all comments, or None when there are none.
    pub fn request_changes_message(&self) -> Option<String> {
        if self.comments.is_empty() {
            return None;
        }
        let mut sorted: Vec<&Comment> = self.comments.iter().collect();
        sorted
            .sort_by(|a, b| (&a.anchor.file, a.anchor.line).cmp(&(&b.anchor.file, b.anchor.line)));
        let body: Vec<String> = sorted
            .iter()
            .map(|comment| {
                let sign = match comment.anchor.side {
                    Side::Additions => '+',
                    Side::Deletions => '-',
                };
                format!(
                    "{}:{} {sign}\n{}",
                    comment.anchor.file, comment.anchor.line, comment.text
                )
            })
            .collect();
        Some(format!("Review comments:\n\n{}", body.join("\n\n")))
    }

    pub fn toggle_viewed(&mut self, file: &str) {
        if !self.viewed.remove(file) {
            self.viewed.insert(file.to_string());
        }
    }

    pub fn mark_current_viewed(&mut self) {
        let Some(name) = self.files.get(self.current_file).map(|f| f.name.clone()) else {
            return;
        };
        self.toggle_viewed(&name);
    }

    /// Moves the cursor `delta` files and scrolls that file into view.
    pub fn step_file(&mut self, delta: isize) {
        if self.files.is_empty() {
            return;
        }
        let last = self.files.len() as isize - 1;
        self.current_file = (self.current_file as isize + delta).clamp(0, last) as usize;
        self.scroll.scroll_to_top_of_item(self.current_file);
    }

    pub fn focus_file(&mut self, index: usize) {
        self.current_file = index;
        self.scroll.scroll_to_top_of_item(index);
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".lgtm/desktop.toml"))
}

fn config() -> toml::Table {
    config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn stored_style() -> DiffStyle {
    match config().get("diff_style").and_then(toml::Value::as_str) {
        Some("split") => DiffStyle::Split,
        _ => DiffStyle::Unified,
    }
}

/// Read-modify-write so the token and orchestrator keys survive the toggle.
/// Best effort: an unwritable home directory must not break the button.
fn persist_style(style: DiffStyle) {
    let Some(path) = config_path() else {
        return;
    };
    let name = match style {
        DiffStyle::Unified => "unified",
        DiffStyle::Split => "split",
    };
    let mut table = config();
    table.insert("diff_style".into(), toml::Value::String(name.into()));
    let _ = std::fs::write(path, table.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(file: &str, line: u32, side: Side) -> Anchor {
        Anchor {
            file: file.to_string(),
            side,
            line,
        }
    }

    fn comment(file: &str, line: u32, side: Side, text: &str) -> Comment {
        Comment {
            anchor: anchor(file, line, side),
            text: text.to_string(),
        }
    }

    const PATCH: &str = concat!(
        "diff --git a/src/main.rs b/src/main.rs\n",
        "--- a/src/main.rs\n",
        "+++ b/src/main.rs\n",
        "@@ -1,2 +1,2 @@\n",
        " fn main() {\n",
        "-    old();\n",
        "+    new();\n",
    );

    #[test]
    fn request_changes_message_lists_comments_by_file_then_line() {
        let mut state = ReviewState::blank(DiffStyle::Unified);
        state.comments = vec![
            comment("src/main.rs", 12, Side::Deletions, "drop this"),
            comment("src/lib.rs", 3, Side::Additions, "name it better"),
        ];
        assert_eq!(
            state.request_changes_message().unwrap(),
            "Review comments:\n\nsrc/lib.rs:3 +\nname it better\n\nsrc/main.rs:12 -\ndrop this"
        );
    }

    #[test]
    fn request_changes_message_is_none_without_comments() {
        assert!(ReviewState::blank(DiffStyle::Unified)
            .request_changes_message()
            .is_none());
    }

    #[test]
    fn load_keeps_comments_for_the_same_task_and_drops_them_for_a_new_one() {
        let mut state = ReviewState::blank(DiffStyle::Unified);
        state.load("task-1", PATCH);
        state
            .comments
            .push(comment("src/main.rs", 2, Side::Additions, "hi"));
        state.viewed.insert("src/main.rs".into());

        state.load("task-1", PATCH);
        assert_eq!(state.comment_count(), 1);
        assert!(state.viewed.contains("src/main.rs"));

        state.load("task-2", PATCH);
        assert_eq!(state.comment_count(), 0);
        assert!(state.viewed.is_empty());
        assert_eq!(state.task_id.as_deref(), Some("task-2"));
    }

    #[test]
    fn toggle_viewed_flips_membership() {
        let mut state = ReviewState::blank(DiffStyle::Unified);
        state.toggle_viewed("a.rs");
        assert!(state.viewed.contains("a.rs"));
        state.toggle_viewed("a.rs");
        assert!(!state.viewed.contains("a.rs"));
    }
}
