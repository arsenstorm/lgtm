use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::git::exec::{run_git, run_git_ok};

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub root_path: String,
    pub display_name: String,
    pub current_branch: Option<String>,
    pub head_sha: Option<String>,
    pub detached: bool,
    pub unborn: bool,
    pub remote_url: Option<String>,
    pub default_base_branch: Option<String>,
    pub branches: Vec<String>,
}

pub async fn open_repository(path: &str) -> Result<RepositoryInfo, AppError> {
    let canonical = std::fs::canonicalize(path).map_err(|_| AppError::RepositoryNotFound {
        path: path.to_string(),
    })?;

    if !canonical.is_dir() {
        return Err(AppError::RepositoryNotFound {
            path: path.to_string(),
        });
    }

    let inside_work_tree = run_git(&canonical, &["rev-parse", "--is-inside-work-tree"]).await?;
    if !inside_work_tree.ok() || inside_work_tree.stdout_text().trim() != "true" {
        return Err(AppError::NotAGitRepository {
            path: canonical.display().to_string(),
        });
    }

    let toplevel = run_git_ok(&canonical, &["rev-parse", "--show-toplevel"]).await?;
    let root = std::fs::canonicalize(toplevel.stdout_text().trim()).map_err(|_| {
        AppError::NotAGitRepository {
            path: canonical.display().to_string(),
        }
    })?;
    let root_path = root.display().to_string();

    let display_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root_path.clone());

    let current_branch = run_git(&root, &["symbolic-ref", "--short", "-q", "HEAD"])
        .await
        .ok()
        .filter(|o| o.ok())
        .map(|o| o.stdout_text().trim().to_string());

    let head_sha = run_git(
        &root,
        &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
    )
    .await
    .ok()
    .filter(|o| o.ok())
    .map(|o| o.stdout_text().trim().to_string());

    let unborn = current_branch.is_some() && head_sha.is_none();
    let detached = current_branch.is_none() && head_sha.is_some();

    let remote_url = run_git(&root, &["remote", "get-url", "origin"])
        .await
        .ok()
        .filter(|o| o.ok())
        .map(|o| o.stdout_text().trim().to_string());

    let default_base_branch = default_base_branch(&root).await;

    let for_each_ref = run_git_ok(
        &root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .await?;
    let branches = for_each_ref
        .stdout_text()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(RepositoryInfo {
        root_path,
        display_name,
        current_branch,
        head_sha,
        detached,
        unborn,
        remote_url,
        default_base_branch,
        branches,
    })
}

async fn default_base_branch(root: &Path) -> Option<String> {
    if let Ok(out) = run_git(
        root,
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
    )
    .await
    {
        if out.ok() {
            let trimmed = out.stdout_text().trim().to_string();
            if let Some(stripped) = trimmed.strip_prefix("origin/") {
                return Some(stripped.to_string());
            }
            return Some(trimmed);
        }
    }

    for candidate in ["refs/heads/main", "refs/heads/master"] {
        if let Ok(out) = run_git(root, &["rev-parse", "--verify", "--quiet", candidate]).await {
            if out.ok() {
                return Some(candidate.trim_start_matches("refs/heads/").to_string());
            }
        }
    }

    None
}

/// Canonicalizes `repo_path` and verifies it is exactly the root of a Git
/// repository (not a subdirectory), returning the canonical root.
pub async fn validate_repo_path(repo_path: &str) -> Result<PathBuf, AppError> {
    let canonical = std::fs::canonicalize(repo_path).map_err(|_| AppError::RepositoryNotFound {
        path: repo_path.to_string(),
    })?;

    let toplevel = run_git_ok(&canonical, &["rev-parse", "--show-toplevel"]).await?;
    let root = std::fs::canonicalize(toplevel.stdout_text().trim()).map_err(|_| {
        AppError::InvalidArgument {
            message: "Path is not a repository root".to_string(),
        }
    })?;

    if root != canonical {
        return Err(AppError::InvalidArgument {
            message: "Path is not a repository root".to_string(),
        });
    }

    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{commit, git, init_repo, write_file};

    #[tokio::test]
    async fn valid_repo_returns_info() {
        let dir = init_repo();
        write_file(dir.path(), "a.txt", "hello\n");
        git(dir.path(), &["add", "a.txt"]);
        commit(dir.path(), "initial");

        let info = open_repository(dir.path().to_str().unwrap()).await.unwrap();
        let expected_root = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(info.root_path, expected_root.display().to_string());
        assert_eq!(info.current_branch.as_deref(), Some("main"));
        assert!(info.head_sha.is_some());
        assert!(!info.detached);
        assert!(!info.unborn);
    }

    #[tokio::test]
    async fn subdirectory_resolves_root() {
        let dir = init_repo();
        write_file(dir.path(), "a.txt", "hello\n");
        git(dir.path(), &["add", "a.txt"]);
        commit(dir.path(), "initial");

        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();

        let info = open_repository(sub.to_str().unwrap()).await.unwrap();
        let expected_root = std::fs::canonicalize(dir.path()).unwrap();
        assert_eq!(info.root_path, expected_root.display().to_string());
    }

    #[tokio::test]
    async fn non_git_dir_returns_not_a_git_repository() {
        let dir = tempfile::tempdir().unwrap();
        let result = open_repository(dir.path().to_str().unwrap()).await;
        assert!(matches!(result, Err(AppError::NotAGitRepository { .. })));
    }

    #[tokio::test]
    async fn nonexistent_path_returns_repository_not_found() {
        let missing = "/definitely/does/not/exist/lgtm-test";
        let result = open_repository(missing).await;
        assert!(matches!(result, Err(AppError::RepositoryNotFound { .. })));
    }

    #[tokio::test]
    async fn unborn_branch_has_no_head_sha() {
        let dir = init_repo();
        let info = open_repository(dir.path().to_str().unwrap()).await.unwrap();
        assert!(info.unborn);
        assert!(info.head_sha.is_none());
        assert_eq!(info.current_branch.as_deref(), Some("main"));
    }

    #[tokio::test]
    async fn detached_head_reports_no_branch() {
        let dir = init_repo();
        write_file(dir.path(), "a.txt", "hello\n");
        git(dir.path(), &["add", "a.txt"]);
        commit(dir.path(), "initial");
        let sha_output = git(dir.path(), &["rev-parse", "HEAD"]);
        let sha = String::from_utf8_lossy(&sha_output.stdout)
            .trim()
            .to_string();

        git(dir.path(), &["checkout", "--detach", &sha]);

        let info = open_repository(dir.path().to_str().unwrap()).await.unwrap();
        assert!(info.detached);
        assert!(info.current_branch.is_none());
    }

    #[tokio::test]
    async fn default_base_branch_falls_back_to_main() {
        let dir = init_repo();
        write_file(dir.path(), "a.txt", "hello\n");
        git(dir.path(), &["add", "a.txt"]);
        commit(dir.path(), "initial");

        let info = open_repository(dir.path().to_str().unwrap()).await.unwrap();
        assert_eq!(info.default_base_branch.as_deref(), Some("main"));
    }
}
