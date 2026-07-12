//! OS-keychain storage for GitHub credentials. Credentials are never
//! persisted anywhere else (not SQLite, not logs).
use std::sync::OnceLock;

use keyring::Entry;

use crate::error::AppError;

const KEYRING_SERVICE: &str = "com.arsenstorm.lgtm";
const KEYRING_USER: &str = "github-token";

/// Everything the keychain entry may hold. Legacy entries are a bare PAT
/// string (no `refresh_token`/`expires_at`/`client_id`); device-flow entries
/// are this struct serialized as JSON.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCredentials {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// Unix seconds when the access token expires, if it expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

// Manual Debug: access_token/refresh_token are credentials and must never
// appear in a `{:?}` print (logs, panics, etc.).
impl std::fmt::Debug for StoredCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredCredentials")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("expires_at", &self.expires_at)
            .field("client_id", &self.client_id)
            .finish()
    }
}

// A single cached Entry, reused across calls. Real OS keychain backends look
// up by service+user regardless, but keyring's mock backend (used in tests)
// keeps its data on the Entry object itself rather than a shared store, so a
// fresh `Entry::new` per call would silently "forget" what was just stored.
fn entry() -> Result<&'static Entry, AppError> {
    static ENTRY: OnceLock<Entry> = OnceLock::new();
    if let Some(e) = ENTRY.get() {
        return Ok(e);
    }
    let created = Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| AppError::Internal {
        message: format!("failed to open keyring entry: {e}"),
    })?;
    Ok(ENTRY.get_or_init(|| created))
}

pub fn store_credentials(creds: &StoredCredentials) -> Result<(), AppError> {
    let serialized = serde_json::to_string(creds).map_err(|e| AppError::Internal {
        message: format!("failed to serialize credentials: {e}"),
    })?;
    entry()?
        .set_password(&serialized)
        .map_err(|e| AppError::Internal {
            message: format!("failed to store credentials in keyring: {e}"),
        })
}

pub fn load_credentials() -> Result<Option<StoredCredentials>, AppError> {
    let raw = match entry()?.get_password() {
        Ok(raw) => raw,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => {
            return Err(AppError::Internal {
                message: format!("failed to load credentials from keyring: {e}"),
            })
        }
    };

    if raw.starts_with('{') {
        if let Ok(parsed) = serde_json::from_str::<StoredCredentials>(&raw) {
            return Ok(Some(parsed));
        }
    }

    // Legacy entry: a bare personal access token string.
    Ok(Some(StoredCredentials {
        access_token: raw,
        refresh_token: None,
        expires_at: None,
        client_id: None,
    }))
}

pub fn store_token(token: &str) -> Result<(), AppError> {
    store_credentials(&StoredCredentials {
        access_token: token.to_string(),
        refresh_token: None,
        expires_at: None,
        client_id: None,
    })
}

pub fn load_token() -> Result<Option<String>, AppError> {
    load_credentials().map(|o| o.map(|c| c.access_token))
}

pub fn clear_token() -> Result<(), AppError> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Internal {
            message: format!("failed to clear token from keyring: {e}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    // All tests in this module share the single process-wide cached `Entry`
    // (see `entry()` above), which the mock keyring backend uses as its
    // actual storage. Run test bodies one at a time so they don't race.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn use_mock_keyring() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    }

    #[test]
    fn store_load_clear_roundtrip() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        // Other tests in this module share the same underlying mock entry;
        // start from a known-empty state regardless of run order.
        clear_token().unwrap();
        assert_eq!(load_token().unwrap(), None);

        store_token("ghp_example_token").unwrap();
        assert_eq!(load_token().unwrap(), Some("ghp_example_token".to_string()));

        clear_token().unwrap();
        assert_eq!(load_token().unwrap(), None);

        // Clearing an already-empty entry is a no-op, not an error.
        clear_token().unwrap();
    }

    #[test]
    fn store_credentials_roundtrips_all_fields() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        let creds = StoredCredentials {
            access_token: "gho_access".to_string(),
            refresh_token: Some("ghr_refresh".to_string()),
            expires_at: Some(1_700_000_000),
            client_id: Some("Iv1.abc123".to_string()),
        };

        store_credentials(&creds).unwrap();
        let loaded = load_credentials().unwrap().expect("credentials present");

        assert_eq!(loaded.access_token, "gho_access");
        assert_eq!(loaded.refresh_token, Some("ghr_refresh".to_string()));
        assert_eq!(loaded.expires_at, Some(1_700_000_000));
        assert_eq!(loaded.client_id, Some("Iv1.abc123".to_string()));
    }

    #[test]
    fn load_credentials_falls_back_to_legacy_pat() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        entry().unwrap().set_password("ghp_x").unwrap();

        let loaded = load_credentials().unwrap().expect("credentials present");
        assert_eq!(loaded.access_token, "ghp_x");
        assert_eq!(loaded.refresh_token, None);
        assert_eq!(loaded.expires_at, None);
        assert_eq!(loaded.client_id, None);
    }

    #[test]
    fn store_token_is_readable_by_load_credentials() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_mock_keyring();
        store_token("ghp_via_store_token").unwrap();

        let loaded = load_credentials().unwrap().expect("credentials present");
        assert_eq!(loaded.access_token, "ghp_via_store_token");
        assert_eq!(loaded.refresh_token, None);
        assert_eq!(loaded.expires_at, None);
        assert_eq!(loaded.client_id, None);
    }
}
