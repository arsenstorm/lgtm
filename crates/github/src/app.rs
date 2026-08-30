//! GitHub App authentication: a JWT signed with the app's private key,
//! exchanged for a token that can push to one repository and nothing else.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context};
use serde::Deserialize;

use crate::{GitHub, Repo};

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// GitHub expires an installation token after an hour. Ten minutes of slack
/// leaves a push that picked one up time to finish with it.
const TTL: Duration = Duration::from_secs(50 * 60);

pub struct GithubApp {
    id: String,
    /// The key stays a path rather than its bytes: signing shells out to
    /// `openssl`, which reads the PEM itself, so the private key never has to
    /// sit in this process or in a temporary file of our own.
    key: PathBuf,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

#[derive(Deserialize)]
struct Installation {
    id: u64,
}

#[derive(Deserialize)]
struct AccessToken {
    token: String,
}

impl GithubApp {
    /// `LGTM_GITHUB_APP_ID` and `LGTM_GITHUB_APP_KEY`, the path to the app's
    /// private key PEM. `None` unless both are set.
    pub fn from_env() -> Option<Self> {
        let id = std::env::var("LGTM_GITHUB_APP_ID").ok()?;
        let key = std::env::var("LGTM_GITHUB_APP_KEY").ok()?;
        // The id is interpolated into the JWT claims, so anything but digits
        // would either break the JSON or forge a claim.
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) || key.is_empty() {
            return None;
        }
        Some(GithubApp {
            id,
            key: PathBuf::from(key),
            cache: Mutex::new(HashMap::new()),
        })
    }
}

impl GitHub {
    pub fn with_app(mut self, app: Option<GithubApp>) -> Self {
        self.app = app.map(Arc::new);
        self
    }

    pub fn has_app(&self) -> bool {
        self.app.is_some()
    }

    /// The token last fetched for `repo`, while it is still fresh. Sync so a
    /// caller holding a lock can take a token without waiting on GitHub.
    pub fn cached_installation_token(&self, repo: &Repo) -> Option<String> {
        let app = self.app.as_ref()?;
        fresh(
            app.cache.lock().unwrap().get(&cache_key(repo)),
            Instant::now(),
        )
    }

    /// A token scoped to `repo`'s contents, from the cache or from GitHub.
    pub async fn installation_token(&self, repo: &Repo) -> anyhow::Result<String> {
        if let Some(token) = self.cached_installation_token(repo) {
            return Ok(token);
        }
        let app = self.app.clone().context("no github app configured")?;
        let jwt = jwt(&app, now_secs()?)?;
        let path = format!("/repos/{}/{}/installation", repo.owner, repo.repo);
        let installation: Installation =
            Self::send_json(self.request_as(reqwest::Method::GET, &path, &jwt)).await?;
        let path = format!("/app/installations/{}/access_tokens", installation.id);
        let req = self
            .request_as(reqwest::Method::POST, &path, &jwt)
            .json(&serde_json::json!({
                "repositories": [repo.repo],
                "permissions": {"contents": "write"},
            }));
        let issued: AccessToken = Self::send_json(req).await?;
        app.cache
            .lock()
            .unwrap()
            .insert(cache_key(repo), (issued.token.clone(), Instant::now()));
        Ok(issued.token)
    }
}

fn cache_key(repo: &Repo) -> String {
    format!("{}/{}", repo.owner, repo.repo)
}

fn fresh(entry: Option<&(String, Instant)>, now: Instant) -> Option<String> {
    let (token, issued) = entry?;
    (now.duration_since(*issued) < TTL).then(|| token.clone())
}

fn now_secs() -> anyhow::Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

/// The app's JWT, expiring inside the ten minutes GitHub allows.
pub fn jwt(app: &GithubApp, now: u64) -> anyhow::Result<String> {
    let header = base64url(br#"{"alg":"RS256","typ":"JWT"}"#);
    // Backdating `iat` covers a clock a little ahead of GitHub's, which would
    // otherwise reject the token as issued in the future.
    let claims = format!(
        r#"{{"iat":{},"exp":{},"iss":"{}"}}"#,
        now.saturating_sub(60),
        now + 540,
        app.id
    );
    let signed = format!("{header}.{}", base64url(claims.as_bytes()));
    let signature = sign(&app.key, signed.as_bytes())?;
    Ok(format!("{signed}.{}", base64url(&signature)))
}

/// RS256 over `message`. `ring` reaches the build only through rustls, so
/// naming it here would mean a new dependency; `openssl` is already what the
/// runner docs have people install.
fn sign(key: &Path, message: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut child = Command::new("openssl")
        .args(["dgst", "-sha256", "-sign"])
        .arg(key)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running openssl to sign the github app jwt")?;
    child
        .stdin
        .take()
        .context("openssl stdin")?
        .write_all(message)?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!("signing the github app jwt: {}", stderr.trim()));
    }
    Ok(out.stdout)
}

/// base64url without padding, as JWT wants it.
fn base64url(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut buf = [0u8; 3];
        buf[..chunk.len()].copy_from_slice(chunk);
        let n = u32::from_be_bytes([0, buf[0], buf[1], buf[2]]);
        for i in 0..chunk.len() + 1 {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 0x3f) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base64url_decode(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in s.as_bytes().chunks(4) {
            let mut n = 0u32;
            for (i, c) in chunk.iter().enumerate() {
                let value = ALPHABET.iter().position(|a| a == c).expect("alphabet") as u32;
                n |= value << (18 - 6 * i);
            }
            for i in 0..chunk.len() - 1 {
                out.push((n >> (16 - 8 * i)) as u8);
            }
        }
        out
    }

    fn openssl_missing() -> bool {
        Command::new("openssl")
            .arg("version")
            .output()
            .map(|out| !out.status.success())
            .unwrap_or(true)
    }

    #[test]
    fn base64url_drops_padding() {
        assert_eq!(base64url(b"Man"), "TWFu");
        assert_eq!(base64url(b"Ma"), "TWE");
        assert_eq!(base64url(b"M"), "TQ");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }

    #[test]
    fn base64url_round_trips() {
        for len in 0..32 {
            let bytes: Vec<u8> = (0..len).map(|i| i as u8 * 7).collect();
            assert_eq!(base64url_decode(&base64url(&bytes)), bytes);
        }
    }

    #[test]
    fn fresh_returns_a_young_token_and_drops_an_old_one() {
        let issued = Instant::now();
        let entry = Some(("t".to_string(), issued));
        assert_eq!(fresh(entry.as_ref(), issued), Some("t".to_string()));
        assert_eq!(
            fresh(entry.as_ref(), issued + TTL - Duration::from_secs(1)),
            Some("t".to_string())
        );
        assert_eq!(fresh(entry.as_ref(), issued + TTL), None);
        assert_eq!(fresh(None, issued), None);
    }

    #[test]
    fn jwt_signs_header_and_claims() {
        if openssl_missing() {
            eprintln!("skipped: openssl is not on PATH");
            return;
        }
        let key = std::env::temp_dir().join(format!("lgtm-jwt-{}.pem", std::process::id()));
        let generated = Command::new("openssl")
            .args(["genpkey", "-algorithm", "RSA", "-pkeyopt"])
            .arg("rsa_keygen_bits:2048")
            .arg("-out")
            .arg(&key)
            .output()
            .expect("openssl genpkey");
        assert!(generated.status.success(), "openssl genpkey failed");

        let app = GithubApp {
            id: "1234".to_string(),
            key: key.clone(),
            cache: Mutex::new(HashMap::new()),
        };
        let token = jwt(&app, 1_000_000).expect("jwt");
        let _ = std::fs::remove_file(&key);

        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            base64url_decode(parts[0]),
            br#"{"alg":"RS256","typ":"JWT"}"#
        );
        assert_eq!(
            String::from_utf8(base64url_decode(parts[1])).unwrap(),
            r#"{"iat":999940,"exp":1000540,"iss":"1234"}"#
        );
        assert_eq!(base64url_decode(parts[2]).len(), 256);
    }
}
