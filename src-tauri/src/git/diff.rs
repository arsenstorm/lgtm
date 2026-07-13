use std::path::Path;

use crate::error::AppError;
use crate::git::exec::{run_git, run_git_ok};

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DiffSourceArgs {
    WorkingTree,
    Branch {
        #[serde(rename = "base")]
        base: String,
        #[serde(default)]
        head: Option<String>,
    },
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub patch: String,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub untracked: Vec<String>,
}

pub const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

const COMMON_DIFF_ARGS: [&str; 7] = [
    "--no-ext-diff",
    "--no-color",
    "--find-renames",
    "--submodule=short",
    "--unified=3",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

fn is_remote_ref(name: &str) -> bool {
    name.starts_with("origin/")
}

pub(crate) fn validate_ref_name(name: &str) -> Result<(), AppError> {
    let ok = !name.is_empty()
        && name.len() <= 256
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.chars().any(|c| c.is_whitespace() || c.is_control())
        && !name.contains(['~', '^', ':', '?', '*', '[', '\\']);
    if ok {
        Ok(())
    } else {
        Err(AppError::InvalidArgument {
            message: format!("Invalid ref name: {name}"),
        })
    }
}

async fn head_sha(repo_root: &Path) -> Option<String> {
    run_git(
        repo_root,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
    )
    .await
    .ok()
    .filter(|o| o.ok())
    .map(|o| o.stdout_text().trim().to_string())
}

async fn resolve_commit(repo_root: &Path, ref_name: &str) -> Result<String, AppError> {
    let verify_arg = format!("{ref_name}^{{commit}}");
    let verify = run_git(
        repo_root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            &verify_arg,
        ],
    )
    .await?;
    if !verify.ok() {
        let stderr = verify.stderr.trim();
        if stderr.is_empty() {
            return Err(AppError::InvalidArgument {
                message: "Unknown ref".to_string(),
            });
        }
        return Err(AppError::GitCommandFailed {
            command: format!("git rev-parse --verify --quiet --end-of-options {verify_arg}"),
            status: verify.status,
            stderr: stderr.chars().take(2000).collect(),
        });
    }
    Ok(verify.stdout_text().trim().to_string())
}

async fn branch_diff(
    exec_root: &Path,
    base_resolved: &str,
    head_resolved: &str,
) -> Result<DiffResult, AppError> {
    let merge_base = run_git_ok(
        exec_root,
        &[
            "merge-base",
            "--end-of-options",
            base_resolved,
            head_resolved,
        ],
    )
    .await?
    .stdout_text()
    .trim()
    .to_string();

    let mut args: Vec<&str> = vec!["diff"];
    args.extend_from_slice(&COMMON_DIFF_ARGS);
    args.push(&merge_base);
    args.push(head_resolved);
    let patch = run_git_ok(exec_root, &args).await?.stdout_text();

    Ok(DiffResult {
        patch,
        base_sha: Some(merge_base),
        head_sha: Some(head_resolved.to_string()),
        untracked: vec![],
    })
}

pub async fn get_diff(
    repo_root: &Path,
    source: &DiffSourceArgs,
    shadow_base: &Path,
) -> Result<DiffResult, AppError> {
    let head_sha = head_sha(repo_root).await;

    match source {
        DiffSourceArgs::WorkingTree => {
            let target = head_sha.as_deref().unwrap_or(EMPTY_TREE_SHA);
            let mut args: Vec<&str> = vec!["diff"];
            args.extend_from_slice(&COMMON_DIFF_ARGS);
            args.push(target);
            let patch = run_git_ok(repo_root, &args).await?.stdout_text();

            let untracked_out = run_git_ok(
                repo_root,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )
            .await?;
            let untracked = untracked_out
                .stdout
                .split(|&b| b == 0)
                .filter(|chunk| !chunk.is_empty())
                .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
                .collect();

            Ok(DiffResult {
                patch,
                base_sha: head_sha.clone(),
                head_sha,
                untracked,
            })
        }
        DiffSourceArgs::Branch { base, head } => {
            validate_ref_name(base)?;
            if let Some(head_ref) = head {
                validate_ref_name(head_ref)?;
            }
            let remote_involved = is_remote_ref(base) || head.as_deref().is_some_and(is_remote_ref);

            if remote_involved {
                let url_out = run_git(repo_root, &["remote", "get-url", "origin"]).await?;
                if !url_out.ok() {
                    return Err(AppError::InvalidArgument {
                        message: "The repository has no 'origin' remote to fetch from".to_string(),
                    });
                }
                let remote_url = url_out.stdout_text().trim().to_string();
                let shadow =
                    crate::git::shadow::ensure_shadow(shadow_base, repo_root, &remote_url).await?;
                crate::git::shadow::fetch_origin(&shadow, &remote_url).await?;

                let base_resolved = if is_remote_ref(base) {
                    resolve_commit(&shadow, base).await?
                } else {
                    resolve_commit(repo_root, base).await?
                };
                let head_resolved = match head.as_deref() {
                    Some(h) if is_remote_ref(h) => resolve_commit(&shadow, h).await?,
                    Some(h) => resolve_commit(repo_root, h).await?,
                    None => head_sha.ok_or_else(|| AppError::InvalidArgument {
                        message: "Repository has no commits on HEAD".to_string(),
                    })?,
                };
                return branch_diff(&shadow, &base_resolved, &head_resolved).await;
            }

            let base_resolved = resolve_commit(repo_root, base).await?;
            let head_resolved = match head {
                Some(head_ref) => resolve_commit(repo_root, head_ref).await?,
                None => head_sha.ok_or_else(|| AppError::InvalidArgument {
                    message: "Repository has no commits on HEAD".to_string(),
                })?,
            };
            branch_diff(repo_root, &base_resolved, &head_resolved).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, git, init_repo, write_file};

    fn canonical(dir: &std::path::Path) -> std::path::PathBuf {
        std::fs::canonicalize(dir).unwrap()
    }

    #[tokio::test]
    async fn working_tree_diff_excludes_untracked() {
        let dir = init_repo();
        write_file(dir.path(), "tracked.txt", "one\n");
        git(dir.path(), &["add", "tracked.txt"]);
        commit(dir.path(), "initial");

        write_file(dir.path(), "tracked.txt", "one\ntwo\n");
        write_file(dir.path(), "untracked.txt", "new\n");

        let root = canonical(dir.path());
        let result = get_diff(&root, &DiffSourceArgs::WorkingTree, &std::env::temp_dir())
            .await
            .unwrap();

        assert!(result.patch.contains("diff --git a/"));
        assert!(result.patch.contains("tracked.txt"));
        assert!(!result.patch.contains("untracked.txt"));
        assert_eq!(result.untracked, vec!["untracked.txt".to_string()]);
    }

    #[tokio::test]
    async fn working_tree_diff_unborn_repo_diffs_against_empty_tree() {
        let dir = init_repo();
        write_file(dir.path(), "staged.txt", "content\n");
        git(dir.path(), &["add", "staged.txt"]);

        let root = canonical(dir.path());
        let result = get_diff(&root, &DiffSourceArgs::WorkingTree, &std::env::temp_dir())
            .await
            .unwrap();

        assert!(result.patch.contains("staged.txt"));
        assert!(result.head_sha.is_none());
        assert!(result.base_sha.is_none());
    }

    #[tokio::test]
    async fn branch_diff_uses_merge_base_and_shows_only_feature_change() {
        let dir = init_repo();
        write_file(dir.path(), "base.txt", "base\n");
        git(dir.path(), &["add", "base.txt"]);
        commit(dir.path(), "initial");

        let merge_base_output = git(dir.path(), &["rev-parse", "HEAD"]);
        let expected_merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
            .trim()
            .to_string();

        git(dir.path(), &["checkout", "-b", "feature"]);
        write_file(dir.path(), "feature.txt", "feature change\n");
        git(dir.path(), &["add", "feature.txt"]);
        commit(dir.path(), "feature commit");

        git(dir.path(), &["checkout", "main"]);
        write_file(dir.path(), "main.txt", "main only change\n");
        git(dir.path(), &["add", "main.txt"]);
        commit(dir.path(), "main commit after branch");

        git(dir.path(), &["checkout", "feature"]);

        let root = canonical(dir.path());
        let result = get_diff(
            &root,
            &DiffSourceArgs::Branch {
                base: "main".to_string(),
                head: None,
            },
            &std::env::temp_dir(),
        )
        .await
        .unwrap();

        assert!(result.patch.contains("feature.txt"));
        assert!(!result.patch.contains("main.txt"));
        assert_eq!(result.base_sha, Some(expected_merge_base));
    }

    #[tokio::test]
    async fn branch_diff_with_explicit_head_reviews_unchecked_out_branch() {
        let dir = init_repo();
        write_file(dir.path(), "base.txt", "base\n");
        git(dir.path(), &["add", "base.txt"]);
        commit(dir.path(), "initial");

        git(dir.path(), &["checkout", "-b", "feature"]);
        write_file(dir.path(), "feature.txt", "feature change\n");
        git(dir.path(), &["add", "feature.txt"]);
        commit(dir.path(), "feature commit");

        git(dir.path(), &["checkout", "main"]);

        let head_output = git(dir.path(), &["rev-parse", "feature"]);
        let feature_sha = String::from_utf8_lossy(&head_output.stdout)
            .trim()
            .to_string();

        let root = canonical(dir.path());
        let result = get_diff(
            &root,
            &DiffSourceArgs::Branch {
                base: "main".to_string(),
                head: Some("feature".to_string()),
            },
            &std::env::temp_dir(),
        )
        .await
        .unwrap();

        assert!(result.patch.contains("feature.txt"));
        assert_eq!(result.head_sha, Some(feature_sha));
    }

    #[tokio::test]
    async fn branch_diff_detects_rename() {
        let dir = init_repo();
        write_file(dir.path(), "old.txt", "some content that stays\n");
        git(dir.path(), &["add", "old.txt"]);
        commit(dir.path(), "initial");

        git(dir.path(), &["checkout", "-b", "feature"]);
        git(dir.path(), &["mv", "old.txt", "new.txt"]);
        commit(dir.path(), "rename file");

        let root = canonical(dir.path());
        let result = get_diff(
            &root,
            &DiffSourceArgs::Branch {
                base: "main".to_string(),
                head: None,
            },
            &std::env::temp_dir(),
        )
        .await
        .unwrap();

        assert!(result.patch.contains("rename from"));
    }

    #[test]
    fn validate_ref_name_rejects_bad_refs() {
        let bad = [
            "",
            "-foo",
            "a..b",
            "a b",
            "a~1",
            "a^",
            "a:b",
            "a?",
            "a*",
            "a[b",
            "a\\b",
            &"x".repeat(300),
        ];
        for name in bad {
            assert!(
                validate_ref_name(name).is_err(),
                "expected {name:?} to be rejected"
            );
        }
    }

    #[test]
    fn validate_ref_name_accepts_good_refs() {
        for name in ["main", "feature/x-1", "v1.2.3"] {
            assert!(
                validate_ref_name(name).is_ok(),
                "expected {name:?} to be accepted"
            );
        }
    }

    #[tokio::test]
    async fn remote_ref_diff_fetches_via_shadow_without_touching_the_repo() {
        // The "remote" is a filesystem clone, so fetch works offline.
        let user = init_repo();
        write_file(user.path(), "base.txt", "base\n");
        git(user.path(), &["add", "base.txt"]);
        commit(user.path(), "initial");

        let clone_parent = tempfile::tempdir().unwrap();
        git(
            clone_parent.path(),
            &["clone", user.path().to_str().unwrap(), "clone"],
        );
        let clone = clone_parent.path().join("clone");
        git(&clone, &["checkout", "-b", "feature"]);
        write_file(&clone, "feature.txt", "from the remote\n");
        git(&clone, &["add", "feature.txt"]);
        commit(&clone, "feature commit");

        // The user repo points at the clone but never fetches it itself.
        git(
            user.path(),
            &["remote", "add", "origin", clone.to_str().unwrap()],
        );
        // A local-only commit on main proves the shadow sees the user's objects
        // through alternates when computing the merge base.
        write_file(user.path(), "local.txt", "local only\n");
        git(user.path(), &["add", "local.txt"]);
        commit(user.path(), "local commit");

        let shadow_base = tempfile::tempdir().unwrap();
        let root = canonical(user.path());
        let source = DiffSourceArgs::Branch {
            base: "main".to_string(),
            head: Some("origin/feature".to_string()),
        };

        let result = get_diff(&root, &source, shadow_base.path()).await.unwrap();
        assert!(result.patch.contains("feature.txt"));
        assert!(!result.patch.contains("local.txt"));

        // Second run reuses the same shadow (idempotent ensure + refetch).
        let again = get_diff(&root, &source, shadow_base.path()).await.unwrap();
        assert_eq!(again.patch, result.patch);

        // The user's repository gained no remote-tracking refs.
        let user_refs = git(user.path(), &["for-each-ref", "refs/remotes"]);
        assert!(String::from_utf8_lossy(&user_refs.stdout).trim().is_empty());
    }
}
