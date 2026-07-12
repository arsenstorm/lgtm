use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::github::client::GithubClient;
use crate::github::device::{self, DeviceFlowManager, DeviceFlowStart};
use crate::github::{
    self, GithubReviewCommentInput, ImportPage, ImportedGithubComment, PrRef, PullRequestInfo,
    SubmittedReview,
};

// ---- Private GitHub API response shapes (only the fields we use). ----

#[derive(Debug, Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GhRef {
    #[serde(rename = "ref")]
    ref_name: String,
    sha: String,
}

#[derive(Debug, Deserialize)]
struct GhPullRequest {
    number: u64,
    title: String,
    user: GhUser,
    state: String,
    draft: bool,
    base: GhRef,
    head: GhRef,
    changed_files: u64,
    additions: u64,
    deletions: u64,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct GhReview {
    id: u64,
    html_url: String,
}

#[derive(Debug, Deserialize)]
struct GhReviewComment {
    id: u64,
    path: String,
    body: String,
    #[serde(default)]
    diff_hunk: String,
    line: Option<u64>,
    original_line: Option<u64>,
    side: Option<String>,
    user: GhUser,
    created_at: String,
    pull_request_url: String,
}

fn pull_request_info(
    pr: GhPullRequest,
    owner: &str,
    repository: &str,
    viewer_login: String,
) -> PullRequestInfo {
    PullRequestInfo {
        owner: owner.to_string(),
        repository: repository.to_string(),
        pull_number: pr.number,
        title: pr.title,
        author_login: pr.user.login,
        state: pr.state,
        draft: pr.draft,
        base_ref: pr.base.ref_name,
        base_sha: pr.base.sha,
        head_ref: pr.head.ref_name,
        head_sha: pr.head.sha,
        changed_files: pr.changed_files,
        additions: pr.additions,
        deletions: pr.deletions,
        html_url: pr.html_url,
        viewer_login,
    }
}

/// Rewrites a generic `PullRequestNotFound` (raised without a specific
/// reference by the shared status mapper) to carry the actual PR reference.
fn with_pr_reference(err: AppError, pr_ref: &PrRef) -> AppError {
    match err {
        AppError::PullRequestNotFound { .. } => AppError::PullRequestNotFound {
            reference: pr_ref.display(),
        },
        other => other,
    }
}

#[tauri::command]
pub async fn github_set_token(token: String) -> Result<String, AppError> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "Token must not be empty".to_string(),
        });
    }

    let client = GithubClient::new(trimmed.to_string());
    let user: GhUser = client.get_json("/user").await?;

    github::token::store_token(trimmed)?;
    Ok(user.login)
}

#[tauri::command]
pub async fn github_token_status() -> Result<Option<String>, AppError> {
    let Some(token) = github::token::load_token()? else {
        return Ok(None);
    };

    let client = GithubClient::new(token);
    match client.get_json::<GhUser>("/user").await {
        Ok(user) => Ok(Some(user.login)),
        Err(AppError::AuthenticationFailed) => Ok(None),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn github_clear_token() -> Result<(), AppError> {
    github::token::clear_token()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPrBundle {
    pub info: PullRequestInfo,
    pub patch: String,
}

#[tauri::command]
pub async fn github_open_pr(url: String) -> Result<GithubPrBundle, AppError> {
    let pr_ref = github::parse_pr_url(&url)?;
    let client = GithubClient::resolve().await?;

    let path = format!(
        "/repos/{}/{}/pulls/{}",
        pr_ref.owner, pr_ref.repository, pr_ref.pull_number
    );
    let pr: GhPullRequest = client
        .get_json(&path)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    let viewer: GhUser = client.get_json("/user").await?;
    let info = pull_request_info(pr, &pr_ref.owner, &pr_ref.repository, viewer.login);

    let patch = client.get_diff(&path).await?;

    Ok(GithubPrBundle { info, patch })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReviewArgs {
    pub owner: String,
    pub repository: String,
    pub pull_number: u64,
    pub expected_head_sha: String,
    pub event: String,
    pub body: String,
    pub comments: Vec<GithubReviewCommentInput>,
}

#[derive(Debug, Serialize)]
struct ReviewCommentPayload {
    path: String,
    body: String,
    line: u64,
    side: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_side: Option<String>,
}

impl From<GithubReviewCommentInput> for ReviewCommentPayload {
    fn from(c: GithubReviewCommentInput) -> Self {
        Self {
            path: c.path,
            body: c.body,
            line: c.line,
            side: c.side,
            start_line: c.start_line,
            start_side: c.start_side,
        }
    }
}

#[derive(Debug, Serialize)]
struct ReviewSubmission {
    commit_id: String,
    body: String,
    event: String,
    comments: Vec<ReviewCommentPayload>,
}

const VALID_REVIEW_EVENTS: [&str; 3] = ["COMMENT", "APPROVE", "REQUEST_CHANGES"];

#[tauri::command]
pub async fn github_submit_review(args: SubmitReviewArgs) -> Result<SubmittedReview, AppError> {
    github::validate_owner_repo(&args.owner, &args.repository)?;
    if !VALID_REVIEW_EVENTS.contains(&args.event.as_str()) {
        return Err(AppError::InvalidArgument {
            message: format!("Invalid review event: {}", args.event),
        });
    }

    let pr_ref = PrRef {
        owner: args.owner.clone(),
        repository: args.repository.clone(),
        pull_number: args.pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path = format!(
        "/repos/{}/{}/pulls/{}",
        args.owner, args.repository, args.pull_number
    );
    let pr: GhPullRequest = client
        .get_json(&path)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    if pr.head.sha != args.expected_head_sha {
        return Err(AppError::PullRequestRevisionChanged {
            expected: args.expected_head_sha,
            actual: pr.head.sha,
        });
    }

    let submission = ReviewSubmission {
        commit_id: args.expected_head_sha,
        body: args.body,
        event: args.event,
        comments: args.comments.into_iter().map(Into::into).collect(),
    };

    let reviews_path = format!(
        "/repos/{}/{}/pulls/{}/reviews",
        args.owner, args.repository, args.pull_number
    );
    let review: GhReview = client.post_json(&reviews_path, &submission).await?;

    Ok(SubmittedReview {
        review_id: review.id,
        html_url: review.html_url,
    })
}

/// Parses the trailing `.../pulls/{number}` segment off a GitHub
/// `pull_request_url` field.
fn pull_number_from_url(pull_request_url: &str) -> Option<u64> {
    pull_request_url.rsplit('/').next()?.parse().ok()
}

#[tauri::command]
pub async fn github_import_review_comments(
    owner: String,
    repository: String,
    page: u64,
) -> Result<ImportPage, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    if page < 1 {
        return Err(AppError::InvalidArgument {
            message: "page must be >= 1".to_string(),
        });
    }

    let client = GithubClient::resolve().await?;
    let viewer: GhUser = client.get_json("/user").await?;

    let path = format!(
        "/repos/{owner}/{repository}/pulls/comments?sort=created&direction=desc&per_page=100&page={page}"
    );
    let raw: Vec<GhReviewComment> = client.get_json(&path).await?;
    let has_more = raw.len() == 100;

    let comments = raw
        .into_iter()
        .filter(|c| c.user.login == viewer.login)
        .filter_map(|c| {
            let pull_number = pull_number_from_url(&c.pull_request_url)?;
            Some(ImportedGithubComment {
                id: c.id.to_string(),
                pull_number,
                path: c.path,
                body: c.body,
                diff_hunk: c.diff_hunk,
                original_line: c.line.or(c.original_line),
                side: c.side,
                author_login: c.user.login,
                commented_at: c.created_at,
            })
        })
        .collect();

    Ok(ImportPage { comments, has_more })
}

#[tauri::command]
pub async fn github_device_start(
    state: tauri::State<'_, DeviceFlowManager>,
    client_id: Option<String>,
) -> Result<DeviceFlowStart, AppError> {
    device::start(&state, client_id).await
}

#[tauri::command]
pub async fn github_device_wait(
    state: tauri::State<'_, DeviceFlowManager>,
) -> Result<String, AppError> {
    device::wait(&state).await
}

#[tauri::command]
pub async fn github_device_cancel(
    state: tauri::State<'_, DeviceFlowManager>,
) -> Result<(), AppError> {
    device::cancel(&state);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_comment_serializes_start_fields() {
        let input = GithubReviewCommentInput {
            path: "src/main.rs".to_string(),
            body: "nit".to_string(),
            line: 10,
            side: "RIGHT".to_string(),
            start_line: Some(5),
            start_side: Some("RIGHT".to_string()),
        };
        let payload: ReviewCommentPayload = input.into();
        let value = serde_json::to_value(&payload).unwrap();

        assert_eq!(value["start_line"], 5);
        assert_eq!(value["start_side"], "RIGHT");
    }

    #[test]
    fn single_line_comment_omits_start_fields() {
        let input = GithubReviewCommentInput {
            path: "src/main.rs".to_string(),
            body: "nit".to_string(),
            line: 10,
            side: "RIGHT".to_string(),
            start_line: None,
            start_side: None,
        };
        let payload: ReviewCommentPayload = input.into();
        let value = serde_json::to_value(&payload).unwrap();

        assert!(!value.as_object().unwrap().contains_key("start_line"));
        assert!(!value.as_object().unwrap().contains_key("start_side"));
    }

    #[test]
    fn pull_number_from_url_parses_trailing_segment() {
        assert_eq!(
            pull_number_from_url("https://api.github.com/repos/foo/bar/pulls/42"),
            Some(42)
        );
        assert_eq!(pull_number_from_url("not-a-url"), None);
    }
}
