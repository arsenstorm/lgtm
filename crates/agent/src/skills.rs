//! Puts the skills a task was handed into its worktree, where Claude and
//! Codex each look for a `SKILL.md`, and keeps them out of every commit.

use std::path::Path;

use anyhow::{bail, Context, Result};
use lgtm_protocol::{validate_skill_path, Skill, SkillRef};

use crate::git::exclude;

/// Where the skills live; the harness directories only link here.
pub const DIR: &str = ".lgtm/skills";
/// Where each harness discovers skills under the working directory.
const HARNESS_DIRS: [&str; 2] = [".claude/skills", ".agents/skills"];
/// The names linked into the harness directories by the last run, so a
/// skill removed since can be unlinked before this run.
const DELIVERED: &str = ".delivered";

/// Writes every skill under `.lgtm/skills/<name>/` and links it from each
/// harness directory, replacing what an earlier run of this task left. A
/// path the repository ships itself at a harness location is left alone:
/// the repository's own skill is the more specific one. Returns what was
/// written, for the task's record.
pub async fn materialise(worktree: &Path, skills: &[Skill]) -> Result<Vec<SkillRef>> {
    let root = worktree.join(DIR);
    unlink_previous(worktree, &root).await;
    if root.exists() {
        tokio::fs::remove_dir_all(&root)
            .await
            .with_context(|| format!("clear {}", root.display()))?;
    }
    if skills.is_empty() {
        return Ok(Vec::new());
    }
    let mut delivered = Vec::new();
    for skill in skills {
        write_skill(&root, skill).await?;
        for dir in HARNESS_DIRS {
            link(worktree, dir, &skill.name, &root.join(&skill.name)).await?;
        }
        delivered.push(skill.reference());
    }
    let names: Vec<&str> = skills.iter().map(|skill| skill.name.as_str()).collect();
    tokio::fs::write(root.join(DELIVERED), names.join("\n")).await?;
    exclude(worktree, &format!("{DIR}/")).await?;
    for dir in HARNESS_DIRS {
        for name in &names {
            exclude(worktree, &format!("{dir}/{name}")).await?;
        }
    }
    Ok(delivered)
}

async fn write_skill(root: &Path, skill: &Skill) -> Result<()> {
    let dir = root.join(&skill.name);
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(dir.join("SKILL.md"), &skill.content).await?;
    for file in &skill.files {
        // The orchestrator validated these; a bad one here is a bug, not
        // something to write around.
        if let Err(reason) = validate_skill_path(&file.path) {
            bail!("skill {}: {reason}", skill.name);
        }
        let path = dir.join(&file.path);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let bytes = match file.bytes() {
            Ok(bytes) => bytes,
            Err(reason) => bail!("skill {}: {reason}", skill.name),
        };
        tokio::fs::write(&path, bytes).await?;
    }
    Ok(())
}

/// Removes what the last run linked, whether a symlink or a copied tree, and
/// nothing else: a directory the repository ships under the same harness
/// path was never in the list.
async fn unlink_previous(worktree: &Path, root: &Path) {
    let Ok(listed) = tokio::fs::read_to_string(root.join(DELIVERED)).await else {
        return;
    };
    for name in listed.lines().filter(|name| !name.is_empty()) {
        for dir in HARNESS_DIRS {
            let path = worktree.join(dir).join(name);
            let Ok(meta) = tokio::fs::symlink_metadata(&path).await else {
                continue;
            };
            let _ = if meta.is_dir() {
                tokio::fs::remove_dir_all(&path).await
            } else {
                tokio::fs::remove_file(&path).await
            };
        }
    }
}

async fn link(worktree: &Path, dir: &str, name: &str, target: &Path) -> Result<()> {
    let link = worktree.join(dir).join(name);
    if tokio::fs::symlink_metadata(&link).await.is_ok() {
        tracing::warn!(skill = name, path = %link.display(), "the repository ships its own; leaving it");
        return Ok(());
    }
    if let Some(parent) = link.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    place(target, &link)
        .await
        .with_context(|| format!("link {}", link.display()))
}

/// A relative symlink, so the worktree can move; `<dir>/<name>` is two
/// levels below the worktree root.
#[cfg(unix)]
async fn place(target: &Path, link: &Path) -> std::io::Result<()> {
    let relative = Path::new("../..")
        .join(DIR)
        .join(target.file_name().expect("a skill directory has a name"));
    tokio::fs::symlink(relative, link).await
}

/// Windows symlinks need Developer Mode, so the tree is copied instead.
#[cfg(not(unix))]
async fn place(target: &Path, link: &Path) -> std::io::Result<()> {
    copy_dir(target, link)
}

#[cfg(not(unix))]
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use lgtm_protocol::{MemorySource, SkillFile, Verification};

    use super::*;
    use crate::git::git;

    fn skill(name: &str, files: Vec<(&str, &str)>) -> Skill {
        Skill {
            id: name.to_string(),
            name: name.to_string(),
            description: "d".to_string(),
            repository: None,
            content: format!("---\nname: {name}\ndescription: d\n---\nbody"),
            files: files
                .into_iter()
                .map(|(path, content)| SkillFile::text(path, content))
                .collect(),
            origin: None,
            revision: 1,
            created_at: 0,
            updated_at: 0,
            source: MemorySource::User,
            verification: Verification::UserApproved,
            proposed_by: None,
            workspace: None,
            created_by: None,
        }
    }

    async fn repo(tag: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("lgtm-skills-{tag}-{nonce}"));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        git(&["init", "-q", &dir.display().to_string()], None)
            .await
            .unwrap();
        dir
    }

    #[tokio::test]
    async fn skills_land_where_both_harnesses_look_and_stay_out_of_git() {
        let dir = repo("both").await;
        let skills = vec![
            skill("review", vec![("references/rules.md", "r")]),
            skill("commit", vec![]),
        ];

        let refs = materialise(&dir, &skills).await.unwrap();

        assert!(dir.join(".lgtm/skills/review/SKILL.md").exists());
        assert_eq!(
            tokio::fs::read_to_string(dir.join(".claude/skills/review/SKILL.md"))
                .await
                .unwrap(),
            skills[0].content
        );
        assert_eq!(
            tokio::fs::read_to_string(dir.join(".agents/skills/review/references/rules.md"))
                .await
                .unwrap(),
            "r"
        );
        assert_eq!(
            refs,
            vec![
                SkillRef {
                    name: "review".to_string(),
                    revision: 1
                },
                SkillRef {
                    name: "commit".to_string(),
                    revision: 1
                },
            ]
        );

        let status = git(
            &["-C", &dir.display().to_string(), "status", "--porcelain"],
            None,
        )
        .await
        .unwrap();
        assert_eq!(status.trim(), "");

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn a_binary_file_lands_byte_for_byte() {
        let dir = repo("binary").await;
        let bytes = [137u8, 80, 78, 71, 13, 10, 26, 10, 0, 255];
        let mut logo = skill("logo", vec![]);
        logo.files
            .push(SkillFile::binary("assets/logo.png", &bytes));

        materialise(&dir, &[logo]).await.unwrap();

        let written = tokio::fs::read(dir.join(".claude/skills/logo/assets/logo.png"))
            .await
            .unwrap();
        assert_eq!(written, bytes);

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn a_second_run_drops_the_skill_that_went_away() {
        let dir = repo("drop").await;
        let skills = vec![skill("review", vec![]), skill("commit", vec![])];
        materialise(&dir, &skills).await.unwrap();

        materialise(&dir, &[skill("review", vec![])]).await.unwrap();

        assert!(
            tokio::fs::symlink_metadata(dir.join(".claude/skills/commit"))
                .await
                .is_err()
        );
        assert!(
            tokio::fs::symlink_metadata(dir.join(".agents/skills/commit"))
                .await
                .is_err()
        );
        assert!(!dir.join(".lgtm/skills/commit").exists());
        assert_eq!(
            tokio::fs::read_to_string(dir.join(".claude/skills/review/SKILL.md"))
                .await
                .unwrap(),
            skill("review", vec![]).content
        );

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn a_skill_the_repository_ships_is_left_alone() {
        let dir = repo("theirs").await;
        tokio::fs::create_dir_all(dir.join(".claude/skills/review"))
            .await
            .unwrap();
        tokio::fs::write(dir.join(".claude/skills/review/SKILL.md"), "theirs")
            .await
            .unwrap();

        let refs = materialise(&dir, &[skill("review", vec![])]).await.unwrap();

        assert_eq!(
            tokio::fs::read_to_string(dir.join(".claude/skills/review/SKILL.md"))
                .await
                .unwrap(),
            "theirs"
        );
        assert_eq!(
            tokio::fs::read_to_string(dir.join(".agents/skills/review/SKILL.md"))
                .await
                .unwrap(),
            skill("review", vec![]).content
        );
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "review");

        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn an_empty_delivery_clears_the_directory() {
        let dir = repo("empty").await;
        materialise(&dir, &[skill("review", vec![])]).await.unwrap();

        let refs = materialise(&dir, &[]).await.unwrap();

        assert!(refs.is_empty());
        assert!(!dir.join(".lgtm/skills").exists());
        assert!(
            tokio::fs::symlink_metadata(dir.join(".claude/skills/review"))
                .await
                .is_err()
        );
        assert!(
            tokio::fs::symlink_metadata(dir.join(".agents/skills/review"))
                .await
                .is_err()
        );

        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
