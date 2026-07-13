//! LGTM-owned bare "shadow" repositories used to fetch remote refs without
//! ever writing to the repository under review. A shadow lives in the app
//! data directory, shares the source repo's objects via alternates (zero
//! copy), and is a disposable cache: any structural failure deletes and
//! recreates it.
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::AppError;
use crate::git::exec::{run_git, run_git_ok, run_git_with_env, ExecLimits};

const FETCH_TIMEOUT: Duration = Duration::from_secs(120);

fn fnv64(input: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// "<sanitized-basename>-<fnv64 of the full path>" so shadows are stable per
/// repo and human-findable on disk.
fn shadow_dir_name(repo_root: &Path) -> String {
    let full = repo_root.display().to_string();
    let base: String = repo_root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("{base}-{:016x}", fnv64(&full))
}

async fn is_bare_repo(dir: &Path) -> bool {
    match run_git(dir, &["rev-parse", "--is-bare-repository"]).await {
        Ok(out) => out.ok() && out.stdout_text().trim() == "true",
        Err(_) => false,
    }
}

/// Ensures the shadow for `repo_root` exists under `shadow_base` with fresh
/// alternates and origin URL. Idempotent; broken shadows are recreated.
pub async fn ensure_shadow(
    shadow_base: &Path,
    repo_root: &Path,
    remote_url: &str,
) -> Result<PathBuf, AppError> {
    let shadow = shadow_base.join(shadow_dir_name(repo_root));

    if !is_bare_repo(&shadow).await {
        if shadow.exists() {
            std::fs::remove_dir_all(&shadow).map_err(io_error("remove broken shadow"))?;
        }
        std::fs::create_dir_all(&shadow).map_err(io_error("create shadow dir"))?;
        run_git_ok(&shadow, &["init", "--bare", "--quiet"]).await?;
    }

    // Alternates make every object of the source repo visible in the shadow
    // (zero-copy). Rewritten every time so a moved repo self-heals.
    let common_dir = run_git_ok(
        repo_root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .await?
    .stdout_text()
    .trim()
    .to_string();
    let objects = Path::new(&common_dir).join("objects");
    let info_dir = shadow.join("objects").join("info");
    std::fs::create_dir_all(&info_dir).map_err(io_error("create alternates dir"))?;
    std::fs::write(
        info_dir.join("alternates"),
        format!("{}\n", objects.display()),
    )
    .map_err(io_error("write alternates"))?;

    let current = run_git(&shadow, &["remote", "get-url", "origin"]).await?;
    if current.ok() {
        if current.stdout_text().trim() != remote_url {
            run_git_ok(&shadow, &["remote", "set-url", "origin", remote_url]).await?;
        }
    } else {
        run_git_ok(&shadow, &["remote", "add", "origin", remote_url]).await?;
    }

    Ok(shadow)
}

/// Fetches origin into the shadow. GitHub HTTPS remotes get the stored token
/// as a per-invocation env config header — never written to disk or argv.
pub async fn fetch_origin(shadow: &Path, remote_url: &str) -> Result<(), AppError> {
    let mut extra_env: Vec<(String, String)> = Vec::new();
    if remote_url.starts_with("https://github.com/") {
        if let Some(token) = crate::github::client::resolve_token_for_git().await {
            let header = format!(
                "AUTHORIZATION: basic {}",
                base64(format!("x-access-token:{token}").as_bytes())
            );
            extra_env.push(("GIT_CONFIG_COUNT".to_string(), "1".to_string()));
            extra_env.push((
                "GIT_CONFIG_KEY_0".to_string(),
                "http.https://github.com/.extraheader".to_string(),
            ));
            extra_env.push(("GIT_CONFIG_VALUE_0".to_string(), header));
        }
    }
    let limits = ExecLimits {
        timeout: FETCH_TIMEOUT,
        ..ExecLimits::default()
    };
    let out = run_git_with_env(
        shadow,
        &["fetch", "--prune", "--quiet", "origin"],
        &limits,
        &extra_env,
    )
    .await?;
    if out.ok() {
        Ok(())
    } else {
        Err(AppError::GitCommandFailed {
            command: "git fetch --prune --quiet origin".to_string(),
            status: out.status,
            stderr: out.stderr.chars().take(2000).collect(),
        })
    }
}

fn io_error(context: &'static str) -> impl Fn(std::io::Error) -> AppError {
    move |e| AppError::Internal {
        message: format!("{context}: {e}"),
    }
}

fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n as usize) & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
