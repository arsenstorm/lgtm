//! A path-keyed file tree with expansion state, built from the paths of a diff.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Canonical path: `/`-separated, no trailing slash.
    pub path: String,
    /// Display name: the last segment, or `a/b/c` when directories were flattened.
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    path: String,
    name: String,
    is_dir: bool,
    expanded: bool,
    children: Vec<Entry>,
}

pub struct Tree {
    roots: Vec<Entry>,
    len: usize,
}

/// Intermediate nested map used only while building.
#[derive(Default)]
struct Raw {
    children: BTreeMap<String, Raw>,
    is_file: bool,
}

impl Tree {
    /// Builds a tree from file paths, inferring the directories between them.
    ///
    /// With `flatten_empty_dirs`, a directory whose only child is a directory
    /// collapses into a single node named `a/b`. All directories start expanded.
    pub fn build<I: IntoIterator<Item = S>, S: AsRef<str>>(
        paths: I,
        flatten_empty_dirs: bool,
    ) -> Self {
        let mut root = Raw::default();
        for path in paths {
            let mut node = &mut root;
            let segments: Vec<&str> = path.as_ref().split('/').filter(|s| !s.is_empty()).collect();
            let Some((last, dirs)) = segments.split_last() else {
                continue;
            };
            for segment in dirs {
                node = node.children.entry((*segment).to_string()).or_default();
            }
            let leaf = node.children.entry((*last).to_string()).or_default();
            leaf.is_file = true;
        }
        let roots = convert(&root, "", flatten_empty_dirs);
        let len = roots.iter().map(Entry::count).sum();
        Self { roots, len }
    }

    pub fn toggle(&mut self, path: &str) {
        if let Some(entry) = find_mut(&mut self.roots, path) {
            entry.expanded = !entry.expanded;
        }
    }

    pub fn set_expanded(&mut self, path: &str, expanded: bool) {
        if let Some(entry) = find_mut(&mut self.roots, path) {
            entry.expanded = expanded;
        }
    }

    /// Depth-first visible nodes: children of collapsed directories are omitted,
    /// directories come before files, both alphabetical (case-insensitive).
    pub fn visible(&self) -> Vec<Node> {
        let mut out = Vec::new();
        push_visible(&self.roots, 0, &mut out);
        out
    }

    /// Total number of nodes in the tree, visible or not.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Entry {
    fn count(&self) -> usize {
        1 + self.children.iter().map(Entry::count).sum::<usize>()
    }
}

fn convert(raw: &Raw, prefix: &str, flatten: bool) -> Vec<Entry> {
    let mut entries: Vec<Entry> = raw
        .children
        .iter()
        .map(|(name, child)| {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let is_dir = !child.children.is_empty();
            let mut entry = Entry {
                name: name.clone(),
                children: convert(child, &path, flatten),
                path,
                is_dir,
                expanded: true,
            };
            if flatten {
                collapse(&mut entry);
            }
            entry
        })
        .collect();
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    entries
}

/// A directory whose only child is a directory becomes one node named `a/b`.
fn collapse(entry: &mut Entry) {
    while entry.is_dir && entry.children.len() == 1 && entry.children[0].is_dir {
        let child = entry.children.remove(0);
        entry.name = format!("{}/{}", entry.name, child.name);
        entry.path = child.path;
        entry.children = child.children;
    }
}

fn find_mut<'a>(entries: &'a mut [Entry], path: &str) -> Option<&'a mut Entry> {
    for entry in entries {
        if entry.path == path {
            return Some(entry);
        }
        if path.starts_with(&entry.path) && path.as_bytes().get(entry.path.len()) == Some(&b'/') {
            return find_mut(&mut entry.children, path);
        }
    }
    None
}

fn push_visible(entries: &[Entry], depth: usize, out: &mut Vec<Node>) {
    for entry in entries {
        out.push(Node {
            path: entry.path.clone(),
            name: entry.name.clone(),
            depth,
            is_dir: entry.is_dir,
            expanded: entry.expanded,
        });
        if entry.is_dir && entry.expanded {
            push_visible(&entry.children, depth + 1, out);
        }
    }
}
