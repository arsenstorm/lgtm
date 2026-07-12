pub mod client;
pub mod device;
pub mod token;

use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestInfo {
    pub owner: String,
    pub repository: String,
    pub pull_number: u64,
    pub title: String,
    pub author_login: String,
    pub state: String,
    pub draft: bool,
    pub base_ref: String,
    pub base_sha: String,
    pub head_ref: String,
    pub head_sha: String,
    pub changed_files: u64,
    pub additions: u64,
    pub deletions: u64,
    pub html_url: String,
    pub viewer_login: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSummary {
    pub number: u64,
    pub title: String,
    pub author_login: String,
    pub base_ref: String,
    pub head_ref: String,
    pub draft: bool,
    pub updated_at: String,
    pub html_url: String,
}

pub struct PrRef {
    pub owner: String,
    pub repository: String,
    pub pull_number: u64,
}

impl PrRef {
    /// "owner/repo#number", used as the `reference` in not-found errors.
    pub fn display(&self) -> String {
        format!("{}/{}#{}", self.owner, self.repository, self.pull_number)
    }
}

/// GitHub owner/repo path segments: letters, digits, `_`, `.`, `-`; never
/// exactly "." or "..". Used to keep unvalidated input out of request URLs.
/// (Equivalent to `^[A-Za-z0-9_.-]+$` minus "." and ".." — plain char checks,
/// no need to pull in a regex crate for this.)
pub fn is_valid_owner_or_repo(value: &str) -> bool {
    value != "."
        && value != ".."
        && !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
}

pub fn validate_owner_repo(owner: &str, repository: &str) -> Result<(), AppError> {
    if !is_valid_owner_or_repo(owner) || !is_valid_owner_or_repo(repository) {
        return Err(AppError::InvalidArgument {
            message: "Invalid GitHub owner or repository name".to_string(),
        });
    }
    Ok(())
}

pub fn parse_pr_url(input: &str) -> Result<PrRef, AppError> {
    let invalid = || AppError::InvalidArgument {
        message: "Not a GitHub pull request URL".to_string(),
    };

    let url = url::Url::parse(input.trim()).map_err(|_| invalid())?;

    if url.scheme() != "https" {
        return Err(invalid());
    }

    let host = url.host_str().ok_or_else(invalid)?;
    if host != "github.com" && host != "www.github.com" {
        return Err(invalid());
    }

    let segments: Vec<&str> = url
        .path_segments()
        .ok_or_else(invalid)?
        .filter(|s| !s.is_empty())
        .collect();

    // Trailing segments (e.g. "/files") are allowed; only the first four matter.
    if segments.len() < 4 || segments[2] != "pull" {
        return Err(invalid());
    }

    let owner = segments[0];
    let repository = segments[1];
    let pull_number: u64 = segments[3].parse().map_err(|_| invalid())?;

    if pull_number == 0 {
        return Err(invalid());
    }
    validate_owner_repo(owner, repository)?;

    Ok(PrRef {
        owner: owner.to_string(),
        repository: repository.to_string(),
        pull_number,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubReviewCommentInput {
    pub path: String,
    pub body: String,
    pub line: u64,
    pub side: String, // "LEFT" | "RIGHT"
    pub start_line: Option<u64>,
    pub start_side: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmittedReview {
    pub review_id: u64,
    pub html_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedGithubComment {
    pub id: String,
    pub pull_number: u64,
    pub path: String,
    pub body: String,
    pub diff_hunk: String,
    pub original_line: Option<u64>,
    pub side: Option<String>,
    pub author_login: String,
    pub commented_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPage {
    pub comments: Vec<ImportedGithubComment>,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub merged: bool,
    pub sha: Option<String>,
    pub message: String,
    pub branch_deleted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewInfo {
    pub id: u64,
    pub author_login: String,
    pub state: String,
    pub body: String,
    pub submitted_at: Option<String>,
    pub html_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrInlineComment {
    pub id: u64,
    pub author_login: String,
    pub path: String,
    pub line: Option<u64>,
    pub original_line: Option<u64>,
    pub side: Option<String>,
    pub body: String,
    pub created_at: String,
    pub html_url: String,
    pub in_reply_to_id: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationComment {
    pub id: u64,
    pub author_login: String,
    pub body: String,
    pub created_at: String,
    pub html_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRunInfo {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub details_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrCiStatus {
    pub check_runs: Vec<CheckRunInfo>,
    pub commit_state: String,
    pub mergeable: Option<bool>,
    pub mergeable_state: Option<String>,
    pub head_sha: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pr_url_accepts_plain_url() {
        let r = parse_pr_url("https://github.com/foo/bar/pull/12").unwrap();
        assert_eq!(r.owner, "foo");
        assert_eq!(r.repository, "bar");
        assert_eq!(r.pull_number, 12);
    }

    #[test]
    fn parse_pr_url_accepts_trailing_segments() {
        let r = parse_pr_url("https://github.com/foo/bar/pull/12/files").unwrap();
        assert_eq!(r.pull_number, 12);
    }

    #[test]
    fn parse_pr_url_accepts_www_host() {
        let r = parse_pr_url("https://www.github.com/foo/bar/pull/12").unwrap();
        assert_eq!(r.owner, "foo");
    }

    #[test]
    fn parse_pr_url_rejects_other_hosts() {
        assert!(parse_pr_url("https://gitlab.com/foo/bar/pull/12").is_err());
    }

    #[test]
    fn parse_pr_url_rejects_http() {
        assert!(parse_pr_url("http://github.com/foo/bar/pull/12").is_err());
    }

    #[test]
    fn parse_pr_url_rejects_missing_number() {
        assert!(parse_pr_url("https://github.com/foo/bar/pull/").is_err());
        assert!(parse_pr_url("https://github.com/foo/bar/pull").is_err());
    }

    #[test]
    fn parse_pr_url_rejects_issues_path() {
        assert!(parse_pr_url("https://github.com/foo/bar/issues/12").is_err());
    }

    #[test]
    fn parse_pr_url_rejects_dotdot_owner() {
        assert!(parse_pr_url("https://github.com/../bar/pull/12").is_err());
    }

    #[test]
    fn parse_pr_url_rejects_owner_with_slash() {
        // A literal "/" can't survive as a single path segment, but guard the
        // validator directly since URL parsing would just split it further.
        assert!(!is_valid_owner_or_repo("foo/bar"));
    }

    #[test]
    fn parse_pr_url_rejects_zero_number() {
        assert!(parse_pr_url("https://github.com/foo/bar/pull/0").is_err());
    }

    #[test]
    fn parse_pr_url_rejects_non_numeric_number() {
        assert!(parse_pr_url("https://github.com/foo/bar/pull/abc").is_err());
    }

    #[test]
    fn owner_repo_validation_rejects_dot_and_dotdot() {
        assert!(!is_valid_owner_or_repo("."));
        assert!(!is_valid_owner_or_repo(".."));
    }

    #[test]
    fn owner_repo_validation_rejects_invalid_characters() {
        for bad in ["foo/bar", "foo bar", "foo@bar", "foo:bar", ""] {
            assert!(
                !is_valid_owner_or_repo(bad),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn owner_repo_validation_accepts_typical_names() {
        for good in ["foo", "foo-bar", "foo_bar", "foo.bar", "123"] {
            assert!(
                is_valid_owner_or_repo(good),
                "expected {good:?} to be accepted"
            );
        }
    }
}
