//! What the `standard` profile means in practice: the agent runs with a
//! stripped environment, may write only where its work belongs, and cannot
//! read the host's secrets, and spends no more memory, processes or CPU than
//! the repository allows. `custom` is `standard` with the repository's own
//! `readable`/`writable`/`denied` paths layered on. macOS confines with
//! `sandbox-exec`, Linux with `bubblewrap`; everywhere else only the
//! environment allowlist applies.

use std::path::{Path, PathBuf};

use lgtm_protocol::SandboxProfile;

use crate::policy::{CustomPaths, Limits};

pub struct Paths<'a> {
    pub worktree: &'a Path,
    pub mirror: &'a Path,
    pub home: &'a Path,
}

/// The network the run gets: everything, nothing, or only the allowlist proxy
/// listening on this port. The hosts themselves are the proxy's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Network {
    #[default]
    Unrestricted,
    Blocked,
    Proxy(u16),
}

/// The proxy variables in both spellings, so every client finds one it reads.
const PROXY_VARS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
];

/// What the run is told about proxies: the run's own proxy under an
/// allowlist, empty strings under `none` so an inherited `HTTP_PROXY` cannot
/// stand in for the network this run is not allowed. `NO_PROXY` is emptied
/// either way: an inherited exception would be a hole in the only route out.
pub fn network_env(network: Network) -> Vec<(&'static str, String)> {
    let proxy = match network {
        Network::Unrestricted => return Vec::new(),
        Network::Blocked => String::new(),
        Network::Proxy(port) => format!("http://127.0.0.1:{port}"),
    };
    let mut env: Vec<(&'static str, String)> = PROXY_VARS
        .iter()
        .map(|name| (*name, proxy.clone()))
        .collect();
    env.push(("NO_PROXY", String::new()));
    env.push(("no_proxy", String::new()));
    env
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
    // Windows system configuration: paths and machine facts, no secrets.
    // Without SYSTEMROOT, bun (and so the claude CLI) refuses to make any
    // network request at all.
    "SYSTEMROOT",
    "WINDIR",
    "SYSTEMDRIVE",
    "COMSPEC",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "TEMP",
    "TMP",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
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
    // Windows environment names are case-insensitive and case-preserving
    // ("Path", "SystemRoot"), so there they are matched uppercased.
    #[cfg(windows)]
    let upper = name.to_ascii_uppercase();
    #[cfg(windows)]
    let name = upper.as_str();
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
pub fn wrap(
    profile: SandboxProfile,
    paths: &Paths,
    network: Network,
    limits: &Limits,
    custom: &CustomPaths,
    program: &Path,
    args: &[String],
) -> Wrapped {
    match profile {
        SandboxProfile::Off => {
            return Wrapped {
                program: program.to_path_buf(),
                args: args.to_vec(),
            }
        }
        SandboxProfile::Strict => {
            tracing::warn!("strict isolation is not implemented yet; running the standard profile");
        }
        SandboxProfile::Standard | SandboxProfile::Custom => {}
    }
    // The limits go on inside the sandbox, so they bind the agent and the
    // sandbox binds the shell that sets them.
    let limited = limited(limits, program, args);
    let no_paths = CustomPaths::default();
    // A repository's [sandbox] readable/writable/denied stay in the config
    // whichever profile is active; only `custom` itself applies them, so
    // switching back to `standard` needs no edit to un-name them.
    let custom = if profile == SandboxProfile::Custom {
        custom
    } else {
        &no_paths
    };
    match confine(paths, network, custom, &limited.program, &limited.args) {
        Ok(wrapped) => wrapped,
        Err(reason) => {
            tracing::warn!(
                "{reason}; sandbox profile {} falls back to the environment allowlist only{}",
                profile.as_str(),
                unenforced(network)
            );
            limited
        }
    }
}

/// `program args` behind the shell that sets the run's resource limits, or
/// unchanged when there are none to set. Failures are swallowed: a shell whose
/// build lacks one of these must still run the agent.
#[cfg(unix)]
fn limited(limits: &Limits, program: &Path, args: &[String]) -> Wrapped {
    let Some(script) = ulimit_script(limits) else {
        return Wrapped {
            program: program.to_path_buf(),
            args: args.to_vec(),
        };
    };
    let mut wrapped = vec!["-c".to_string(), script, program.display().to_string()];
    wrapped.extend_from_slice(args);
    Wrapped {
        program: PathBuf::from("/bin/sh"),
        args: wrapped,
    }
}

#[cfg(not(unix))]
fn limited(_limits: &Limits, program: &Path, args: &[String]) -> Wrapped {
    Wrapped {
        program: program.to_path_buf(),
        args: args.to_vec(),
    }
}

fn ulimit_script(limits: &Limits) -> Option<String> {
    let mut set: Vec<String> = Vec::new();
    // `-v` is address space in KiB; the other two are already in their units.
    if let Some(mb) = limits.memory_mb {
        set.push(format!("ulimit -v {}", mb.saturating_mul(1024)));
    }
    if let Some(processes) = limits.processes {
        set.push(format!("ulimit -u {processes}"));
    }
    if let Some(seconds) = limits.cpu_seconds {
        set.push(format!("ulimit -t {seconds}"));
    }
    if set.is_empty() {
        return None;
    }
    let set: Vec<String> = set
        .into_iter()
        .map(|s| format!("{s} 2>/dev/null"))
        .collect();
    Some(format!("{}; exec \"$0\" \"$@\"", set.join("; ")))
}

/// The cgroup a run was put in, removed when this drops: a cgroup directory
/// left behind outlives the runner that made it.
pub struct Confined(Option<PathBuf>);

impl Drop for Confined {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_dir(path);
        }
    }
}

/// What can only be applied to a running child: on Linux a cgroup, when this
/// user has one delegated. Everywhere else the ulimits are all there is.
#[cfg(target_os = "linux")]
pub fn confine_child(child: &tokio::process::Child, limits: &Limits) -> Confined {
    Confined(child.id().and_then(|pid| cgroup_limits(pid, limits)))
}

#[cfg(not(target_os = "linux"))]
pub fn confine_child(_child: &tokio::process::Child, _limits: &Limits) -> Confined {
    Confined(None)
}

/// Named for the pid rather than the task: a run is what the cgroup holds, and
/// a pid needs no escaping to be a directory name.
#[cfg(target_os = "linux")]
fn cgroup_limits(pid: u32, limits: &Limits) -> Option<PathBuf> {
    let path = Path::new("/sys/fs/cgroup").join(format!("lgtm-{pid}"));
    if limits.memory_mb.is_none() && limits.processes.is_none() {
        return None;
    }
    if let Err(err) = std::fs::create_dir(&path) {
        tracing::debug!("no cgroup for this run ({err}); the ulimits still apply");
        return None;
    }
    if let Some(mb) = limits.memory_mb {
        let _ = std::fs::write(path.join("memory.max"), (mb * 1024 * 1024).to_string());
    }
    if let Some(processes) = limits.processes {
        let _ = std::fs::write(path.join("pids.max"), processes.to_string());
    }
    // An empty cgroup limits nothing, so a run that cannot join it is better
    // off without one.
    if let Err(err) = std::fs::write(path.join("cgroup.procs"), pid.to_string()) {
        tracing::debug!("cgroup {}: {err}", path.display());
        let _ = std::fs::remove_dir(&path);
        return None;
    }
    Some(path)
}

/// Without a sandbox the network policy is whatever the agent's own client
/// honours, which is not a boundary. Say so rather than imply one.
fn unenforced(network: Network) -> &'static str {
    match network {
        Network::Unrestricted => "",
        _ => ", and the network policy is not enforced",
    }
}

#[cfg(target_os = "macos")]
fn confine(
    paths: &Paths,
    network: Network,
    custom: &CustomPaths,
    program: &Path,
    args: &[String],
) -> Result<Wrapped, String> {
    let tmpdir = std::env::var_os("TMPDIR").map(PathBuf::from);
    let mut wrapped = vec![
        "-p".to_string(),
        seatbelt_profile(paths, tmpdir.as_deref(), custom, network),
        program.display().to_string(),
    ];
    wrapped.extend_from_slice(args);
    Ok(Wrapped {
        program: PathBuf::from("sandbox-exec"),
        args: wrapped,
    })
}

#[cfg(target_os = "linux")]
fn confine(
    paths: &Paths,
    network: Network,
    custom: &CustomPaths,
    program: &Path,
    args: &[String],
) -> Result<Wrapped, String> {
    let bwrap = which::which("bwrap").map_err(|_| "bwrap not found".to_string())?;
    Ok(Wrapped {
        program: bwrap,
        args: bwrap_args(paths, program, args, custom, network),
    })
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn confine(
    _paths: &Paths,
    _network: Network,
    _custom: &CustomPaths,
    _program: &Path,
    _args: &[String],
) -> Result<Wrapped, String> {
    Err(format!("no sandbox on {}", std::env::consts::OS))
}

/// The seatbelt profile for a run. Paths are canonicalized because seatbelt
/// matches the real path and `/tmp` is a symlink to `/private/tmp`.
pub fn seatbelt_profile(
    paths: &Paths,
    tmpdir: Option<&Path>,
    custom: &CustomPaths,
    network: Network,
) -> String {
    let writes = subpaths(&writable_roots(paths, tmpdir, &custom.writable));
    let reads = subpaths(&secret_paths(paths, &custom.denied));
    // Ordered after the deny above: seatbelt applies the later of two
    // matching rules, so this is what lets a `readable` path back in when a
    // `denied` parent would otherwise cover it too.
    let allow_reads = readable_paths(custom);
    let allow_reads = match allow_reads.is_empty() {
        true => String::new(),
        false => format!("(allow file-read*{})\n", subpaths(&allow_reads)),
    };
    format!(
        "(version 1)\n\
         (allow default)\n\
         (deny file-write*)\n\
         (allow file-write*{writes} (literal \"/dev/null\") \
         (regex #\"^/dev/tty\") (regex #\"^/dev/std\"))\n\
         (deny file-read*{reads})\n\
         {allow_reads}{}",
        seatbelt_network(network)
    )
}

/// Unix sockets stay open under an allowlist: the harnesses talk to their own
/// MCP servers over them, and they leave the machine no more than a pipe does.
fn seatbelt_network(network: Network) -> String {
    match network {
        Network::Unrestricted => String::new(),
        Network::Blocked => "(deny network*)\n".to_string(),
        Network::Proxy(port) => format!(
            "(deny network-outbound)\n\
             (allow network-outbound (to ip \"localhost:{port}\"))\n\
             (allow network-outbound (to unix-socket))\n"
        ),
    }
}

/// The bubblewrap argv for a run: everything readable, writes only to the
/// roots that exist, secrets shadowed by an empty tmpfs or `/dev/null`.
pub fn bwrap_args(
    paths: &Paths,
    program: &Path,
    args: &[String],
    custom: &CustomPaths,
    network: Network,
) -> Vec<String> {
    let mut argv = strings(&["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
    // ponytail: an allowlist on Linux is only the proxy variables, which a
    // determined process can ignore. Upgrade: a network namespace with a veth
    // to the proxy, so nothing else has a route at all.
    if network == Network::Blocked {
        argv.push("--unshare-net".to_string());
    }
    for root in existing(writable_roots(paths, None, &custom.writable)) {
        argv.extend(strings(&["--bind", &root, &root]));
    }
    for secret in existing(secret_paths(paths, &custom.denied)) {
        match Path::new(&secret).is_dir() {
            true => argv.extend(strings(&["--tmpfs", &secret])),
            false => argv.extend(strings(&["--ro-bind", "/dev/null", &secret])),
        }
    }
    // After the shadows above: the whole root is already read-only, so this
    // only matters when a `denied` parent just hid one of these paths back.
    for readable in existing(readable_paths(custom)) {
        argv.extend(strings(&["--ro-bind", &readable, &readable]));
    }
    argv.extend(strings(&["--die-with-parent", "--unshare-pid", "--chdir"]));
    argv.push(paths.worktree.display().to_string());
    argv.push("--".to_string());
    argv.push(program.display().to_string());
    argv.extend_from_slice(args);
    argv
}

fn writable_roots(paths: &Paths, tmpdir: Option<&Path>, custom: &[String]) -> Vec<String> {
    let mut roots = vec![paths.worktree.to_path_buf(), paths.mirror.to_path_buf()];
    roots.extend(HOME_WRITES.iter().map(|dir| paths.home.join(dir)));
    roots.extend(TMP_ROOTS.iter().map(PathBuf::from));
    roots.extend(tmpdir.map(Path::to_path_buf));
    roots.extend(custom.iter().map(PathBuf::from));
    real_all(roots)
}

fn secret_paths(paths: &Paths, custom: &[String]) -> Vec<String> {
    let mut denies: Vec<PathBuf> = SECRETS.iter().map(|name| paths.home.join(name)).collect();
    denies.extend(custom.iter().map(PathBuf::from));
    real_all(denies)
}

fn readable_paths(custom: &CustomPaths) -> Vec<String> {
    real_all(custom.readable.iter().map(PathBuf::from).collect())
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
