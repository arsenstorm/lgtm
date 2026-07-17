//! GitHub OAuth device flow. The device_code never leaves this module's
//! state; the frontend only ever sees the user_code and verification URI.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::github::client::{http_client, GithubClient};
use crate::github::token::{self, StoredCredentials};

pub const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
pub const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// Baked-in GitHub App client ID. Empty until a public app is registered;
/// users can supply their own via settings, passed per-call.
pub const DEFAULT_GITHUB_CLIENT_ID: &str = "Iv23liij7Uf2YDj2jm28";
const POLL_TICK: Duration = Duration::from_secs(1);
const SLOW_DOWN_BUMP_SECS: u64 = 5;
const MAX_CLIENT_ID_LEN: usize = 100;

/// One in-flight (or completed) device authorization. `device_code` is a
/// credential: this struct deliberately does not derive `Debug` so it can
/// never end up in a log or panic message.
pub struct DeviceFlow {
    device_code: String,
    client_id: String,
    interval_secs: AtomicU64,
    deadline: Instant,
    cancelled: AtomicBool,
}

#[derive(Default)]
pub struct DeviceFlowManager(pub Mutex<Option<Arc<DeviceFlow>>>);

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFlowStart {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Shape of GitHub's response body for both the device-code request and the
/// polling/refresh token exchange. `TokenGrant` is `pub(crate)` so
/// `client.rs`'s token-refresh path can reuse the same wire format.
#[derive(serde::Deserialize)]
pub(crate) struct TokenGrant {
    pub(crate) access_token: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_in: Option<i64>,
    pub(crate) error: Option<String>,
}

#[derive(serde::Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// Small private mirror of `commands::github::GhUser` — kept private and
/// duplicated rather than exported from there, since that struct isn't part
/// of this module's public surface.
#[derive(serde::Deserialize)]
struct GhUser {
    login: String,
}

enum PollOutcome {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Success(TokenGrant),
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolves the client ID to use: a trimmed non-empty override, else the
/// baked-in default (if non-empty), else an error telling the user to
/// configure one. Either way the result is validated against
/// `[A-Za-z0-9._-]{1,100}` before it's allowed anywhere near a URL.
// The emptiness guard is deliberate: it keeps the error path correct if the
// baked-in default is ever removed again.
#[allow(clippy::const_is_empty)]
pub fn resolve_client_id(override_id: Option<&str>) -> Result<String, AppError> {
    let trimmed_override = override_id.map(str::trim).filter(|s| !s.is_empty());

    let candidate = match trimmed_override {
        Some(id) => id.to_string(),
        None if !DEFAULT_GITHUB_CLIENT_ID.is_empty() => DEFAULT_GITHUB_CLIENT_ID.to_string(),
        None => {
            return Err(AppError::InvalidArgument {
                message: "No GitHub App client ID is configured. Add one in Settings or connect with a personal access token.".to_string(),
            })
        }
    };

    let valid = !candidate.is_empty()
        && candidate.len() <= MAX_CLIENT_ID_LEN
        && candidate
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(AppError::InvalidArgument {
            message: "Invalid GitHub App client ID".to_string(),
        });
    }

    Ok(candidate)
}

pub async fn start(
    manager: &DeviceFlowManager,
    client_id_override: Option<String>,
) -> Result<DeviceFlowStart, AppError> {
    let client_id = resolve_client_id(client_id_override.as_deref())?;

    let resp = http_client()
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .form(&[("client_id", client_id.as_str())])
        .send()
        .await
        .map_err(|e| AppError::NetworkFailed {
            message: e.to_string(),
        })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(AppError::NetworkFailed {
            message: format!("GitHub returned status {status} starting device authorization"),
        });
    }

    let body: DeviceCodeResponse = resp.json().await.map_err(|e| AppError::NetworkFailed {
        message: e.to_string(),
    })?;

    {
        let mut slot = manager.0.lock().map_err(|_| AppError::Internal {
            message: "device flow state poisoned".into(),
        })?;
        // A stale in-flight `wait` (if any) must stop polling for the flow
        // we're about to replace.
        if let Some(previous) = slot.as_ref() {
            previous.cancelled.store(true, Ordering::Relaxed);
        }
        *slot = Some(Arc::new(DeviceFlow {
            device_code: body.device_code,
            client_id: client_id.clone(),
            interval_secs: AtomicU64::new(body.interval.max(1)),
            deadline: Instant::now() + Duration::from_secs(body.expires_in),
            cancelled: AtomicBool::new(false),
        }));
    }

    Ok(DeviceFlowStart {
        user_code: body.user_code,
        verification_uri: body.verification_uri,
        expires_in: body.expires_in,
        interval: body.interval,
    })
}

pub async fn wait(manager: &DeviceFlowManager) -> Result<String, AppError> {
    let flow = manager
        .0
        .lock()
        .map_err(|_| AppError::Internal {
            message: "device flow state poisoned".into(),
        })?
        .clone()
        .ok_or(AppError::InvalidArgument {
            message: "No device authorization in progress".into(),
        })?;

    loop {
        // Sleep one interval in 1s ticks so cancellation is responsive.
        let interval = Duration::from_secs(flow.interval_secs.load(Ordering::Relaxed));
        let sleep_until = Instant::now() + interval;
        while Instant::now() < sleep_until {
            if flow.cancelled.load(Ordering::Relaxed) {
                return Err(AppError::InvalidArgument {
                    message: "Authorization was cancelled".into(),
                });
            }
            tokio::time::sleep(POLL_TICK.min(sleep_until - Instant::now())).await;
        }
        if flow.cancelled.load(Ordering::Relaxed) {
            return Err(AppError::InvalidArgument {
                message: "Authorization was cancelled".into(),
            });
        }
        if Instant::now() > flow.deadline {
            return Err(AppError::InvalidArgument {
                message:
                    "The device code expired before authorization completed. Try connecting again."
                        .into(),
            });
        }

        match poll_once(&flow.client_id, &flow.device_code).await? {
            PollOutcome::Pending => {}
            PollOutcome::SlowDown => {
                flow.interval_secs
                    .fetch_add(SLOW_DOWN_BUMP_SECS, Ordering::Relaxed);
            }
            PollOutcome::Denied => {
                clear_slot(manager);
                return Err(AppError::InvalidArgument {
                    message: "Authorization was declined on GitHub.".into(),
                });
            }
            PollOutcome::Expired => {
                clear_slot(manager);
                return Err(AppError::InvalidArgument {
                    message: "The device code expired before authorization completed. Try connecting again.".into(),
                });
            }
            PollOutcome::Success(grant) => {
                clear_slot(manager);
                return finish(grant, &flow.client_id).await;
            }
        }
    }
}

pub fn cancel(manager: &DeviceFlowManager) {
    if let Ok(slot) = manager.0.lock() {
        if let Some(flow) = slot.as_ref() {
            flow.cancelled.store(true, Ordering::Relaxed);
        }
    }
}

async fn poll_once(client_id: &str, device_code: &str) -> Result<PollOutcome, AppError> {
    let resp = http_client()
        .post(ACCESS_TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| AppError::NetworkFailed {
            message: e.to_string(),
        })?;

    // GitHub returns 200 with an `error` field for pending/denied/expired
    // states, so the grant is parsed regardless of HTTP status.
    let grant: TokenGrant = resp.json().await.map_err(|e| AppError::NetworkFailed {
        message: e.to_string(),
    })?;

    map_grant(grant)
}

fn map_grant(grant: TokenGrant) -> Result<PollOutcome, AppError> {
    if let Some(error) = grant.error {
        return match error.as_str() {
            "authorization_pending" => Ok(PollOutcome::Pending),
            "slow_down" => Ok(PollOutcome::SlowDown),
            "access_denied" => Ok(PollOutcome::Denied),
            "expired_token" => Ok(PollOutcome::Expired),
            other => Err(AppError::NetworkFailed {
                message: format!("GitHub device flow error: {other}"),
            }),
        };
    }

    match grant.access_token {
        Some(_) => Ok(PollOutcome::Success(grant)),
        None => Err(AppError::NetworkFailed {
            message: "GitHub device flow returned no token".into(),
        }),
    }
}

async fn finish(grant: TokenGrant, client_id: &str) -> Result<String, AppError> {
    // Guaranteed Some by map_grant's Success branch; handled defensively
    // rather than unwrapped.
    let access_token = grant.access_token.ok_or_else(|| AppError::Internal {
        message: "device flow reported success without an access token".into(),
    })?;
    let expires_at = grant.expires_in.map(|secs| now_unix() + secs);
    let creds = StoredCredentials {
        access_token: access_token.clone(),
        refresh_token: grant.refresh_token,
        expires_at,
        client_id: Some(client_id.to_string()),
    };

    // Validate the token (and learn the login) before storing anything.
    let client = GithubClient::new(access_token);
    let user: GhUser = client.get_json("/user").await?;

    token::store_credentials(&creds)?;
    Ok(user.login)
}

fn clear_slot(manager: &DeviceFlowManager) {
    if let Ok(mut slot) = manager.0.lock() {
        *slot = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(error: Option<&str>, access_token: Option<&str>) -> TokenGrant {
        TokenGrant {
            access_token: access_token.map(str::to_string),
            refresh_token: None,
            expires_in: None,
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn map_grant_pending() {
        assert!(matches!(
            map_grant(grant(Some("authorization_pending"), None)),
            Ok(PollOutcome::Pending)
        ));
    }

    #[test]
    fn map_grant_slow_down() {
        assert!(matches!(
            map_grant(grant(Some("slow_down"), None)),
            Ok(PollOutcome::SlowDown)
        ));
    }

    #[test]
    fn map_grant_access_denied() {
        assert!(matches!(
            map_grant(grant(Some("access_denied"), None)),
            Ok(PollOutcome::Denied)
        ));
    }

    #[test]
    fn map_grant_expired_token() {
        assert!(matches!(
            map_grant(grant(Some("expired_token"), None)),
            Ok(PollOutcome::Expired)
        ));
    }

    #[test]
    fn map_grant_unknown_error_is_network_failed() {
        assert!(matches!(
            map_grant(grant(Some("some_new_error"), None)),
            Err(AppError::NetworkFailed { .. })
        ));
    }

    #[test]
    fn map_grant_success() {
        let outcome = map_grant(grant(None, Some("gho_token"))).unwrap();
        assert!(matches!(
            outcome,
            PollOutcome::Success(TokenGrant {
                access_token: Some(ref t),
                ..
            }) if t == "gho_token"
        ));
    }

    #[test]
    fn map_grant_missing_token_is_network_failed() {
        assert!(matches!(
            map_grant(grant(None, None)),
            Err(AppError::NetworkFailed { .. })
        ));
    }

    #[test]
    fn resolve_client_id_override_wins() {
        let id = resolve_client_id(Some(" Iv1.abc123 ")).unwrap();
        assert_eq!(id, "Iv1.abc123");
    }

    #[test]
    fn resolve_client_id_falls_back_to_default() {
        assert_eq!(resolve_client_id(None).unwrap(), DEFAULT_GITHUB_CLIENT_ID);
        assert_eq!(
            resolve_client_id(Some("   ")).unwrap(),
            DEFAULT_GITHUB_CLIENT_ID
        );
    }

    #[test]
    fn resolve_client_id_rejects_bad_charset() {
        assert!(resolve_client_id(Some("has a space")).is_err());
        assert!(resolve_client_id(Some("has/slash")).is_err());
    }

    #[test]
    fn resolve_client_id_rejects_too_long() {
        let too_long = "a".repeat(101);
        assert!(resolve_client_id(Some(&too_long)).is_err());
        let just_right = "a".repeat(100);
        assert!(resolve_client_id(Some(&just_right)).is_ok());
    }

    #[test]
    fn cancel_flips_the_flag_on_the_active_flow() {
        let manager = DeviceFlowManager::default();
        let flow = Arc::new(DeviceFlow {
            device_code: "dc".to_string(),
            client_id: "id".to_string(),
            interval_secs: AtomicU64::new(5),
            deadline: Instant::now() + Duration::from_secs(60),
            cancelled: AtomicBool::new(false),
        });
        *manager.0.lock().unwrap() = Some(flow.clone());

        cancel(&manager);

        assert!(flow.cancelled.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn wait_on_empty_manager_errors_immediately() {
        let manager = DeviceFlowManager::default();
        let err = wait(&manager).await.unwrap_err();
        match err {
            AppError::InvalidArgument { message } => {
                assert_eq!(message, "No device authorization in progress");
            }
            other => panic!("expected InvalidArgument, got {other:?}"),
        }
    }
}
