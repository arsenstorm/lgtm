//! Splits a unified diff into per-file blocks the Diff tab can colour.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Add,
    Del,
    Hunk,
    Context,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub lines: Vec<Line>,
}

/// Header lines carrying no content: the path is already the file's title.
const NOISE: [&str; 8] = [
    "index ",
    "--- ",
    "+++ ",
    "new file mode ",
    "deleted file mode ",
    "old mode ",
    "new mode ",
    "similarity index ",
];

pub fn parse(diff: &str) -> Vec<File> {
    let mut files: Vec<File> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(File {
                path: path_of(rest),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if NOISE.iter().any(|prefix| line.starts_with(prefix)) {
            continue;
        }
        let kind = match line.as_bytes().first() {
            Some(b'+') => Kind::Add,
            Some(b'-') => Kind::Del,
            Some(b'@') => Kind::Hunk,
            _ => Kind::Context,
        };
        file.lines.push(Line {
            kind,
            text: line.to_string(),
        });
    }
    files
}

/// `a/src/main.rs b/src/main.rs` -> `src/main.rs`. Paths with spaces in them
/// keep the raw text rather than guessing where the halves split.
fn path_of(rest: &str) -> String {
    let mut parts = rest.split_whitespace();
    let (Some(_), Some(b), None) = (parts.next(), parts.next(), parts.next()) else {
        return rest.to_string();
    };
    b.strip_prefix("b/").unwrap_or(b).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_FILES: &str = concat!(
        "diff --git a/src/main.rs b/src/main.rs\n",
        "index 1111111..2222222 100644\n",
        "--- a/src/main.rs\n",
        "+++ b/src/main.rs\n",
        "@@ -1,3 +1,3 @@\n",
        " fn main() {\n",
        "-    old();\n",
        "+    new();\n",
        " }\n",
        "diff --git a/README.md b/README.md\n",
        "new file mode 100644\n",
        "--- /dev/null\n",
        "+++ b/README.md\n",
        "@@ -0,0 +1 @@\n",
        "+hello\n",
    );

    #[test]
    fn empty_diff_has_no_files() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn two_file_diff_splits_by_path_with_line_kinds() {
        let files = parse(TWO_FILES);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[1].path, "README.md");
        assert_eq!(
            files[0].lines,
            vec![
                Line {
                    kind: Kind::Hunk,
                    text: "@@ -1,3 +1,3 @@".into()
                },
                Line {
                    kind: Kind::Context,
                    text: " fn main() {".into()
                },
                Line {
                    kind: Kind::Del,
                    text: "-    old();".into()
                },
                Line {
                    kind: Kind::Add,
                    text: "+    new();".into()
                },
                Line {
                    kind: Kind::Context,
                    text: " }".into()
                },
            ]
        );
        assert_eq!(
            files[1].lines,
            vec![
                Line {
                    kind: Kind::Hunk,
                    text: "@@ -0,0 +1 @@".into()
                },
                Line {
                    kind: Kind::Add,
                    text: "+hello".into()
                },
            ]
        );
    }
}
