//! Where the data directory, the shared token, and this machine's
//! reachable address come from.

use std::net::{Ipv4Addr, UdpSocket};
use std::path::{Path, PathBuf};

/// `--data-dir`, else `LGTM_DATA_DIR`, else `~/.lgtm`.
pub fn data_dir(flag: Option<PathBuf>) -> PathBuf {
    if let Some(dir) = flag {
        return dir;
    }
    if let Ok(dir) = std::env::var("LGTM_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .expect("HOME or USERPROFILE must be set");
    PathBuf::from(home).join(".lgtm")
}

pub fn stored_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("token")
}

/// `--token`, else `LGTM_TOKEN`, else the token `lgtm serve` saved on this
/// machine. Blank values count as absent.
pub fn resolve_token(flag: Option<String>, data_dir: &Path) -> Option<String> {
    let from_env = || std::env::var("LGTM_TOKEN").ok();
    let from_file = || std::fs::read_to_string(stored_token_path(data_dir)).ok();
    flag.or_else(from_env)
        .or_else(from_file)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Writes the token where `resolve_token` will find it, owner-readable only.
pub fn store_token(data_dir: &Path, token: &str) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let path = stored_token_path(data_dir);
    std::fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// 32 lowercase hex characters, from a v4 UUID's randomness.
pub fn generate_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Address another machine can dial this orchestrator at: the Tailscale IP
/// when there is one, else this host's LAN address, else loopback.
pub fn advertised_ip() -> String {
    tailscale_ip()
        .or_else(|| first_non_loopback_ipv4().map(|ip| ip.to_string()))
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn tailscale_ip() -> Option<String> {
    let out = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line.lines().next()?.trim();
    first.parse::<Ipv4Addr>().ok().map(|ip| ip.to_string())
}

/// The local address the OS would use to reach the outside world. No packet
/// is sent: a connected UDP socket only fixes the route.
pub fn first_non_loopback_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("10.255.255.255:1").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// The two lines `lgtm serve` prints once it is up. `scheme` is the HTTP
/// scheme; the join command's ws/wss scheme follows from it.
pub fn join_line(scheme: &str, ip: &str, port: u16, token: &str) -> String {
    let ws = if scheme == "https" { "wss" } else { "ws" };
    format!(
        "lgtm is listening on {scheme}://127.0.0.1:{port}\n\
         join another machine:  lgtm worker {ws}://{ip}:{port} --token {token}"
    )
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
    fn join_line_reads_as_two_copyable_lines() {
        assert_eq!(
            join_line("http", "100.64.0.1", 4750, "abc"),
            "lgtm is listening on http://127.0.0.1:4750\n\
             join another machine:  lgtm worker ws://100.64.0.1:4750 --token abc"
        );
    }

    #[test]
    fn join_line_uses_wss_under_tls() {
        assert!(join_line("https", "100.64.0.1", 4750, "abc")
            .contains("lgtm worker wss://100.64.0.1:4750 --token abc"));
    }

    #[test]
    fn first_non_loopback_ipv4_finds_this_machine() {
        assert!(first_non_loopback_ipv4().is_some());
    }

    /// A unique empty directory under the test binary's temp dir. `LGTM_TOKEN`
    /// is cleared here because `resolve_token` reads it and tests share a
    /// process; no test in this file sets it.
    fn tempdir() -> PathBuf {
        // SAFETY-adjacent: single-threaded intent, see the doc comment.
        std::env::remove_var("LGTM_TOKEN");
        let dir = std::env::temp_dir().join(format!("lgtm-cli-test-{}", generate_token()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
