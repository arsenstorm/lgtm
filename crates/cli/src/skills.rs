//! Finding skills on disk for `lgtm skill import`: which directories hold a
//! `SKILL.md`, what each one reads as, and which of them a person picked.

use std::path::{Path, PathBuf};

use anyhow::Context;
use lgtm_protocol::{parse_skill_header, Skill, SkillFile};

/// A `SKILL.md` on its own, or the directory around it with every regular
/// file beside it. Hidden entries are skipped: an editor's swap file is not
/// part of a skill. Validated here so a bad file fails before the request.
pub(crate) fn read_skill(path: &Path) -> anyhow::Result<(String, Vec<SkillFile>)> {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    };
    let manifest = if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    };
    let content = std::fs::read_to_string(&manifest)
        .with_context(|| format!("read {}", manifest.display()))?;
    let mut files = Vec::new();
    if path.is_dir() {
        collect_files(&dir, &dir, &mut files)?;
    }
    lgtm_protocol::validate_skill(&content, &files).map_err(anyhow::Error::msg)?;
    Ok((content, files))
}

pub(crate) fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<SkillFile>,
) -> anyhow::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .expect("a walked file is under its root")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if relative == "SKILL.md" {
            continue;
        }
        let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        out.push(match String::from_utf8(bytes) {
            Ok(text) => SkillFile::text(relative, text),
            Err(err) => SkillFile::binary(relative, err.as_bytes()),
        });
    }
    Ok(())
}

/// How far below the named directory a `SKILL.md` is looked for. Deep enough
/// for `<checkout>/.claude/skills/<name>/SKILL.md`; a skill further down is
/// something else's vendored copy.
pub(crate) const MAX_DEPTH: usize = 4;

/// Directories walked into by nobody: vendored trees and git's own.
const SKIPPED: [&str; 3] = [".git", "node_modules", "target"];

/// Every directory under `dir` (itself included) that holds a `SKILL.md`,
/// sorted by path. A directory that holds one is not walked further, since
/// a skill's references are not skills; the starting directory is walked
/// regardless, because a skills repository keeps a `SKILL.md` at its root
/// and its collection under `skills/`.
pub(crate) fn discover(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(dir, dir, 0, &mut found);
    found.sort();
    found
}

fn walk(start: &Path, dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if dir.join("SKILL.md").is_file() {
        found.push(dir.to_path_buf());
        if dir != start {
            return;
        }
    }
    if depth == MAX_DEPTH {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read_dir.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if SKIPPED.iter().any(|skipped| name == *skipped) {
            continue;
        }
        walk(start, &path, depth + 1, found);
    }
}

/// A skill as read from one discovered directory.
#[derive(Debug)]
pub(crate) struct Candidate {
    pub dir: PathBuf,
    pub name: String,
    pub description: String,
    pub content: String,
    pub files: Vec<SkillFile>,
    /// The canonical path of `dir`, recorded on the skill.
    pub origin: String,
}

/// One discovered directory: a candidate, or why it is not one.
pub(crate) struct Found {
    pub dir: PathBuf,
    pub outcome: Result<Candidate, String>,
}

fn load_one(dir: &Path) -> Result<Candidate, String> {
    let (content, files) = read_skill(dir).map_err(|err| err.to_string())?;
    // read_skill already ran validate_skill, which parses the header.
    let header = parse_skill_header(&content).expect("read_skill already validated the header");
    let origin = dir
        .canonicalize()
        .unwrap_or_else(|_| dir.to_path_buf())
        .display()
        .to_string();
    Ok(Candidate {
        dir: dir.to_path_buf(),
        name: header.name,
        description: header.description,
        content,
        files,
        origin,
    })
}

/// Reads every discovered directory. A directory that does not read as a
/// skill is kept with its reason so the person sees it, not dropped. Two
/// directories claiming one name keep the shallower one; the other is a
/// duplicate.
pub(crate) fn load(dirs: &[PathBuf]) -> Vec<Found> {
    let mut outcomes: Vec<Result<Candidate, String>> =
        dirs.iter().map(|dir| load_one(dir)).collect();

    // Decide winners in (depth, path) order, independent of discovery order,
    // then mark every later same-named candidate a duplicate of the winner.
    let mut order: Vec<usize> = (0..outcomes.len())
        .filter(|&i| outcomes[i].is_ok())
        .collect();
    order.sort_by_key(|&i| {
        let dir = &outcomes[i]
            .as_ref()
            .expect("order was filtered to Ok candidates")
            .dir;
        (dir.components().count(), dir.clone())
    });

    let mut winners: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
    let mut duplicates: Vec<(usize, PathBuf)> = Vec::new();
    for i in order {
        let candidate = outcomes[i]
            .as_ref()
            .expect("order was filtered to Ok candidates");
        match winners.get(&candidate.name) {
            Some(winner_dir) => duplicates.push((i, winner_dir.clone())),
            None => {
                winners.insert(candidate.name.clone(), candidate.dir.clone());
            }
        }
    }
    for (i, winner_dir) in duplicates {
        outcomes[i] = Err(format!("duplicate of {}", winner_dir.display()));
    }

    dirs.iter()
        .cloned()
        .zip(outcomes)
        .map(|(dir, outcome)| Found { dir, outcome })
        .collect()
}

/// What to do with a candidate given what the orchestrator already holds in
/// the same scope.
pub(crate) enum Plan {
    Create,
    Update { id: String, revision: u32 },
}

pub(crate) fn plan(candidate: &Candidate, existing: &[Skill]) -> Plan {
    match existing.iter().find(|skill| skill.name == candidate.name) {
        Some(skill) => Plan::Update {
            id: skill.id.clone(),
            revision: skill.revision,
        },
        None => Plan::Create,
    }
}

/// `all`, `none`, an empty answer, or 1-based numbers and ranges such as
/// `1,3-5`. Indices returned are 0-based, deduplicated, in order.
pub(crate) fn parse_selection(answer: &str, count: usize) -> Result<Vec<usize>, String> {
    let answer = answer.trim();
    if answer.eq_ignore_ascii_case("all") {
        return Ok((0..count).collect());
    }
    if answer.is_empty() || answer.eq_ignore_ascii_case("none") {
        return Ok(Vec::new());
    }
    let mut indices = std::collections::BTreeSet::new();
    for token in answer
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|token| !token.is_empty())
    {
        let choice = |n: usize| -> Result<usize, String> {
            if n == 0 || n > count {
                Err(format!("not a choice: {token}"))
            } else {
                Ok(n - 1)
            }
        };
        if let Some((start, end)) = token.split_once('-') {
            let start: usize = start
                .parse()
                .map_err(|_| format!("not a choice: {token}"))?;
            let end: usize = end.parse().map_err(|_| format!("not a choice: {token}"))?;
            if start == 0 || end == 0 || start > end {
                return Err(format!("not a choice: {token}"));
            }
            for n in start..=end {
                indices.insert(choice(n)?);
            }
        } else {
            let n: usize = token
                .parse()
                .map_err(|_| format!("not a choice: {token}"))?;
            indices.insert(choice(n)?);
        }
    }
    Ok(indices.into_iter().collect())
}

/// The indices `--all` or `--only` name; `Ok(None)` when neither was given
/// and the person has to be asked. Unknown `--only` names are the error.
pub(crate) fn select(
    names: &[&str],
    all: bool,
    only: &[String],
) -> Result<Option<Vec<usize>>, String> {
    if all {
        return Ok(Some((0..names.len()).collect()));
    }
    if only.is_empty() {
        return Ok(None);
    }
    for wanted in only {
        if !names.contains(&wanted.as_str()) {
            return Err(format!(
                "{wanted} is not among the importable skills; see the list and any skipped lines above"
            ));
        }
    }
    Ok(Some(
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| only.iter().any(|wanted| wanted == *name))
            .map(|(i, _)| i)
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use lgtm_protocol::MemorySource;

    use super::*;

    fn skill_md(name: &str) -> String {
        format!("---\nname: {name}\ndescription: about {name}\n---\nbody\n")
    }

    struct TempTree {
        root: PathBuf,
    }

    impl TempTree {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("lgtm-skill-import-{nonce}"));
            std::fs::create_dir_all(&root).expect("create temp root");
            Self { root }
        }

        fn write(&self, relative: &str, content: &str) {
            let path = self.root.join(relative);
            std::fs::create_dir_all(path.parent().expect("has parent")).expect("create dirs");
            std::fs::write(path, content).expect("write file");
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn sample_tree() -> TempTree {
        let tree = TempTree::new();
        tree.write("SKILL.md", &skill_md("root-skill"));
        tree.write("skills/a/SKILL.md", &skill_md("a"));
        tree.write("skills/a/references/x.md", "ref");
        tree.write("skills/a/nested/SKILL.md", &skill_md("nested"));
        tree.write("skills/bad/SKILL.md", "not a skill");
        tree.write(".claude/skills/b/SKILL.md", &skill_md("b"));
        tree.write(".git/skills/h/SKILL.md", &skill_md("h"));
        tree.write("node_modules/p/SKILL.md", &skill_md("p"));
        tree.write("d1/d2/d3/d4/deep/SKILL.md", &skill_md("deep"));
        tree.write("d1/d2/d3/four/SKILL.md", &skill_md("four"));
        tree.write("other/x/a/SKILL.md", &skill_md("a"));
        tree
    }

    #[test]
    fn collect_files_carries_a_png_as_binary() {
        let tree = TempTree::new();
        let png = [137u8, 80, 78, 71, 0, 255];
        tree.write("SKILL.md", &skill_md("logo"));
        tree.write("notes.md", "hi");
        std::fs::create_dir_all(tree.root.join("assets")).expect("create assets dir");
        std::fs::write(tree.root.join("assets/a.png"), png).expect("write png");

        let (_, files) = read_skill(&tree.root).expect("skill reads");

        assert_eq!(files.len(), 2);
        let png_file = files
            .iter()
            .find(|f| f.path == "assets/a.png")
            .expect("png file present");
        assert!(png_file.binary);
        assert_eq!(png_file.bytes().unwrap(), png);
        let notes_file = files
            .iter()
            .find(|f| f.path == "notes.md")
            .expect("notes file present");
        assert!(!notes_file.binary);
    }

    #[test]
    fn discover_finds_skills_to_depth_four_and_skips_vendored_trees() {
        let tree = sample_tree();
        let found = discover(&tree.root);
        let mut expected = vec![
            tree.root.clone(),
            tree.root.join(".claude/skills/b"),
            tree.root.join("d1/d2/d3/four"),
            tree.root.join("other/x/a"),
            tree.root.join("skills/a"),
            tree.root.join("skills/bad"),
        ];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn load_keeps_a_bad_skill_with_its_reason_and_marks_duplicates() {
        let tree = sample_tree();
        let dirs = discover(&tree.root);
        let found = load(&dirs);

        let bad = &found
            .iter()
            .find(|f| f.dir == tree.root.join("skills/bad"))
            .expect("bad found")
            .outcome;
        let bad_err = bad.as_ref().expect_err("bad is not a skill");
        assert!(bad_err.contains("frontmatter"), "{bad_err}");

        let dup = &found
            .iter()
            .find(|f| f.dir == tree.root.join("other/x/a"))
            .expect("dup found")
            .outcome;
        let dup_err = dup.as_ref().expect_err("duplicate a");
        assert!(dup_err.contains("duplicate"), "{dup_err}");

        let a = &found
            .iter()
            .find(|f| f.dir == tree.root.join("skills/a"))
            .expect("a found")
            .outcome;
        let a = a.as_ref().expect("a loads");
        // `collect_files` (unchanged) only skips a literal top-level
        // `SKILL.md`, so `nested/SKILL.md` is picked up too, as a plain
        // reference file of `a` — `nested` is not discovered as its own
        // skill, but its manifest is still text under `a`'s directory.
        let mut paths: Vec<&str> = a.files.iter().map(|f| f.path.as_str()).collect();
        paths.sort_unstable();
        assert_eq!(paths, ["nested/SKILL.md", "references/x.md"]);
        assert!(a.origin.ends_with("skills/a"), "{}", a.origin);
    }

    #[test]
    fn parse_selection_reads_all_none_numbers_and_ranges() {
        assert_eq!(parse_selection("all", 4), Ok(vec![0, 1, 2, 3]));
        assert_eq!(parse_selection("", 4), Ok(vec![]));
        assert_eq!(parse_selection("none", 4), Ok(vec![]));
        assert_eq!(parse_selection(" 1, 3-4 ", 4), Ok(vec![0, 2, 3]));
        assert_eq!(parse_selection("2,2", 4), Ok(vec![1]));
        assert!(parse_selection("5", 4).is_err());
        assert!(parse_selection("x", 4).is_err());
    }

    #[test]
    fn select_honours_all_then_only_and_rejects_unknown_names() {
        let names = ["a", "b", "c"];
        assert_eq!(select(&names, true, &[]), Ok(Some(vec![0, 1, 2])));
        let only = vec!["b".to_string(), "a".to_string()];
        assert_eq!(select(&names, false, &only), Ok(Some(vec![0, 1])));
        let unknown = vec!["zzz".to_string()];
        let err = select(&names, false, &unknown).expect_err("zzz is unknown");
        assert!(err.contains("zzz"), "{err}");
        assert_eq!(select(&names, false, &[]), Ok(None));
    }

    #[test]
    fn plan_updates_a_same_named_skill_in_scope() {
        let existing = Skill {
            id: "s1".into(),
            name: "a".into(),
            description: "about a".into(),
            repository: None,
            content: skill_md("a"),
            files: vec![],
            origin: None,
            revision: 3,
            created_at: 0,
            updated_at: 0,
            source: MemorySource::User,
            verification: lgtm_protocol::Verification::UserApproved,
            proposed_by: None,
            workspace: None,
            created_by: None,
        };
        let candidate = Candidate {
            dir: PathBuf::from("skills/a"),
            name: "a".into(),
            description: "about a".into(),
            content: skill_md("a"),
            files: vec![],
            origin: "skills/a".into(),
        };
        match plan(&candidate, std::slice::from_ref(&existing)) {
            Plan::Update { id, revision } => {
                assert_eq!(id, "s1");
                assert_eq!(revision, 3);
            }
            Plan::Create => panic!("expected an update"),
        }

        let other = Candidate {
            name: "different".into(),
            ..candidate
        };
        match plan(&other, std::slice::from_ref(&existing)) {
            Plan::Create => {}
            Plan::Update { .. } => panic!("expected a create"),
        }
    }
}
