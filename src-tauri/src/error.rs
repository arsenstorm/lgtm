use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Repository path not found")]
    RepositoryNotFound { path: String },
    #[error("Not a Git repository")]
    NotAGitRepository { path: String },
    #[error("Git executable not found")]
    GitUnavailable,
    #[error("Git command failed")]
    GitCommandFailed {
        command: String,
        status: Option<i32>,
        stderr: String,
    },
    #[error("Git command timed out")]
    GitTimeout { command: String },
    #[error("Diff output exceeded the size limit")]
    DiffTooLarge,
    #[error("Invalid argument")]
    InvalidArgument { message: String },
    #[error("Internal error")]
    Internal { message: String },
    #[error("GitHub authentication failed")]
    AuthenticationFailed,
    #[error("GitHub API rate limit exceeded")]
    GithubRateLimited { reset: Option<String> },
    #[error("GitHub permission denied")]
    GithubPermissionDenied { detail: Option<String> },
    #[error("Pull request not found")]
    PullRequestNotFound { reference: String },
    #[error("Repository not found or not accessible with your GitHub access")]
    RepositoryNotAccessible { reference: String },
    #[error("Pull request revision changed")]
    PullRequestRevisionChanged { expected: String, actual: String },
    #[error("Network request to GitHub failed")]
    NetworkFailed { message: String },
    #[error("GitHub refused the merge")]
    MergeBlocked { message: String },
}

impl AppError {
    fn code(&self) -> &'static str {
        match self {
            AppError::RepositoryNotFound { .. } => "repository-not-found",
            AppError::NotAGitRepository { .. } => "not-a-git-repository",
            AppError::GitUnavailable => "git-unavailable",
            AppError::GitCommandFailed { .. } => "git-command-failed",
            AppError::GitTimeout { .. } => "git-timeout",
            AppError::DiffTooLarge => "diff-too-large",
            AppError::InvalidArgument { .. } => "invalid-argument",
            AppError::Internal { .. } => "internal",
            AppError::AuthenticationFailed => "authentication-failed",
            AppError::GithubRateLimited { .. } => "github-rate-limited",
            AppError::GithubPermissionDenied { .. } => "github-permission-denied",
            AppError::PullRequestNotFound { .. } => "pull-request-not-found",
            AppError::RepositoryNotAccessible { .. } => "repository-not-accessible",
            AppError::PullRequestRevisionChanged { .. } => "pull-request-revision-changed",
            AppError::NetworkFailed { .. } => "network-failed",
            AppError::MergeBlocked { .. } => "merge-blocked",
        }
    }

    fn details(&self) -> Option<String> {
        match self {
            AppError::RepositoryNotFound { path } | AppError::NotAGitRepository { path } => {
                Some(path.clone())
            }
            AppError::GitCommandFailed {
                command,
                status,
                stderr,
            } => {
                let status_text = status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                let trimmed: String = stderr.trim().chars().take(2000).collect();
                Some(format!("{command} (exit status: {status_text}): {trimmed}"))
            }
            AppError::GitTimeout { command } => Some(command.clone()),
            AppError::InvalidArgument { message } | AppError::Internal { message } => {
                Some(message.clone())
            }
            AppError::GitUnavailable | AppError::DiffTooLarge => None,
            AppError::AuthenticationFailed => None,
            AppError::GithubPermissionDenied { detail } => detail.clone(),
            AppError::GithubRateLimited { reset } => reset.clone(),
            AppError::PullRequestNotFound { reference }
            | AppError::RepositoryNotAccessible { reference } => Some(reference.clone()),
            AppError::PullRequestRevisionChanged { expected, actual } => {
                Some(format!("expected {expected}, found {actual}"))
            }
            AppError::NetworkFailed { message } => Some(message.clone()),
            AppError::MergeBlocked { message } => Some(message.clone()),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let details = self.details();
        let field_count = if details.is_some() { 3 } else { 2 };
        let mut state = serializer.serialize_struct("AppError", field_count)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        if let Some(details) = details {
            state.serialize_field("details", &details)?;
        }
        state.end()
    }
}
