use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::github::client::{map_status, GithubClient};
use crate::github::device::{self, DeviceFlowManager, DeviceFlowStart};
use crate::github::{
    self, CheckRunInfo, ConversationComment, GithubReviewCommentInput, ImportPage,
    ImportedGithubComment, MergeResult, PrCiStatus, PrInlineComment, PrRef, PullRequestInfo,
    PullRequestSummary, ReviewInfo, SubmittedReview,
};

// ---- Private GitHub API response shapes (only the fields we use). ----

#[derive(Debug, Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GhRepoRef {
    full_name: String,
}

#[derive(Debug, Deserialize)]
struct GhRef {
    #[serde(rename = "ref")]
    ref_name: String,
    sha: String,
    #[serde(default)]
    repo: Option<GhRepoRef>,
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
    #[serde(default)]
    mergeable: Option<bool>,
    #[serde(default)]
    mergeable_state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhShortRef {
    #[serde(rename = "ref")]
    ref_name: String,
}

#[derive(Debug, Deserialize)]
struct GhPullSummary {
    number: u64,
    title: String,
    user: GhUser,
    base: GhShortRef,
    head: GhShortRef,
    #[serde(default)]
    draft: bool,
    updated_at: String,
    html_url: String,
    state: String,
    #[serde(default)]
    merged_at: Option<String>,
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

/// Repo-level endpoints 404 both when the repository doesn't exist and when
/// the token can't see it (e.g. a private repo without the GitHub App
/// installed), so the PR-level "not found" would mislead — remap to a
/// repository-scoped error.
fn with_repo_reference(err: AppError, reference: &str) -> AppError {
    match err {
        AppError::PullRequestNotFound { .. } => AppError::RepositoryNotAccessible {
            reference: reference.to_string(),
        },
        other => other,
    }
}

fn pull_request_summary(pr: GhPullSummary) -> PullRequestSummary {
    PullRequestSummary {
        number: pr.number,
        title: pr.title,
        author_login: pr.user.login,
        base_ref: pr.base.ref_name,
        head_ref: pr.head.ref_name,
        draft: pr.draft,
        updated_at: pr.updated_at,
        html_url: pr.html_url,
        state: pr.state,
        merged: pr.merged_at.is_some(),
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
pub async fn github_list_pull_requests(
    owner: String,
    repository: String,
) -> Result<Vec<PullRequestSummary>, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let client = GithubClient::resolve().await?;

    let path = format!(
        "/repos/{owner}/{repository}/pulls?state=all&sort=updated&direction=desc&per_page=100"
    );
    let reference = format!("{owner}/{repository}");
    let prs: Vec<GhPullSummary> = client
        .get_json(&path)
        .await
        .map_err(|e| with_repo_reference(e, &reference))?;

    Ok(prs.into_iter().map(pull_request_summary).collect())
}

const VALID_MERGE_METHODS: [&str; 3] = ["merge", "squash", "rebase"];
const VALID_PR_STATES: [&str; 2] = ["open", "closed"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePrArgs {
    pub owner: String,
    pub repository: String,
    pub pull_number: u64,
    pub expected_head_sha: String,
    pub method: String,
    pub commit_title: Option<String>,
    pub commit_message: Option<String>,
    pub delete_branch: bool,
}

#[derive(Debug, Serialize)]
struct MergePayload<'a> {
    merge_method: &'a str,
    sha: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_title: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    commit_message: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct GhMergeResponse {
    merged: bool,
    sha: Option<String>,
    message: String,
}

/// Extracts a human-readable message from a GitHub error body: `{"message":
/// "..."}` when present, otherwise the trimmed body itself (capped so a huge
/// non-JSON body can't blow up an error string).
pub(crate) fn extract_github_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct GhErrorBody {
        message: Option<String>,
    }

    if let Ok(parsed) = serde_json::from_str::<GhErrorBody>(body) {
        if let Some(message) = parsed.message.filter(|m| !m.trim().is_empty()) {
            return message.trim().chars().take(300).collect();
        }
    }
    body.trim().chars().take(300).collect()
}

/// A git ref destined for a URL path segment: only characters GitHub allows
/// in a ref, no leading `-` (could be parsed as a flag by some tooling), and
/// no `..` (path traversal). Mirrors `is_valid_owner_or_repo`'s intent for a
/// different character set.
pub(crate) fn safe_ref_for_deletion(ref_name: &str) -> bool {
    !ref_name.is_empty()
        && !ref_name.starts_with('-')
        && !ref_name.contains("..")
        && ref_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

#[tauri::command]
pub async fn github_merge_pr(args: MergePrArgs) -> Result<MergeResult, AppError> {
    github::validate_owner_repo(&args.owner, &args.repository)?;
    if !VALID_MERGE_METHODS.contains(&args.method.as_str()) {
        return Err(AppError::InvalidArgument {
            message: format!("Invalid merge method: {}", args.method),
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

    if pr.state != "open" {
        return Err(AppError::InvalidArgument {
            message: "Pull request is not open".to_string(),
        });
    }
    if pr.draft {
        return Err(AppError::InvalidArgument {
            message: "Draft pull requests cannot be merged".to_string(),
        });
    }
    if pr.head.sha != args.expected_head_sha {
        return Err(AppError::PullRequestRevisionChanged {
            expected: args.expected_head_sha,
            actual: pr.head.sha,
        });
    }

    let payload = MergePayload {
        merge_method: &args.method,
        sha: &args.expected_head_sha,
        commit_title: args.commit_title.as_deref(),
        commit_message: args.commit_message.as_deref(),
    };

    let merge_path = format!(
        "/repos/{}/{}/pulls/{}/merge",
        args.owner, args.repository, args.pull_number
    );
    let (status, body) = client.put_json_raw(&merge_path, &payload).await?;

    let mut result = match status {
        200..=299 => {
            let parsed: GhMergeResponse =
                serde_json::from_str(&body).map_err(|e| AppError::Internal {
                    message: format!("failed to parse GitHub merge response: {e}"),
                })?;
            MergeResult {
                merged: parsed.merged,
                sha: parsed.sha,
                message: parsed.message,
                branch_deleted: false,
            }
        }
        405 => {
            return Err(AppError::MergeBlocked {
                message: extract_github_message(&body),
            });
        }
        409 => {
            return Err(AppError::PullRequestRevisionChanged {
                expected: args.expected_head_sha.clone(),
                actual: "changed on GitHub".to_string(),
            });
        }
        401 | 403 | 404 | 429 => {
            let reference = pr_ref.display();
            return Err(map_status(status, None, None, Some(&reference)).unwrap_or(
                AppError::NetworkFailed {
                    message: format!("GitHub returned status {status}"),
                },
            ));
        }
        _ => {
            let trimmed: String = body.trim().chars().take(300).collect();
            return Err(AppError::NetworkFailed {
                message: format!("GitHub returned status {status}: {trimmed}"),
            });
        }
    };

    let same_repo_head = matches!(
        (&pr.head.repo, &pr.base.repo),
        (Some(head), Some(base)) if head.full_name == base.full_name
    );

    if result.merged && args.delete_branch && same_repo_head {
        if safe_ref_for_deletion(&pr.head.ref_name) {
            let ref_path = format!(
                "/repos/{}/{}/git/refs/heads/{}",
                args.owner, args.repository, pr.head.ref_name
            );
            match client.delete(&ref_path).await {
                Ok(()) => result.branch_deleted = true,
                Err(e) => {
                    result.branch_deleted = false;
                    result.message = format!("{}. Branch deletion failed: {e}", result.message);
                }
            }
        } else {
            result.message = format!(
                "{}. Branch deletion skipped: unsafe branch name",
                result.message
            );
        }
    }

    Ok(result)
}

#[derive(Debug, Serialize)]
struct SetStatePayload<'a> {
    state: &'a str,
}

#[derive(Debug, Deserialize)]
struct GhStateResponse {
    state: String,
}

#[tauri::command]
pub async fn github_set_pr_state(
    owner: String,
    repository: String,
    pull_number: u64,
    state: String,
) -> Result<String, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    if !VALID_PR_STATES.contains(&state.as_str()) {
        return Err(AppError::InvalidArgument {
            message: format!("Invalid pull request state: {state}"),
        });
    }

    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path = format!("/repos/{owner}/{repository}/pulls/{pull_number}");
    let payload = SetStatePayload { state: &state };
    let resp: GhStateResponse = client
        .patch_json(&path, &payload)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    Ok(resp.state)
}

#[derive(Debug, Deserialize)]
struct GhReviewFull {
    id: u64,
    user: GhUser,
    state: String,
    #[serde(default)]
    body: Option<String>,
    submitted_at: Option<String>,
    html_url: String,
}

fn review_info(r: GhReviewFull) -> ReviewInfo {
    ReviewInfo {
        id: r.id,
        author_login: r.user.login,
        state: r.state,
        body: r.body.unwrap_or_default(),
        submitted_at: r.submitted_at,
        html_url: r.html_url,
    }
}

#[tauri::command]
pub async fn github_list_reviews(
    owner: String,
    repository: String,
    pull_number: u64,
) -> Result<Vec<ReviewInfo>, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path = format!("/repos/{owner}/{repository}/pulls/{pull_number}/reviews?per_page=100");
    let reviews: Vec<GhReviewFull> = client
        .get_json(&path)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    Ok(reviews.into_iter().map(review_info).collect())
}

#[derive(Debug, Serialize)]
struct DismissReviewPayload<'a> {
    message: &'a str,
}

#[derive(Debug, Deserialize)]
struct GhReviewAck {
    #[allow(dead_code)]
    id: u64,
}

#[tauri::command]
pub async fn github_dismiss_review(
    owner: String,
    repository: String,
    pull_number: u64,
    review_id: u64,
    message: String,
) -> Result<(), AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "A dismissal message is required".to_string(),
        });
    }

    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path =
        format!("/repos/{owner}/{repository}/pulls/{pull_number}/reviews/{review_id}/dismissals");
    let payload = DismissReviewPayload { message: trimmed };
    let _ack: GhReviewAck = client
        .put_json(&path, &payload)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GhInlineComment {
    id: u64,
    user: GhUser,
    path: String,
    #[serde(default)]
    line: Option<u64>,
    #[serde(default)]
    original_line: Option<u64>,
    #[serde(default)]
    side: Option<String>,
    body: String,
    created_at: String,
    html_url: String,
    #[serde(default)]
    in_reply_to_id: Option<u64>,
}

fn pr_inline_comment(c: GhInlineComment) -> PrInlineComment {
    PrInlineComment {
        id: c.id,
        author_login: c.user.login,
        path: c.path,
        line: c.line,
        original_line: c.original_line,
        side: c.side,
        body: c.body,
        created_at: c.created_at,
        html_url: c.html_url,
        in_reply_to_id: c.in_reply_to_id,
    }
}

#[tauri::command]
pub async fn github_list_pr_comments(
    owner: String,
    repository: String,
    pull_number: u64,
) -> Result<Vec<PrInlineComment>, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path = format!("/repos/{owner}/{repository}/pulls/{pull_number}/comments?per_page=100");
    let raw: Vec<GhInlineComment> = client
        .get_json(&path)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    Ok(raw.into_iter().map(pr_inline_comment).collect())
}

#[tauri::command]
pub async fn github_delete_review_comment(
    owner: String,
    repository: String,
    comment_id: u64,
) -> Result<(), AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let client = GithubClient::resolve().await?;

    let path = format!("/repos/{owner}/{repository}/pulls/comments/{comment_id}");
    client.delete(&path).await
}

#[derive(Debug, Deserialize)]
struct GhIssueComment {
    id: u64,
    user: GhUser,
    body: String,
    created_at: String,
    html_url: String,
}

fn conversation_comment(c: GhIssueComment) -> ConversationComment {
    ConversationComment {
        id: c.id,
        author_login: c.user.login,
        body: c.body,
        created_at: c.created_at,
        html_url: c.html_url,
    }
}

#[derive(Debug, Serialize)]
struct ConversationCommentPayload<'a> {
    body: &'a str,
}

#[tauri::command]
pub async fn github_add_conversation_comment(
    owner: String,
    repository: String,
    pull_number: u64,
    body: String,
) -> Result<ConversationComment, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidArgument {
            message: "Comment body must not be empty".to_string(),
        });
    }

    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path = format!("/repos/{owner}/{repository}/issues/{pull_number}/comments");
    let payload = ConversationCommentPayload { body: trimmed };
    let comment: GhIssueComment = client
        .post_json(&path, &payload)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    Ok(conversation_comment(comment))
}

#[tauri::command]
pub async fn github_list_conversation_comments(
    owner: String,
    repository: String,
    pull_number: u64,
) -> Result<Vec<ConversationComment>, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let path = format!("/repos/{owner}/{repository}/issues/{pull_number}/comments?per_page=100");
    let raw: Vec<GhIssueComment> = client
        .get_json(&path)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    Ok(raw.into_iter().map(conversation_comment).collect())
}

#[derive(Debug, Deserialize)]
struct GhCheckRun {
    name: String,
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    details_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhCheckRunsResponse {
    #[serde(default)]
    check_runs: Vec<GhCheckRun>,
}

#[derive(Debug, Deserialize)]
struct GhCombinedStatus {
    state: String,
}

#[tauri::command]
pub async fn github_pr_ci_status(
    owner: String,
    repository: String,
    pull_number: u64,
) -> Result<PrCiStatus, AppError> {
    github::validate_owner_repo(&owner, &repository)?;
    let pr_ref = PrRef {
        owner: owner.clone(),
        repository: repository.clone(),
        pull_number,
    };
    let client = GithubClient::resolve().await?;

    let pr_path = format!("/repos/{owner}/{repository}/pulls/{pull_number}");
    let pr: GhPullRequest = client
        .get_json(&pr_path)
        .await
        .map_err(|e| with_pr_reference(e, &pr_ref))?;

    let check_runs_path = format!(
        "/repos/{owner}/{repository}/commits/{}/check-runs?per_page=100",
        pr.head.sha
    );
    let check_runs = match client
        .get_json::<GhCheckRunsResponse>(&check_runs_path)
        .await
    {
        Ok(resp) => resp
            .check_runs
            .into_iter()
            .map(|c| CheckRunInfo {
                name: c.name,
                status: c.status,
                conclusion: c.conclusion,
                details_url: c.details_url,
            })
            .collect(),
        Err(AppError::GithubPermissionDenied { .. }) => Vec::new(),
        Err(e) => return Err(e),
    };

    let status_path = format!("/repos/{owner}/{repository}/commits/{}/status", pr.head.sha);
    let commit_state = match client.get_json::<GhCombinedStatus>(&status_path).await {
        Ok(resp) => resp.state,
        Err(AppError::GithubPermissionDenied { .. }) => "unknown".to_string(),
        Err(e) => return Err(e),
    };

    Ok(PrCiStatus {
        check_runs,
        commit_state,
        mergeable: pr.mergeable,
        mergeable_state: pr.mergeable_state,
        head_sha: pr.head.sha,
    })
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

    #[test]
    fn pull_summaries_map_and_default_missing_draft_to_false() {
        let json = r#"[
            {
                "number": 42,
                "title": "Add feature",
                "user": {"login": "octocat"},
                "base": {"ref": "main"},
                "head": {"ref": "feature-branch"},
                "draft": true,
                "updated_at": "2024-01-01T00:00:00Z",
                "html_url": "https://github.com/foo/bar/pull/42",
                "state": "closed",
                "merged_at": "2026-07-01T00:00:00Z"
            },
            {
                "number": 7,
                "title": "Fix bug",
                "user": {"login": "someone"},
                "base": {"ref": "main"},
                "head": {"ref": "fix-branch"},
                "updated_at": "2024-02-02T00:00:00Z",
                "html_url": "https://github.com/foo/bar/pull/7",
                "state": "open"
            }
        ]"#;

        let parsed: Vec<GhPullSummary> = serde_json::from_str(json).unwrap();
        let summaries: Vec<PullRequestSummary> =
            parsed.into_iter().map(pull_request_summary).collect();

        assert_eq!(summaries.len(), 2);

        assert_eq!(summaries[0].number, 42);
        assert_eq!(summaries[0].title, "Add feature");
        assert_eq!(summaries[0].author_login, "octocat");
        assert_eq!(summaries[0].base_ref, "main");
        assert_eq!(summaries[0].head_ref, "feature-branch");
        assert!(summaries[0].draft);
        assert_eq!(summaries[0].updated_at, "2024-01-01T00:00:00Z");
        assert_eq!(summaries[0].html_url, "https://github.com/foo/bar/pull/42");
        assert_eq!(summaries[0].state, "closed");
        assert!(summaries[0].merged);

        assert_eq!(summaries[1].number, 7);
        assert!(!summaries[1].draft);
        assert_eq!(summaries[1].state, "open");
        assert!(!summaries[1].merged);
    }

    #[test]
    fn extract_github_message_reads_json_message_field() {
        let body = r#"{"message": "Pull Request is not mergeable"}"#;
        assert_eq!(
            extract_github_message(body),
            "Pull Request is not mergeable"
        );
    }

    #[test]
    fn extract_github_message_falls_back_to_trimmed_body_on_invalid_json() {
        assert_eq!(
            extract_github_message("  not json at all  "),
            "not json at all"
        );
    }

    #[test]
    fn extract_github_message_caps_long_fallback_body_at_300_chars() {
        let long_body = "x".repeat(500);
        let extracted = extract_github_message(&long_body);
        assert_eq!(extracted.chars().count(), 300);
    }

    #[test]
    fn merge_payload_includes_snake_case_optional_fields_when_present() {
        let payload = MergePayload {
            merge_method: "squash",
            sha: "abc123",
            commit_title: Some("Squash it"),
            commit_message: None,
        };
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["merge_method"], "squash");
        assert_eq!(value["sha"], "abc123");
        assert_eq!(value["commit_title"], "Squash it");
        assert!(!value.as_object().unwrap().contains_key("commit_message"));
    }

    #[test]
    fn merge_payload_omits_optional_fields_when_absent() {
        let payload = MergePayload {
            merge_method: "merge",
            sha: "abc123",
            commit_title: None,
            commit_message: None,
        };
        let value = serde_json::to_value(&payload).unwrap();
        assert!(!value.as_object().unwrap().contains_key("commit_title"));
        assert!(!value.as_object().unwrap().contains_key("commit_message"));
    }

    #[test]
    fn merge_method_allowlist_rejects_fast_forward() {
        assert!(!VALID_MERGE_METHODS.contains(&"fast-forward"));
        for good in ["merge", "squash", "rebase"] {
            assert!(VALID_MERGE_METHODS.contains(&good));
        }
    }

    #[test]
    fn pr_state_allowlist_accepts_open_and_closed_only() {
        assert!(VALID_PR_STATES.contains(&"open"));
        assert!(VALID_PR_STATES.contains(&"closed"));
        assert!(!VALID_PR_STATES.contains(&"merged"));
        assert!(!VALID_PR_STATES.contains(&""));
    }

    #[test]
    fn review_info_maps_and_defaults_missing_body_to_empty_string() {
        let json = r#"{
            "id": 99,
            "user": {"login": "reviewer"},
            "state": "APPROVED",
            "submitted_at": "2024-01-01T00:00:00Z",
            "html_url": "https://github.com/foo/bar/pull/1#pullrequestreview-99"
        }"#;
        let parsed: GhReviewFull = serde_json::from_str(json).unwrap();
        let info = review_info(parsed);
        assert_eq!(info.id, 99);
        assert_eq!(info.author_login, "reviewer");
        assert_eq!(info.state, "APPROVED");
        assert_eq!(info.body, "");
        assert_eq!(info.submitted_at.as_deref(), Some("2024-01-01T00:00:00Z"));
    }

    #[test]
    fn pr_inline_comment_maps_missing_line_and_side_to_none() {
        let json = r#"{
            "id": 5,
            "user": {"login": "octocat"},
            "path": "src/main.rs",
            "body": "nit",
            "created_at": "2024-01-01T00:00:00Z",
            "html_url": "https://github.com/foo/bar/pull/1#discussion_r5"
        }"#;
        let parsed: GhInlineComment = serde_json::from_str(json).unwrap();
        let comment = pr_inline_comment(parsed);
        assert_eq!(comment.id, 5);
        assert!(comment.line.is_none());
        assert!(comment.original_line.is_none());
        assert!(comment.side.is_none());
        assert!(comment.in_reply_to_id.is_none());
    }

    #[test]
    fn conversation_comment_maps_all_fields() {
        let json = r#"{
            "id": 7,
            "user": {"login": "octocat"},
            "body": "Looks good",
            "created_at": "2024-01-01T00:00:00Z",
            "html_url": "https://github.com/foo/bar/pull/1#issuecomment-7"
        }"#;
        let parsed: GhIssueComment = serde_json::from_str(json).unwrap();
        let comment = conversation_comment(parsed);
        assert_eq!(comment.id, 7);
        assert_eq!(comment.author_login, "octocat");
        assert_eq!(comment.body, "Looks good");
    }

    #[test]
    fn safe_ref_for_deletion_accepts_typical_branch_names() {
        for good in ["feature/x-1", "main", "release-1.2.3", "a_b"] {
            assert!(safe_ref_for_deletion(good), "expected {good:?} to be safe");
        }
    }

    #[test]
    fn safe_ref_for_deletion_rejects_unsafe_names() {
        for bad in ["-x", "a..b", "a b", "a?b", ""] {
            assert!(!safe_ref_for_deletion(bad), "expected {bad:?} to be unsafe");
        }
    }
}
