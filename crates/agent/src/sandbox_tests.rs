use std::fs;

use super::*;

/// Beside the test binary, because every system temporary directory is a
/// writable root and nothing there would prove a denial.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::current_exe()
        .expect("current exe")
        .parent()
        .expect("exe parent")
        .join(format!("lgtm-sandbox-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    fs::canonicalize(&dir).expect("canonicalize scratch")
}

#[test]
fn keeps_the_agent_variables_and_drops_the_secrets() {
    assert!(!keep_env("AWS_SECRET_ACCESS_KEY"));
    assert!(!keep_env("GITHUB_TOKEN"));
    assert!(keep_env("ANTHROPIC_API_KEY"));
    assert!(keep_env("LC_ALL"));
    assert!(keep_env("PATH"));
    assert!(env_allowlist().contains(&"PATH"));
}

#[test]
fn seatbelt_profile_allows_the_roots_and_denies_the_secrets() {
    let home = PathBuf::from("/home/\"quoted\"");
    let paths = Paths {
        worktree: Path::new("/work/tree"),
        mirror: Path::new("/data/repos/repo.git"),
        home: &home,
    };
    let profile = seatbelt_profile(
        &paths,
        Some(Path::new("/scratch/tmp")),
        Network::Unrestricted,
    );

    // `/tmp` is a symlink on macOS, and seatbelt only matches the real path.
    let tmp = fs::canonicalize("/tmp").expect("/tmp");
    for root in ["/work/tree", "/data/repos/repo.git", "/scratch/tmp"] {
        assert!(profile.contains(&format!("(subpath \"{root}\")")), "{root}");
    }
    assert!(profile.contains(&format!("(subpath \"{}\")", tmp.display())));
    assert!(profile.contains("(subpath \"/home/\\\"quoted\\\"/.claude\")"));
    assert!(profile.contains("(deny file-read*"));
    assert!(profile.contains("(subpath \"/home/\\\"quoted\\\"/.ssh\")"));
    assert!(profile.contains("(literal \"/dev/null\")"));
}

#[test]
fn off_runs_the_program_unchanged() {
    let paths = Paths {
        worktree: Path::new("/work/tree"),
        mirror: Path::new("/data/repo.git"),
        home: Path::new("/home/me"),
    };
    let args = vec!["-p".to_string(), "do it".to_string()];
    let wrapped = wrap(
        SandboxProfile::Off,
        &paths,
        Network::Unrestricted,
        Path::new("/usr/bin/claude"),
        &args,
    );
    assert_eq!(wrapped.program, PathBuf::from("/usr/bin/claude"));
    assert_eq!(wrapped.args, args);
}

#[test]
fn bwrap_argv_binds_the_worktree_over_a_read_only_root() {
    let worktree = scratch("bwrap");
    let paths = Paths {
        worktree: &worktree,
        mirror: &worktree,
        home: Path::new("/home/me"),
    };
    let argv = bwrap_args(
        &paths,
        Path::new("/usr/bin/claude"),
        &["-p".to_string()],
        Network::Unrestricted,
    );

    let joined = argv.join(" ");
    assert!(joined.starts_with("--ro-bind / /"));
    assert!(joined.contains(&format!("--bind {0} {0}", worktree.display())));
    assert!(joined.contains("--die-with-parent"));
    assert!(joined.ends_with("-- /usr/bin/claude -p"));
    assert!(!joined.contains("--unshare-net"));

    let blocked = bwrap_args(&paths, Path::new("/usr/bin/claude"), &[], Network::Blocked);
    assert!(blocked.contains(&"--unshare-net".to_string()));
    // Best effort until a netns lands: the proxy is only an environment hint.
    let proxied = bwrap_args(&paths, Path::new("/usr/bin/claude"), &[], Network::Proxy(1));
    assert!(!proxied.contains(&"--unshare-net".to_string()));
    fs::remove_dir_all(&worktree).ok();
}

#[test]
fn the_seatbelt_profile_says_what_each_network_mode_may_reach() {
    let paths = Paths {
        worktree: Path::new("/work/tree"),
        mirror: Path::new("/data/repo.git"),
        home: Path::new("/home/me"),
    };
    let profile = |network| seatbelt_profile(&paths, None, network);
    assert!(!profile(Network::Unrestricted).contains("network"));
    assert!(profile(Network::Blocked).contains("(deny network*)"));
    let proxied = profile(Network::Proxy(51234));
    assert!(proxied.contains("(deny network-outbound)"));
    assert!(proxied.contains("(allow network-outbound (to ip \"localhost:51234\"))"));
    assert!(proxied.contains("(allow network-outbound (to unix-socket))"));
}

#[test]
fn a_restricted_run_is_told_about_the_proxy_and_an_unrestricted_one_is_not() {
    assert!(network_env(Network::Unrestricted).is_empty());
    let proxied = network_env(Network::Proxy(8080));
    assert!(proxied.contains(&("HTTPS_PROXY", "http://127.0.0.1:8080".to_string())));
    assert!(proxied.contains(&("all_proxy", "http://127.0.0.1:8080".to_string())));
    assert!(proxied.contains(&("NO_PROXY", String::new())));
    // `none` empties them instead, so an inherited proxy is not a way out.
    assert!(network_env(Network::Blocked)
        .iter()
        .all(|(_, value)| value.is_empty()));
}

#[cfg(target_os = "macos")]
#[test]
fn seatbelt_denies_writes_outside_the_worktree_and_reads_of_secrets() {
    let base = scratch("seatbelt");
    let worktree = base.join("worktree");
    let outside = base.join("outside");
    let home = base.join("home");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::create_dir_all(&outside).expect("outside");
    fs::create_dir_all(home.join(".ssh")).expect("ssh dir");
    fs::write(home.join(".ssh").join("id_test"), "secret").expect("key");
    let paths = Paths {
        worktree: &worktree,
        mirror: &worktree,
        home: &home,
    };
    let profile = seatbelt_profile(&paths, None, Network::Unrestricted);

    assert!(sh(&profile, &format!("touch {}/ok", worktree.display())));
    assert!(worktree.join("ok").exists());
    assert!(!sh(&profile, &format!("touch {}/nope", outside.display())));
    assert!(!outside.join("nope").exists());
    assert!(!sh(
        &profile,
        &format!("cat {}/.ssh/id_test", home.display())
    ));
    fs::remove_dir_all(&base).ok();
}

#[cfg(target_os = "macos")]
fn sh(profile: &str, script: &str) -> bool {
    std::process::Command::new("sandbox-exec")
        .args(["-p", profile, "/bin/sh", "-c", script])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("sandbox-exec")
        .success()
}
