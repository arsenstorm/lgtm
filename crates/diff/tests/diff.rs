use lgtm_diff::tree::Tree;
use lgtm_diff::{layout, parse_patch, DiffStyle, FileStatus, LineKind, Row, Segment, Side};

const MULTI: &str = "\
diff --git a/src/added.rs b/src/added.rs
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/src/added.rs
@@ -0,0 +1,2 @@
+fn added() {}
+
diff --git a/src/mod.rs b/src/mod.rs
index 1111111..2222222 100644
--- a/src/mod.rs
+++ b/src/mod.rs
@@ -1,4 +1,4 @@ mod header

-let x = foo(1);
+let x = bar(1);
 use std::io;
 use std::fmt;
@@ -20,3 +20,4 @@
 tail one
 tail two
+tail three
 tail four
diff --git a/old/name.rs b/new/name.rs
similarity index 88%
rename from old/name.rs
rename to new/name.rs
index 3333333..4444444 100644
--- a/old/name.rs
+++ b/new/name.rs
@@ -10 +10,2 @@
-gone
+here
+and here
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 5555555..0000000
--- a/gone.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-first
-second
\\ No newline at end of file
diff --git a/logo.png b/logo.png
index 6666666..7777777 100644
Binary files a/logo.png and b/logo.png differ
";

#[test]
fn parses_a_multi_file_patch() {
    let files = parse_patch(MULTI);
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "src/added.rs",
            "src/mod.rs",
            "new/name.rs",
            "gone.txt",
            "logo.png"
        ]
    );

    let statuses: Vec<FileStatus> = files.iter().map(|f| f.status).collect();
    assert_eq!(
        statuses,
        [
            FileStatus::Added,
            FileStatus::Modified,
            FileStatus::Renamed,
            FileStatus::Deleted,
            FileStatus::Binary
        ]
    );

    let prev: Vec<Option<&str>> = files
        .iter()
        .map(|f| f.prev_name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(prev, [None, None, Some("old/name.rs"), None, None]);

    let counts: Vec<(usize, usize)> = files.iter().map(|f| (f.additions, f.deletions)).collect();
    assert_eq!(counts, [(2, 0), (2, 1), (2, 1), (0, 2), (0, 0)]);

    assert!(files[4].hunks.is_empty(), "binary files carry no hunks");
}

#[test]
fn computes_hunk_bounds_and_line_numbers() {
    let files = parse_patch(MULTI);
    let modified = &files[1];
    assert_eq!(modified.hunks.len(), 2);

    let first = &modified.hunks[0];
    assert_eq!(
        (
            first.old_start,
            first.old_lines,
            first.new_start,
            first.new_lines
        ),
        (1, 4, 1, 4)
    );
    assert_eq!(first.header, "mod header");
    assert_eq!(first.lines.len(), 5);
    assert_eq!(first.lines[0].kind, LineKind::Context);
    assert_eq!(first.lines[0].text, "");
    assert_eq!(
        (first.lines[0].old_no, first.lines[0].new_no),
        (Some(1), Some(1))
    );
    assert_eq!(
        (first.lines[1].old_no, first.lines[1].new_no),
        (Some(2), None)
    );
    assert_eq!(
        (first.lines[2].old_no, first.lines[2].new_no),
        (None, Some(2))
    );
    let last = first.lines.last().expect("hunk has lines");
    assert_eq!((last.old_no, last.new_no), (Some(4), Some(4)));

    let second = &modified.hunks[1];
    assert_eq!((second.old_start, second.new_start), (20, 20));
    assert_eq!(second.header, "");
    assert_eq!(second.lines[2].new_no, Some(22));
    assert_eq!(second.lines[2].old_no, None);

    // Omitted count on the old side of `@@ -10 +10,2 @@` means one line.
    let renamed = &files[2].hunks[0];
    assert_eq!(
        (
            renamed.old_start,
            renamed.old_lines,
            renamed.new_start,
            renamed.new_lines
        ),
        (10, 1, 10, 2)
    );
    assert_eq!(renamed.lines[2].new_no, Some(11));
}

#[test]
fn no_newline_marker_flags_the_previous_line() {
    let files = parse_patch(MULTI);
    let deleted = &files[3].hunks[0];
    assert_eq!(
        deleted.lines.len(),
        2,
        "the marker is not a line of its own"
    );
    assert!(!deleted.lines[0].no_newline);
    assert!(deleted.lines[1].no_newline);
    assert_eq!(deleted.lines[1].text, "second");
}

#[test]
fn anchors_address_the_right_side() {
    let files = parse_patch(MULTI);
    let hunk = &files[1].hunks[0];
    let deletion = hunk.lines[1].anchor("src/mod.rs").expect("deletion anchor");
    assert_eq!((deletion.side, deletion.line), (Side::Deletions, 2));
    let addition = hunk.lines[2].anchor("src/mod.rs").expect("addition anchor");
    assert_eq!((addition.side, addition.line), (Side::Additions, 2));
    let context = hunk.lines[3].anchor("src/mod.rs").expect("context anchor");
    assert_eq!(
        (context.side, context.line, context.file.as_str()),
        (Side::Additions, 3, "src/mod.rs")
    );
}

#[test]
fn word_segments_mark_only_the_changed_token() {
    let files = parse_patch(MULTI);
    let hunk = &files[1].hunks[0];
    assert_eq!(
        hunk.lines[1].segments,
        vec![
            Segment {
                text: "let x = ".into(),
                changed: false
            },
            Segment {
                text: "foo".into(),
                changed: true
            },
            Segment {
                text: "(1);".into(),
                changed: false
            },
        ]
    );
    assert_eq!(
        hunk.lines[2].segments,
        vec![
            Segment {
                text: "let x = ".into(),
                changed: false
            },
            Segment {
                text: "bar".into(),
                changed: true
            },
            Segment {
                text: "(1);".into(),
                changed: false
            },
        ]
    );
    // Unpaired lines keep one unchanged segment.
    let tail = &files[1].hunks[1].lines[2];
    assert_eq!(
        tail.segments,
        vec![Segment {
            text: "tail three".into(),
            changed: false
        }]
    );
}

#[test]
fn unified_layout_is_one_row_per_hunk_and_line() {
    let files = parse_patch(MULTI);
    let file = &files[1];
    let rows = layout(file, DiffStyle::Unified);
    let lines: usize = file.hunks.iter().map(|h| h.lines.len()).sum();
    assert_eq!(rows.len(), file.hunks.len() + lines);
    assert!(matches!(rows[0], Row::Hunk(_)));
    assert!(matches!(rows[1], Row::Unified(_)));
}

#[test]
fn split_layout_pairs_runs_index_wise() {
    let patch = "\
diff --git a/x.rs b/x.rs
index aaa..bbb 100644
--- a/x.rs
+++ b/x.rs
@@ -1,3 +1,2 @@
 keep
-one
-two
+uno
";
    let files = parse_patch(patch);
    let rows = layout(&files[0], DiffStyle::Split);
    assert_eq!(rows.len(), 4, "hunk row plus context plus two paired rows");

    let Row::Split { left, right } = &rows[1] else {
        panic!("expected a split row");
    };
    let (left, right) = (left.expect("left"), right.expect("right"));
    assert_eq!(left.text, "keep");
    assert!(std::ptr::eq(left, right), "context pairs with itself");

    let Row::Split { left, right } = &rows[2] else {
        panic!("expected a split row");
    };
    assert_eq!(left.expect("left").text, "one");
    assert_eq!(right.expect("right").text, "uno");

    let Row::Split { left, right } = &rows[3] else {
        panic!("expected a split row");
    };
    assert_eq!(left.expect("left").text, "two");
    assert!(right.is_none(), "the extra deletion has no partner");
}

#[test]
fn malformed_input_is_skipped_not_fatal() {
    assert!(parse_patch("").is_empty());
    assert!(parse_patch("not a patch at all\n@@ garbage @@\n").is_empty());
    let files = parse_patch("diff --git a/x b/x\n@@ nonsense @@\n+orphan\n");
    assert_eq!(files.len(), 1);
    assert!(files[0].hunks.is_empty());
}

#[test]
fn tree_flattens_single_child_directories() {
    let mut tree = Tree::build(["src/a/b/c.rs", "src/a/b/d.rs", "README.md"], true);
    let visible = tree.visible();
    let shown: Vec<(&str, usize)> = visible.iter().map(|n| (n.name.as_str(), n.depth)).collect();
    assert_eq!(
        shown,
        [("src/a/b", 0), ("c.rs", 1), ("d.rs", 1), ("README.md", 0)]
    );
    assert_eq!(visible[0].path, "src/a/b");
    assert!(visible[0].is_dir);
    assert!(!visible[1].is_dir);
    assert_eq!(tree.len(), 4);
    assert!(!tree.is_empty());

    tree.toggle("src/a/b");
    let after_toggle = tree.visible();
    let collapsed: Vec<&str> = after_toggle.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(collapsed, ["src/a/b", "README.md"]);

    tree.set_expanded("src/a/b", true);
    assert_eq!(tree.visible().len(), 4);
}

#[test]
fn tree_without_flattening_keeps_every_directory() {
    let tree = Tree::build(["src/a/b/c.rs", "src/a/b/d.rs", "README.md"], false);
    let visible = tree.visible();
    let shown: Vec<(&str, usize)> = visible.iter().map(|n| (n.name.as_str(), n.depth)).collect();
    assert_eq!(
        shown,
        [
            ("src", 0),
            ("a", 1),
            ("b", 2),
            ("c.rs", 3),
            ("d.rs", 3),
            ("README.md", 0)
        ]
    );
}

#[test]
fn tree_sorts_directories_first_case_insensitively() {
    let tree = Tree::build(["zeta.rs", "Alpha.rs", "beta/one.rs", "beta/two.rs"], true);
    let visible = tree.visible();
    let shown: Vec<&str> = visible.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(shown, ["beta", "one.rs", "two.rs", "Alpha.rs", "zeta.rs"]);
    assert!(Tree::build(Vec::<String>::new(), true).is_empty());
}
