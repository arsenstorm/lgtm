//! Thin wrapper around a single lazily-built `reqwest::Client`. All GitHub
//! HTTP calls go through here so the auth headers, timeout, size cap, and
//! status-code mapping are applied uniformly.
use std::sync::OnceLock;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::AppError;
use crate::github::token;

const BASE_URL: &str = "https://api.github.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
const USER_AGENT: &str = "lgtm-desktop";

/// Wraps the GitHub token so it can never leak through a `{:?}` print.
pub struct RedactedToken(pub String);

impl std::fmt::Debug for RedactedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("failed to build reqwest client")
    })
}

pub struct GithubClient {
    token: RedactedToken,
}

impl std::fmt::Debug for GithubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GithubClient")
            .field("token", &self.token)
            .finish()
    }
}

impl GithubClient {
    pub fn new(token: String) -> Self {
        Self {
            token: RedactedToken(token),
        }
    }

    pub fn from_keyring() -> Result<Self, AppError> {
        let token = token::load_token()?.ok_or(AppError::AuthenticationFailed)?;
        Ok(Self::new(token))
    }

    fn request(&self, method: reqwest::Method, path_and_query: &str) -> reqwest::RequestBuilder {
        let url = format!("{BASE_URL}{path_and_query}");
        http_client()
            .request(method, url)
            .header("Authorization", format!("Bearer {}", self.token.0))
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", USER_AGENT)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path_and_query: &str) -> Result<T, AppError> {
        let resp = self
            .request(reqwest::Method::GET, path_and_query)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(network_error)?;
        let bytes = read_body_checked(resp, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| AppError::Internal {
            message: format!("failed to parse GitHub response: {e}"),
        })
    }

    pub async fn get_diff(&self, path: &str) -> Result<String, AppError> {
        let resp = self
            .request(reqwest::Method::GET, path)
            .header("Accept", "application/vnd.github.diff")
            .send()
            .await
            .map_err(network_error)?;
        let bytes = read_body_checked(resp, None).await?;
        String::from_utf8(bytes).map_err(|e| AppError::Internal {
            message: format!("GitHub diff response was not valid UTF-8: {e}"),
        })
    }

    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, AppError> {
        let resp = self
            .request(reqwest::Method::POST, path)
            .header("Accept", "application/vnd.github+json")
            .json(body)
            .send()
            .await
            .map_err(network_error)?;
        let bytes = read_body_checked(resp, None).await?;
        serde_json::from_slice(&bytes).map_err(|e| AppError::Internal {
            message: format!("failed to parse GitHub response: {e}"),
        })
    }
}

fn network_error(e: reqwest::Error) -> AppError {
    AppError::NetworkFailed {
        message: e.to_string(),
    }
}

/// Reads the response body, enforcing the size cap, then applies status-code
/// mapping via `map_status`. Body is read first (bounded by Content-Length or
/// the streamed byte count) so error bodies can be included in NetworkFailed.
async fn read_body_checked(
    resp: reqwest::Response,
    reference: Option<&str>,
) -> Result<Vec<u8>, AppError> {
    if let Some(len) = resp.content_length() {
        if len > MAX_BODY_BYTES {
            return Err(AppError::DiffTooLarge);
        }
    }

    let status = resp.status();
    let ratelimit_remaining = resp
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let ratelimit_reset = resp
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // ponytail: reqwest doesn't make it easy to cap a streamed body without
    // pulling in extra stream-adapter plumbing; buffering fully and checking
    // the length is the documented fallback. Content-Length check above
    // already rejects the common case before we get here.
    let bytes = resp.bytes().await.map_err(network_error)?;
    if bytes.len() as u64 > MAX_BODY_BYTES {
        return Err(AppError::DiffTooLarge);
    }

    if let Some(err) = map_status(
        status.as_u16(),
        ratelimit_remaining.as_deref(),
        ratelimit_reset.as_deref(),
        reference,
    ) {
        // The pure mapping fn (kept network/body-free for unit testing) uses a
        // generic message for the fallback NetworkFailed case; enrich it with
        // the trimmed response body here, where we actually have it.
        if let AppError::NetworkFailed { .. } = &err {
            let body_text = String::from_utf8_lossy(&bytes);
            let trimmed: String = body_text.trim().chars().take(500).collect();
            return Err(AppError::NetworkFailed {
                message: format!("GitHub returned status {status}: {trimmed}"),
            });
        }
        return Err(err);
    }

    Ok(bytes.to_vec())
}

/// Pure status-code -> AppError mapping, kept separate from I/O so it can be
/// unit tested without a live network call.
pub fn map_status(
    status: u16,
    ratelimit_remaining: Option<&str>,
    ratelimit_reset: Option<&str>,
    reference: Option<&str>,
) -> Option<AppError> {
    if (200..300).contains(&status) {
        return None;
    }

    match status {
        401 => Some(AppError::AuthenticationFailed),
        403 if ratelimit_remaining == Some("0") => Some(AppError::GithubRateLimited {
            reset: ratelimit_reset.map(str::to_string),
        }),
        429 => Some(AppError::GithubRateLimited {
            reset: ratelimit_reset.map(str::to_string),
        }),
        403 => Some(AppError::GithubPermissionDenied),
        404 => Some(AppError::PullRequestNotFound {
            reference: reference.unwrap_or("unknown").to_string(),
        }),
        _ => Some(AppError::NetworkFailed {
            message: format!("GitHub returned status {status}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_401_to_authentication_failed() {
        assert!(matches!(
            map_status(401, None, None, None),
            Some(AppError::AuthenticationFailed)
        ));
    }

    #[test]
    fn maps_403_with_exhausted_ratelimit_to_rate_limited() {
        let err = map_status(403, Some("0"), Some("1699999999"), None);
        assert!(matches!(
            err,
            Some(AppError::GithubRateLimited { reset: Some(ref r) }) if r == "1699999999"
        ));
    }

    #[test]
    fn maps_403_without_exhausted_ratelimit_to_permission_denied() {
        assert!(matches!(
            map_status(403, Some("42"), None, None),
            Some(AppError::GithubPermissionDenied)
        ));
        assert!(matches!(
            map_status(403, None, None, None),
            Some(AppError::GithubPermissionDenied)
        ));
    }

    #[test]
    fn maps_404_to_pull_request_not_found_with_reference() {
        let err = map_status(404, None, None, Some("acme/widgets#12"));
        assert!(matches!(
            err,
            Some(AppError::PullRequestNotFound { ref reference }) if reference == "acme/widgets#12"
        ));
    }

    #[test]
    fn maps_429_to_rate_limited() {
        let err = map_status(429, None, Some("123"), None);
        assert!(matches!(
            err,
            Some(AppError::GithubRateLimited { reset: Some(ref r) }) if r == "123"
        ));
    }

    #[test]
    fn maps_500_to_network_failed() {
        assert!(matches!(
            map_status(500, None, None, None),
            Some(AppError::NetworkFailed { .. })
        ));
    }

    #[test]
    fn maps_2xx_to_none() {
        assert!(map_status(200, None, None, None).is_none());
        assert!(map_status(204, None, None, None).is_none());
    }
}
