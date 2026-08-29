//! What the `standard` profile means in practice: the agent runs with a
//! stripped environment, may write only where its work belongs, and cannot
//! read the host's secrets. macOS confines with `sandbox-exec`, Linux with
//! `bubblewrap`; everywhere else only the environment allowlist applies.

use std::path::{Path, PathBuf};

use lgtm_protocol::SandboxProfile;

pub struct Paths<'a> {
    pub worktree: &'a Path,
    pub mirror: &'a Path,
    pub home: &'a Path,
}

/// A program and its arguments, wrapped or not.
pub struct Wrapped {
    pub program: PathBuf,
    pub args: Vec<String>,
}

/// Directories under `HOME` an agent legitimately writes to: its own state,
/// and the caches its tools rebuild anyway.
const HOME_WRITES: &[&str] = &[".claude", ".codex", ".cache", ".npm", ".cargo"];

/// Temporary directories every toolchain assumes it can write to.
const TMP_ROOTS: &[&str] = &["/tmp", "/private/tmp", "/private/var/folders"];

/// HOME itself stays the real home, so claude and codex keep their own config
/// and login; the trade-off is that these secrets need denying by name.
// Not the login keychain: claude's OAuth token lives there on macOS, and
// allow-default gives it to every host process anyway.
const SECRETS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".config/gh",
    ".netrc",
    ".docker/config.json",
];

/// Variables kept by name.
const EXACT: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "TERM",
    "TMPDIR",
    "TZ",
    "COLORTERM",
    "NO_COLOR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// Variables kept by prefix: locale, desktop paths, the agent harnesses' own
/// settings, what `lgtm mcp` needs to answer for the run, and proxy
/// configuration in both spellings.
const PREFIXES: &[&str] = &[
    "LC_",
    "XDG_",
    "LGTM_",
    "ANTHROPIC_",
    "CLAUDE_",
    "OPENAI_",
    "CODEX_",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
];

/// The variables an agent run keeps; everything else is dropped so a token in
/// the runner's shell never reaches the agent.
pub fn env_allowlist() -> &'static [&'static str] {
    EXACT
}

pub fn keep_env(name: &str) -> bool {
    EXACT.contains(&name) || PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// The home the sandbox is built around: the real one, or the worktree when
/// the environment does not say.
pub fn home_dir(fallback: &Path) -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map_or_else(|| fallback.to_path_buf(), PathBuf::from)
}

/// Wraps `program args` for `profile`. `Off` returns them unchanged.
pub fn wrap(profile: SandboxProfile, paths: &Paths, program: &Path, args: &[String]) -> Wrapped {
    let plain = || Wrapped {
        program: program.to_path_buf(),
        args: args.to_vec(),
    };
    match profile {
        SandboxProfile::Off => return plain(),
        SandboxProfile::Strict => {
            tracing::warn!("strict isolation is not implemented yet; running the standard profile");
        }
        SandboxProfile::Standard => {}
    }
    match confine(paths, program, args) {
        Ok(wrapped) => wrapped,
        Err(reason) => {
            tracing::warn!(
                "{reason}; sandbox profile {} falls back to the environment allowlist only",
                profile.as_str()
            );
            plain()
        }
    }
}

#[cfg(target_os = "macos")]
fn confine(paths: &Paths, program: &Path, args: &[String]) -> Result<Wrapped, String> {
    let tmpdir = std::env::var_os("TMPDIR").map(PathBuf::from);
    let mut wrapped = vec![
        "-p".to_string(),
        seatbelt_profile(paths, tmpdir.as_deref()),
        program.display().to_string(),
    ];
    wrapped.extend_from_slice(args);
    Ok(Wrapped {
        program: PathBuf::from("sandbox-exec"),
        args: wrapped,
    })
}

#[cfg(target_os = "linux")]
fn confine(paths: &Paths, program: &Path, args: &[String]) -> Result<Wrapped, String> {
    let bwrap = which::which("bwrap").map_err(|_| "bwrap not found".to_string())?;
    Ok(Wrapped {
        program: bwrap,
        args: bwrap_args(paths, program, args),
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confine(_paths: &Paths, _program: &Path, _args: &[String]) -> Result<Wrapped, String> {
    Err(format!("no sandbox on {}", std::env::consts::OS))
}

/// The seatbelt profile for a run. Paths are canonicalized because seatbelt
/// matches the real path and `/tmp` is a symlink to `/private/tmp`.
pub fn seatbelt_profile(paths: &Paths, tmpdir: Option<&Path>) -> String {
    let writes = subpaths(&writable_roots(paths, tmpdir));
    let reads = subpaths(&secret_paths(paths));
    format!(
        "(version 1)\n\
         (allow default)\n\
         (deny file-write*)\n\
         (allow file-write*{writes} (literal \"/dev/null\") \
         (regex #\"^/dev/tty\") (regex #\"^/dev/std\"))\n\
         (deny file-read*{reads})\n"
    )
}

/// The bubblewrap argv for a run: everything readable, writes only to the
/// roots that exist, secrets shadowed by an empty tmpfs or `/dev/null`.
pub fn bwrap_args(paths: &Paths, program: &Path, args: &[String]) -> Vec<String> {
    let mut argv = strings(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
    for root in existing(writable_roots(paths, None)) {
        argv.extend(strings(&["--bind", &root, &root]));
    }
    for secret in existing(secret_paths(paths)) {
        match Path::new(&secret).is_dir() {
            true => argv.extend(strings(&["--tmpfs", &secret])),
            false => argv.extend(strings(&["--ro-bind", "/dev/null", &secret])),
        }
    }
    argv.extend(strings(&["--die-with-parent", "--unshare-pid", "--chdir"]));
    argv.push(paths.worktree.display().to_string());
    argv.push("--".to_string());
    argv.push(program.display().to_string());
    argv.extend_from_slice(args);
    argv
}

fn writable_roots(paths: &Paths, tmpdir: Option<&Path>) -> Vec<String> {
    let mut roots = vec![paths.worktree.to_path_buf(), paths.mirror.to_path_buf()];
    roots.extend(HOME_WRITES.iter().map(|dir| paths.home.join(dir)));
    roots.extend(TMP_ROOTS.iter().map(PathBuf::from));
    roots.extend(tmpdir.map(Path::to_path_buf));
    real_all(roots)
}

fn secret_paths(paths: &Paths) -> Vec<String> {
    real_all(SECRETS.iter().map(|name| paths.home.join(name)).collect())
}

fn real_all(paths: Vec<PathBuf>) -> Vec<String> {
    paths
        .into_iter()
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path))
        .map(|path| path.display().to_string())
        .collect()
}

fn existing(paths: Vec<String>) -> impl Iterator<Item = String> {
    paths.into_iter().filter(|path| Path::new(path).exists())
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn subpaths(paths: &[String]) -> String {
    paths
        .iter()
        .map(|path| format!(" (subpath \"{}\")", escape(path)))
        .collect()
}

/// Scheme string escaping: only the backslash and the quote matter.
fn escape(path: &str) -> String {
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
#[path = "sandbox_tests.rs"]
mod tests;
