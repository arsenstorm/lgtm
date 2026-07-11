//! OS-keychain storage for the GitHub personal access token. The token is
//! never persisted anywhere else (not SQLite, not logs).
use std::sync::OnceLock;

use keyring::Entry;

use crate::error::AppError;

const KEYRING_SERVICE: &str = "com.arsenstorm.lgtm";
const KEYRING_USER: &str = "github-token";

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

pub fn store_token(token: &str) -> Result<(), AppError> {
    entry()?
        .set_password(token)
        .map_err(|e| AppError::Internal {
            message: format!("failed to store token in keyring: {e}"),
        })
}

pub fn load_token() -> Result<Option<String>, AppError> {
    match entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::Internal {
            message: format!("failed to load token from keyring: {e}"),
        }),
    }
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
    use super::*;

    fn use_mock_keyring() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    }

    #[test]
    fn store_load_clear_roundtrip() {
        use_mock_keyring();
        assert_eq!(load_token().unwrap(), None);

        store_token("ghp_example_token").unwrap();
        assert_eq!(load_token().unwrap(), Some("ghp_example_token".to_string()));

        clear_token().unwrap();
        assert_eq!(load_token().unwrap(), None);

        // Clearing an already-empty entry is a no-op, not an error.
        clear_token().unwrap();
    }
}
