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
        &CustomPaths::default(),
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
fn seatbelt_profile_merges_the_custom_paths_and_orders_the_readable_allow_last() {
    let home = PathBuf::from("/home/me");
    let paths = Paths {
        worktree: Path::new("/work/tree"),
        mirror: Path::new("/data/repo.git"),
        home: &home,
    };
    let custom = CustomPaths {
        readable: vec!["/data/secret/public.json".to_string()],
        writable: vec!["/data/scratch".to_string()],
        denied: vec!["/data/secret".to_string()],
    };
    let profile = seatbelt_profile(&paths, None, &custom, Network::Unrestricted);

    assert!(profile.contains("(subpath \"/data/scratch\")"));
    assert!(profile.contains("(subpath \"/data/secret\")"));
    assert!(profile.contains("(subpath \"/data/secret/public.json\")"));
    let deny_at = profile
        .find("(deny file-read*")
        .expect("deny file-read line");
    let allow_at = profile
        .rfind("(allow file-read*")
        .expect("allow file-read line");
    assert!(deny_at < allow_at, "{profile}");
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
        &Limits {
            memory_mb: Some(4096),
            processes: Some(256),
            cpu_seconds: Some(3600),
        },
        &CustomPaths::default(),
        Path::new("/usr/bin/claude"),
        &args,
    );
    assert_eq!(wrapped.program, PathBuf::from("/usr/bin/claude"));
    assert_eq!(wrapped.args, args);
}

#[cfg(unix)]
#[test]
fn the_shell_sets_only_the_limits_the_config_named() {
    let claude = Path::new("/usr/bin/claude");
    let args = ["-p".to_string(), "do it".to_string()];
    let script = |limits: &Limits| {
        let wrapped = limited(limits, claude, &args);
        assert_eq!(wrapped.program, PathBuf::from("/bin/sh"));
        assert_eq!(wrapped.args[0], "-c");
        assert_eq!(wrapped.args[2..], ["/usr/bin/claude", "-p", "do it"]);
        wrapped.args[1].clone()
    };

    // Memory is asked for in MiB and `ulimit -v` wants KiB.
    let all = script(&Limits {
        memory_mb: Some(4096),
        processes: Some(256),
        cpu_seconds: Some(3600),
    });
    assert_eq!(
        all,
        "ulimit -v 4194304 2>/dev/null; ulimit -u 256 2>/dev/null; \
         ulimit -t 3600 2>/dev/null; exec \"$0\" \"$@\""
    );
    let one = |limits| script(&limits);
    assert_eq!(
        one(Limits {
            memory_mb: Some(512),
            ..Limits::default()
        }),
        "ulimit -v 524288 2>/dev/null; exec \"$0\" \"$@\""
    );
    assert_eq!(
        one(Limits {
            processes: Some(64),
            ..Limits::default()
        }),
        "ulimit -u 64 2>/dev/null; exec \"$0\" \"$@\""
    );
    assert_eq!(
        one(Limits {
            cpu_seconds: Some(60),
            ..Limits::default()
        }),
        "ulimit -t 60 2>/dev/null; exec \"$0\" \"$@\""
    );
}

#[cfg(unix)]
#[test]
fn no_limits_means_no_shell_in_the_way() {
    let args = ["-p".to_string()];
    let wrapped = limited(&Limits::default(), Path::new("/usr/bin/claude"), &args);
    assert_eq!(wrapped.program, PathBuf::from("/usr/bin/claude"));
    assert_eq!(wrapped.args, args);
    assert!(ulimit_script(&Limits::default()).is_none());
}

/// Proves the wrapper is a real limit and not just an argv: the same shell
/// runs work that fits and kills work that does not. The limit is CPU time
/// rather than memory because Darwin refuses `RLIMIT_AS` outright, which is
/// what the script's `2>/dev/null` is there to survive.
#[cfg(unix)]
#[test]
fn a_run_over_its_cpu_limit_is_killed_and_one_under_it_is_not() {
    let Ok(python) = which::which("python3") else {
        return;
    };
    let capped = Limits {
        cpu_seconds: Some(1),
        ..Limits::default()
    };
    let run = |script: &str| {
        let args = ["-c".to_string(), script.to_string()];
        let wrapped = limited(&capped, &python, &args);
        std::process::Command::new(&wrapped.program)
            .args(&wrapped.args)
            .stderr(std::process::Stdio::null())
            .status()
            .expect("python3")
            .success()
    };
    assert!(run("pass"));
    assert!(!run("while True: pass"));
}

#[test]
fn bwrap_argv_binds_the_worktree_over_a_read_only_root() {
    let worktree = scratch("bwrap");
    let paths = Paths {
        worktree: &worktree,
        mirror: &worktree,
        home: Path::new("/home/me"),
    };
    let no_custom = CustomPaths::default();
    let argv = bwrap_args(
        &paths,
        Path::new("/usr/bin/claude"),
        &["-p".to_string()],
        &no_custom,
        Network::Unrestricted,
    );

    let joined = argv.join(" ");
    assert!(joined.starts_with("--ro-bind / /"));
    assert!(joined.contains(&format!("--bind {0} {0}", worktree.display())));
    assert!(joined.contains("--die-with-parent"));
    assert!(joined.ends_with("-- /usr/bin/claude -p"));
    assert!(!joined.contains("--unshare-net"));

    let blocked = bwrap_args(
        &paths,
        Path::new("/usr/bin/claude"),
        &[],
        &no_custom,
        Network::Blocked,
    );
    assert!(blocked.contains(&"--unshare-net".to_string()));
    // Best effort until a netns lands: the proxy is only an environment hint.
    let proxied = bwrap_args(
        &paths,
        Path::new("/usr/bin/claude"),
        &[],
        &no_custom,
        Network::Proxy(1),
    );
    assert!(!proxied.contains(&"--unshare-net".to_string()));
    fs::remove_dir_all(&worktree).ok();
}

#[test]
fn bwrap_argv_denies_before_it_restores_a_readable_path() {
    let base = scratch("bwrap-custom");
    let worktree = base.join("worktree");
    let denied = base.join("denied");
    let readable = denied.join("keep");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::create_dir_all(&readable).expect("readable");
    let paths = Paths {
        worktree: &worktree,
        mirror: &worktree,
        home: Path::new("/home/me"),
    };
    let custom = CustomPaths {
        readable: vec![readable.display().to_string()],
        writable: Vec::new(),
        denied: vec![denied.display().to_string()],
    };
    let argv = bwrap_args(
        &paths,
        Path::new("/usr/bin/claude"),
        &[],
        &custom,
        Network::Unrestricted,
    );

    let deny_at = argv
        .iter()
        .position(|arg| arg == &denied.display().to_string())
        .expect("denied path bound");
    let allow_at = argv
        .iter()
        .position(|arg| arg == &readable.display().to_string())
        .expect("readable path bound");
    assert!(deny_at < allow_at, "{argv:?}");
    assert_eq!(argv[deny_at - 1], "--tmpfs");
    assert_eq!(argv[allow_at - 1], "--ro-bind");
    fs::remove_dir_all(&base).ok();
}

#[test]
fn the_seatbelt_profile_says_what_each_network_mode_may_reach() {
    let paths = Paths {
        worktree: Path::new("/work/tree"),
        mirror: Path::new("/data/repo.git"),
        home: Path::new("/home/me"),
    };
    let profile = |network| seatbelt_profile(&paths, None, &CustomPaths::default(), network);
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
    let custom_writable = base.join("custom-writable");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::create_dir_all(&outside).expect("outside");
    fs::create_dir_all(&custom_writable).expect("custom writable");
    fs::create_dir_all(home.join(".ssh")).expect("ssh dir");
    fs::write(home.join(".ssh").join("id_test"), "secret").expect("key");
    let paths = Paths {
        worktree: &worktree,
        mirror: &worktree,
        home: &home,
    };
    let custom = CustomPaths {
        readable: Vec::new(),
        writable: vec![custom_writable.display().to_string()],
        denied: Vec::new(),
    };
    let profile = seatbelt_profile(&paths, None, &custom, Network::Unrestricted);

    assert!(sh(&profile, &format!("touch {}/ok", worktree.display())));
    assert!(worktree.join("ok").exists());
    assert!(!sh(&profile, &format!("touch {}/nope", outside.display())));
    assert!(!outside.join("nope").exists());
    // The one path this profile's [sandbox] writable named, outside the
    // worktree and mirror both.
    assert!(sh(
        &profile,
        &format!("touch {}/ok", custom_writable.display())
    ));
    assert!(custom_writable.join("ok").exists());
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
