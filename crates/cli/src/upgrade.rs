//! `lgtm upgrade`: replace this binary with the latest (or a pinned) GitHub
//! release, verified against the release's `SHA256SUMS`.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPO: &str = "arsenstorm/lgtm";
const USER_AGENT: &str = "lgtm";

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

/// Base of the GitHub releases API, overridable with `LGTM_RELEASE_API` so
/// tests don't hit the network.
fn api_base() -> String {
    std::env::var("LGTM_RELEASE_API")
        .unwrap_or_else(|_| format!("https://api.github.com/repos/{REPO}"))
}

/// This machine's release target triple, or `None` on a platform lgtm
/// doesn't publish a build for.
pub fn target() -> Option<&'static str> {
    use std::env::consts::{ARCH, OS};
    match (OS, ARCH) {
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("linux", arch) if arch.contains("arm") => Some("armv7-unknown-linux-gnueabihf"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

/// The release asset name for a target: a `.zip` on Windows, `.tar.gz`
/// everywhere else.
pub fn asset_name(target: &str) -> String {
    if target.contains("windows") {
        format!("lgtm-{target}.zip")
    } else {
        format!("lgtm-{target}.tar.gz")
    }
}

/// Looks up `<hex>  <file>` (or `<hex> *<file>`, sha256sum's binary-mode
/// marker) in a `SHA256SUMS` file's contents.
pub fn expected_sha(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hex = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == file).then(|| hex.to_lowercase())
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The sibling `lgtm.exe.old` a Windows upgrade leaves behind; a running
/// binary can't delete itself on Windows, so the *next* run cleans it up.
/// Best-effort: called at every start of `main`.
pub fn cleanup_old_binary() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(name) = exe.file_name() else {
        return;
    };
    let old = exe.with_file_name(format!("{}.old", name.to_string_lossy()));
    let _ = std::fs::remove_file(old);
}

pub async fn run(version: Option<String>) -> anyhow::Result<i32> {
    let target = target().ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported platform: {}/{}; download from https://github.com/{REPO}/releases",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;

    let base = api_base();
    let url = match &version {
        Some(tag) => format!("{base}/releases/tags/{tag}"),
        None => format!("{base}/releases/latest"),
    };
    let http = reqwest::Client::new();
    let response = http
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(match &version {
            Some(tag) => format!("no release named {tag}"),
            // `latest` skips pre-releases, so this is the state before v0.1.0.
            None => "no stable release yet; pass --version <tag> for a pre-release".to_string(),
        });
    }
    let release: Release = response.error_for_status()?.json().await?;

    let latest = release.tag_name.trim_start_matches('v');
    let current = env!("CARGO_PKG_VERSION");
    if version.is_none() && latest == current {
        println!("already up to date (v{current})");
        return Ok(0);
    }

    let asset = asset_name(target);
    let asset_url = release
        .assets
        .iter()
        .find(|a| a.name == asset)
        .map(|a| a.browser_download_url.clone())
        .ok_or_else(|| anyhow::anyhow!("release {} has no asset for {target}", release.tag_name))?;
    let sums_url = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .map(|a| a.browser_download_url.clone())
        .ok_or_else(|| anyhow::anyhow!("release {} is missing SHA256SUMS", release.tag_name))?;

    let asset_bytes = download(&http, &asset_url).await?;
    let sums_text = String::from_utf8(download(&http, &sums_url).await?)?;

    let expected = expected_sha(&sums_text, &asset)
        .ok_or_else(|| anyhow::anyhow!("SHA256SUMS has no entry for {asset}"))?;
    let actual = sha256_hex(&asset_bytes);
    if actual != expected {
        anyhow::bail!("checksum mismatch for {asset}: expected {expected}, got {actual}");
    }

    let work_dir = std::env::temp_dir().join(format!("lgtm-upgrade-{}", std::process::id()));
    std::fs::create_dir_all(&work_dir)?;
    let archive_path = work_dir.join(&asset);
    std::fs::write(&archive_path, &asset_bytes)?;

    let extract_dir = work_dir.join("extracted");
    std::fs::create_dir_all(&extract_dir)?;
    extract(&archive_path, &extract_dir, &asset)?;

    let binary_name = if target.contains("windows") {
        "lgtm.exe"
    } else {
        "lgtm"
    };
    let new_binary = find_binary(&extract_dir, binary_name)
        .ok_or_else(|| anyhow::anyhow!("{binary_name} not found in {asset}"))?;

    let current_exe = std::env::current_exe()?;
    replace_binary(&current_exe, &new_binary)?;
    let _ = std::fs::remove_dir_all(&work_dir);

    println!("lgtm v{current} → v{latest}");
    Ok(0)
}

async fn download(http: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = http
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await?
        .error_for_status()?;
    Ok(resp.bytes().await?.to_vec())
}

/// Extracts `archive` (a `.tar.gz` or, on Windows, a `.zip`) into `dest` by
/// shelling out to `tar`/`Expand-Archive` rather than adding a
/// flate2+tar/zip dependency neither already in the build.
fn extract(archive: &Path, dest: &Path, asset_name: &str) -> anyhow::Result<()> {
    let status = if asset_name.ends_with(".zip") {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    dest.display()
                ),
            ])
            .status()?
    } else {
        Command::new("tar")
            .args([
                "-xzf",
                &archive.to_string_lossy(),
                "-C",
                &dest.to_string_lossy(),
            ])
            .status()?
    };
    if !status.success() {
        anyhow::bail!("extracting {} failed", archive.display());
    }
    Ok(())
}

/// Depth-first search for a file named `name` under `dir`, since a release
/// archive's internal layout isn't guaranteed to put the binary at the top.
fn find_binary(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut subdirs = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path.file_name().is_some_and(|f| f == name) {
            return Some(path);
        }
    }
    subdirs.into_iter().find_map(|d| find_binary(&d, name))
}

#[cfg(unix)]
fn replace_binary(current: &Path, new_binary: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let staged = current.with_extension("new");
    std::fs::copy(new_binary, &staged)?;
    std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staged, current)?;
    Ok(())
}

#[cfg(windows)]
fn replace_binary(current: &Path, new_binary: &Path) -> anyhow::Result<()> {
    let name = current
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("current exe has no file name"))?;
    let old = current.with_file_name(format!("{}.old", name.to_string_lossy()));
    let _ = std::fs::remove_file(&old);
    std::fs::rename(current, &old)?;
    std::fs::copy(new_binary, current)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_known_on_this_machine() {
        assert!(target().is_some());
    }

    #[test]
    fn asset_name_covers_every_published_target() {
        let cases = [
            ("aarch64-apple-darwin", "lgtm-aarch64-apple-darwin.tar.gz"),
            ("x86_64-apple-darwin", "lgtm-x86_64-apple-darwin.tar.gz"),
            (
                "x86_64-unknown-linux-gnu",
                "lgtm-x86_64-unknown-linux-gnu.tar.gz",
            ),
            (
                "aarch64-unknown-linux-gnu",
                "lgtm-aarch64-unknown-linux-gnu.tar.gz",
            ),
            (
                "armv7-unknown-linux-gnueabihf",
                "lgtm-armv7-unknown-linux-gnueabihf.tar.gz",
            ),
            ("x86_64-pc-windows-msvc", "lgtm-x86_64-pc-windows-msvc.zip"),
        ];
        for (target, expected) in cases {
            assert_eq!(asset_name(target), expected);
        }
    }

    #[test]
    fn expected_sha_finds_the_matching_line() {
        let sums = "\
deadbeef00000000000000000000000000000000000000000000000000000000  lgtm-x86_64-apple-darwin.tar.gz
cafef00d00000000000000000000000000000000000000000000000000000000  lgtm-aarch64-apple-darwin.tar.gz
";
        assert_eq!(
            expected_sha(sums, "lgtm-aarch64-apple-darwin.tar.gz"),
            Some("cafef00d00000000000000000000000000000000000000000000000000000000".to_string())
        );
    }

    #[test]
    fn expected_sha_is_none_for_a_missing_file() {
        let sums = "deadbeef  lgtm-x86_64-apple-darwin.tar.gz\n";
        assert_eq!(expected_sha(sums, "lgtm-x86_64-pc-windows-msvc.zip"), None);
    }
}
