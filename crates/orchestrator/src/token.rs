//! Where the shared token comes from: a flag, the environment, a file this
//! machine already wrote, or a freshly generated one — and where the data
//! directory that file lives in comes from. Shared by `lgtm serve` and
//! anything else that wants to stand up an orchestrator on this machine.

use std::path::{Path, PathBuf};

/// `--data-dir`, else `LGTM_DATA_DIR`, else `~/.lgtm`.
pub fn data_dir(flag: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = flag {
        return dir;
    }
    if let Ok(dir) = std::env::var("LGTM_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .expect("home directory must be resolvable")
        .join(".lgtm")
}

pub fn stored_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("token")
}

/// Where `lgtm login` saves the minted per-user token. A separate file from
/// the shared token: `serve` reads that one back as the orchestrator secret
/// and `runner` presents it to the runner WebSocket, so login must never
/// overwrite it.
pub fn stored_user_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("user-token")
}

fn present(value: Option<String>) -> Option<String> {
    value
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// `--token`, else `LGTM_TOKEN`, else the token `lgtm serve` saved on this
/// machine. Blank values count as absent. For `serve` and `runner`, which
/// must use the shared token; person-facing commands go through
/// [`resolve_client_token`].
pub fn resolve_token(flag: Option<String>, data_dir: &Path) -> Option<String> {
    present(flag)
        .or_else(|| present(std::env::var("LGTM_TOKEN").ok()))
        .or_else(|| present(std::fs::read_to_string(stored_token_path(data_dir)).ok()))
}

/// [`resolve_token`], except the token `lgtm login` saved outranks the
/// shared one, so a person who logged in acts as themselves.
pub fn resolve_client_token(flag: Option<String>, data_dir: &Path) -> Option<String> {
    present(flag)
        .or_else(|| present(std::env::var("LGTM_TOKEN").ok()))
        .or_else(|| present(std::fs::read_to_string(stored_user_token_path(data_dir)).ok()))
        .or_else(|| present(std::fs::read_to_string(stored_token_path(data_dir)).ok()))
}

/// Writes the token where `resolve_token` will find it, owner-readable only.
pub fn store_token(data_dir: &Path, token: &str) -> anyhow::Result<()> {
    write_secret(data_dir, &stored_token_path(data_dir), token)
}

/// Writes the token where `resolve_client_token` will find it first.
pub fn store_user_token(data_dir: &Path, token: &str) -> anyhow::Result<()> {
    write_secret(data_dir, &stored_user_token_path(data_dir), token)
}

fn write_secret(data_dir: &Path, path: &Path, token: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// 32 lowercase hex characters, from a v4 UUID's randomness.
pub fn generate_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Where the token `resolve_or_create` returned came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenSource {
    Flag,
    Env,
    Stored,
    Generated,
}

/// `--token`, else `LGTM_TOKEN`, else the stored token, else a freshly
/// generated one — which this also saves to `data_dir`, the way `lgtm serve`
/// mints one rather than demanding it like every other subcommand.
pub fn resolve_or_create(
    flag: Option<String>,
    data_dir: &Path,
) -> anyhow::Result<(String, TokenSource)> {
    if let Some(t) = present(flag) {
        return Ok((t, TokenSource::Flag));
    }
    if let Some(t) = present(std::env::var("LGTM_TOKEN").ok()) {
        return Ok((t, TokenSource::Env));
    }
    if let Some(t) = present(std::fs::read_to_string(stored_token_path(data_dir)).ok()) {
        return Ok((t, TokenSource::Stored));
    }
    let t = generate_token();
    store_token(data_dir, &t)?;
    Ok((t, TokenSource::Generated))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_token_prefers_the_flag_over_the_file() {
        let dir = tempdir();
        store_token(&dir, "from-file").unwrap();
        assert_eq!(
            resolve_token(Some("from-flag".into()), &dir),
            Some("from-flag".into())
        );
    }

    #[test]
    fn store_token_round_trips_through_resolve_token() {
        let dir = tempdir();
        store_token(&dir, "stored-secret").unwrap();
        assert_eq!(resolve_token(None, &dir), Some("stored-secret".into()));
    }

    #[test]
    fn resolve_token_ignores_a_blank_stored_token() {
        let dir = tempdir();
        store_token(&dir, "   \n").unwrap();
        assert_eq!(resolve_token(None, &dir), None);
    }

    #[test]
    fn resolve_token_is_none_without_a_flag_or_a_file() {
        let dir = tempdir();
        assert_eq!(resolve_token(None, &dir), None);
    }

    #[test]
    fn generate_token_is_32_lowercase_hex() {
        let token = generate_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(token, token.to_lowercase());
    }

    #[test]
    fn a_login_token_outranks_the_shared_file_for_clients_only() {
        let dir = tempdir();
        store_token(&dir, "shared").unwrap();
        store_user_token(&dir, "mine").unwrap();
        assert_eq!(resolve_client_token(None, &dir), Some("mine".into()));
        assert_eq!(resolve_token(None, &dir), Some("shared".into()));
    }

    #[test]
    fn resolve_client_token_falls_back_to_the_shared_file() {
        let dir = tempdir();
        store_token(&dir, "shared").unwrap();
        assert_eq!(resolve_client_token(None, &dir), Some("shared".into()));
    }

    #[test]
    fn resolve_or_create_prefers_the_flag() {
        let dir = tempdir();
        let (token, source) = resolve_or_create(Some("from-flag".into()), &dir).unwrap();
        assert_eq!(token, "from-flag");
        assert_eq!(source, TokenSource::Flag);
    }

    #[test]
    fn resolve_or_create_finds_a_stored_token() {
        let dir = tempdir();
        store_token(&dir, "stored-secret").unwrap();
        let (token, source) = resolve_or_create(None, &dir).unwrap();
        assert_eq!(token, "stored-secret");
        assert_eq!(source, TokenSource::Stored);
    }

    #[test]
    fn resolve_or_create_generates_and_saves_a_token_when_nothing_else_provides_one() {
        let dir = tempdir();
        let (token, source) = resolve_or_create(None, &dir).unwrap();
        assert_eq!(source, TokenSource::Generated);
        assert_eq!(resolve_token(None, &dir), Some(token));
        assert!(stored_token_path(&dir).exists());
    }

    /// A unique empty directory under the test binary's temp dir. `LGTM_TOKEN`
    /// is cleared here because `resolve_token` reads it and tests share a
    /// process; no test in this file sets it.
    fn tempdir() -> PathBuf {
        std::env::remove_var("LGTM_TOKEN");
        let dir = std::env::temp_dir().join(format!("lgtm-orchestrator-test-{}", generate_token()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
