//! The `[policy]` table of `<worktree>/.lgtm/config.toml`, and the prompts the
//! policies drive: fixing failing checks, and reviewing the finished diff.

use std::path::Path;

use lgtm_protocol::{
    Executor, Finding, Review, SandboxProfile, Severity, TaskSpec, ValidationResult,
};

/// How much of the diff the reviewer is shown.
const DIFF_CHARS: usize = 60_000;

/// An hour: long enough for real work, short enough that a wedged agent does
/// not hold a slot overnight.
const DEFAULT_TIMEOUT_SECS: u64 = 3600;

/// What an `allowlist` run reaches when the repository names no hosts: the
/// registries a build needs and the APIs the harnesses talk to.
const DEFAULT_ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "api.github.com",
    "api.anthropic.com",
    "registry.npmjs.org",
    "crates.io",
    "static.crates.io",
    "index.crates.io",
    "pypi.org",
    "files.pythonhosted.org",
];

/// What the repository asked the runner to do beyond running the agent once.
#[derive(Clone, Debug, PartialEq)]
pub struct PolicyConfig {
    /// Extra agent runs after a crash.
    pub retry: u32,
    /// Follow-up runs that try to fix failing checks.
    pub fix_checks: u32,
    /// Review the finished diff with a second agent run.
    pub review: bool,
    pub auto_approve: bool,
    pub auto_merge: bool,
    /// Refuse auto-approve when the diff has more added+removed lines than this.
    pub max_diff_lines: Option<u32>,
    /// Paths (with `*` wildcards) an automatic approval must not touch.
    pub protected_files: Vec<String>,
    /// Refuse auto-approve when the run cost more than this.
    pub budget_per_task_usd: Option<f64>,
    /// Move a lost or failed task to another runner this many times.
    pub reassign: u32,
    /// Kill an agent run that has been going this long.
    pub timeout_secs: u64,
    pub sandbox: SandboxProfile,
    pub network: NetworkPolicy,
    pub limits: Limits,
    pub review_executor: ReviewExecutor,
}

/// What one sandboxed run may consume. `None` is no limit of that kind, which
/// stays the default: capping a run that used to be uncapped can only break it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub memory_mb: Option<u64>,
    pub processes: Option<u64>,
    pub cpu_seconds: Option<u64>,
}

/// Where an agent run may go on the network. Still `Unrestricted` by default:
/// narrowing that is a decision for its own change.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum NetworkPolicy {
    #[default]
    Unrestricted,
    None,
    Allowlist(Vec<String>),
}

/// The harness for the review pass. Defaults to `Auto` so a runner with both
/// harnesses reviews under the one that didn't write the diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ReviewExecutor {
    #[default]
    Auto,
    Fixed(Executor),
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            retry: 0,
            fix_checks: 0,
            review: false,
            auto_approve: false,
            auto_merge: false,
            max_diff_lines: None,
            protected_files: Vec::new(),
            budget_per_task_usd: None,
            reassign: 0,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            sandbox: SandboxProfile::Standard,
            network: NetworkPolicy::Unrestricted,
            limits: Limits::default(),
            review_executor: ReviewExecutor::Auto,
        }
    }
}

/// The task's own choice wins; then the repository's `[policy]
/// review_executor`; otherwise the first available harness that isn't the
/// one that implemented the task, falling back to that same harness when the
/// runner has no other.
pub fn reviewer(spec: &TaskSpec, policy: &PolicyConfig, available: &[Executor]) -> Executor {
    if let Some(executor) = spec.review_executor {
        return executor;
    }
    if let ReviewExecutor::Fixed(executor) = policy.review_executor {
        return executor;
    }
    available
        .iter()
        .copied()
        .find(|&executor| executor != spec.executor)
        .unwrap_or(spec.executor)
}

/// The task's own `sandbox` wins; otherwise the repository's `[sandbox]
/// profile`, which already defaults to `Standard`.
pub fn effective_sandbox(spec: &TaskSpec, policy: &PolicyConfig) -> SandboxProfile {
    spec.sandbox.unwrap_or(policy.sandbox)
}

pub fn load_policy(worktree: &Path) -> PolicyConfig {
    match std::fs::read_to_string(worktree.join(".lgtm").join("config.toml")) {
        Ok(text) => parse_policy(&text),
        Err(_) => PolicyConfig::default(),
    }
}

/// A missing section, a missing key, or a key of the wrong type all leave the
/// default in place: a malformed config must not change what the runner does.
pub fn parse_policy(text: &str) -> PolicyConfig {
    let mut policy = PolicyConfig::default();
    let table: toml::Table = match text.parse() {
        Ok(table) => table,
        Err(err) => {
            tracing::warn!(".lgtm/config.toml: {err}");
            return policy;
        }
    };
    read_sandbox(&table, &mut policy);
    let Some(section) = table.get("policy").and_then(toml::Value::as_table) else {
        return policy;
    };
    for (key, value) in section {
        match key.as_str() {
            "retry" => count(&mut policy.retry, key, value),
            "fix_checks" => count(&mut policy.fix_checks, key, value),
            "review" => flag(&mut policy.review, key, value),
            "auto_approve" => flag(&mut policy.auto_approve, key, value),
            "auto_merge" => flag(&mut policy.auto_merge, key, value),
            "max_diff_lines" => optional_count(&mut policy.max_diff_lines, key, value),
            "protected_files" => strings(&mut policy.protected_files, key, value),
            "budget_per_task_usd" => optional_money(&mut policy.budget_per_task_usd, key, value),
            "reassign" => count(&mut policy.reassign, key, value),
            "timeout_secs" => seconds(&mut policy.timeout_secs, key, value),
            "review_executor" => review_executor(&mut policy.review_executor, key, value),
            _ => tracing::warn!("[policy] unknown key {key}, ignoring"),
        }
    }
    policy
}

/// A separate table from `[policy]`: what confines a run is a runner concern,
/// not one of the checks-and-approval policies above it.
fn read_sandbox(table: &toml::Table, policy: &mut PolicyConfig) {
    let Some(section) = table.get("sandbox").and_then(toml::Value::as_table) else {
        return;
    };
    if let Some(profile) = section.get("profile").and_then(toml::Value::as_str) {
        match SandboxProfile::parse(profile) {
            Some(parsed) => policy.sandbox = parsed,
            None => tracing::warn!("[sandbox] profile must be off, standard or strict, ignoring"),
        }
    }
    policy.network = read_network(section);
    policy.limits = read_limits(section);
}

fn read_limits(section: &toml::Table) -> Limits {
    Limits {
        memory_mb: limit(section, "memory_mb"),
        processes: limit(section, "processes"),
        cpu_seconds: limit(section, "cpu_seconds"),
    }
}

/// Zero is refused along with the wrong types: a limit of nothing is a run
/// that cannot start, which is never what the config meant.
fn limit(section: &toml::Table, key: &str) -> Option<u64> {
    let parsed = section
        .get(key)?
        .as_integer()
        .and_then(|n| u64::try_from(n).ok())
        .filter(|n| *n > 0);
    if parsed.is_none() {
        tracing::warn!("[sandbox] {key} must be a positive integer, ignoring");
    }
    parsed
}

/// `allowed_hosts` is read whatever order it appears in, so an allowlist is
/// never silently the default list because the keys were the other way round.
fn read_network(section: &toml::Table) -> NetworkPolicy {
    let mut hosts: Vec<String> = DEFAULT_ALLOWED_HOSTS
        .iter()
        .map(|h| h.to_string())
        .collect();
    if let Some(value) = section.get("allowed_hosts") {
        strings(&mut hosts, "allowed_hosts", value);
    }
    match section.get("network").and_then(toml::Value::as_str) {
        Some("unrestricted") | None => NetworkPolicy::Unrestricted,
        Some("none") => NetworkPolicy::None,
        Some("allowlist") => NetworkPolicy::Allowlist(hosts),
        Some(_) => {
            tracing::warn!("[sandbox] network must be unrestricted, none or allowlist, ignoring");
            NetworkPolicy::Unrestricted
        }
    }
}

fn count(slot: &mut u32, key: &str, value: &toml::Value) {
    match value.as_integer().and_then(|n| u32::try_from(n).ok()) {
        Some(n) => *slot = n,
        None => tracing::warn!("[policy] {key} must be a non-negative integer, ignoring"),
    }
}

fn optional_count(slot: &mut Option<u32>, key: &str, value: &toml::Value) {
    match value.as_integer().and_then(|n| u32::try_from(n).ok()) {
        Some(n) => *slot = Some(n),
        None => tracing::warn!("[policy] {key} must be a non-negative integer, ignoring"),
    }
}

fn strings(slot: &mut Vec<String>, key: &str, value: &toml::Value) {
    let parsed: Option<Vec<String>> = value.as_array().map(|items| {
        items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect()
    });
    match parsed {
        Some(list) => *slot = list,
        None => tracing::warn!("[policy] {key} must be an array of strings, ignoring"),
    }
}

/// An amount of money, so an integer `2` is as valid as `2.0`.
fn optional_money(slot: &mut Option<f64>, key: &str, value: &toml::Value) {
    match value
        .as_float()
        .or_else(|| value.as_integer().map(|n| n as f64))
    {
        Some(n) => *slot = Some(n),
        None => tracing::warn!("[policy] {key} must be a number, ignoring"),
    }
}

fn seconds(slot: &mut u64, key: &str, value: &toml::Value) {
    match value.as_integer().and_then(|n| u64::try_from(n).ok()) {
        Some(n) => *slot = n,
        None => tracing::warn!("[policy] {key} must be a non-negative integer, ignoring"),
    }
}

fn flag(slot: &mut bool, key: &str, value: &toml::Value) {
    match value.as_bool() {
        Some(b) => *slot = b,
        None => tracing::warn!("[policy] {key} must be true or false, ignoring"),
    }
}

fn review_executor(slot: &mut ReviewExecutor, key: &str, value: &toml::Value) {
    match value.as_str() {
        Some("auto") => *slot = ReviewExecutor::Auto,
        Some("claude") => *slot = ReviewExecutor::Fixed(Executor::Claude),
        Some("codex") => *slot = ReviewExecutor::Fixed(Executor::Codex),
        _ => tracing::warn!("[policy] {key} must be auto, claude or codex, ignoring"),
    }
}

/// Asks the agent that just ran to fix the checks it broke, in its own session.
pub fn fix_prompt(failed: &[&ValidationResult]) -> String {
    let blocks: Vec<String> = failed
        .iter()
        .map(|check| format!("{}: {}\n{}", check.name, check.command, check.output_tail))
        .collect();
    format!(
        "These checks failed:\n\n{}\n\nFix them. Change nothing unrelated.",
        blocks.join("\n\n")
    )
}

pub fn failed_names(failed: &[&ValidationResult]) -> String {
    failed
        .iter()
        .map(|check| check.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

const REVIEW_HEAD: &str =
    "You are reviewing a change made by another agent. Do not modify files.\n\nTask the agent was given:\n";

const REVIEW_TAIL: &str = r#"Report only real problems: bugs, missing requirements, security issues, missing tests for changed behaviour. Answer with a single ```json block and nothing after it:
```json
{"findings": [{"severity": "blocking", "file": "path", "line": 12, "message": "what is wrong and why"}]}
```
severity is "blocking" for anything that must change before merge, "warning" otherwise. An empty findings list is a valid answer."#;

pub fn review_prompt(task_prompt: &str, diff: &str) -> String {
    format!(
        "{REVIEW_HEAD}{task_prompt}\n\nDiff:\n```diff\n{}\n```\n\n{REVIEW_TAIL}",
        truncate(diff)
    )
}

fn truncate(diff: &str) -> String {
    match diff.char_indices().nth(DIFF_CHARS) {
        Some((at, _)) => format!("{}\n[truncated]", &diff[..at]),
        None => diff.to_string(),
    }
}

/// The reviewer answers in the same fenced shape as a plan. Anything else is
/// reported as a warning rather than failing the task: the change is committed
/// either way, and a human still reads it.
pub fn parse_review(text: &str) -> Review {
    let json = crate::plan::last_json_block(text).unwrap_or_else(|| text.trim());
    match serde_json::from_str::<Review>(json) {
        Ok(review) => review,
        Err(err) => review_warning(format!("review output was not valid JSON: {err}")),
    }
}

pub fn review_warning(message: String) -> Review {
    Review {
        findings: vec![Finding {
            severity: Severity::Warning,
            file: String::new(),
            line: None,
            message,
        }],
        executor: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_section_and_bad_toml_are_the_default() {
        assert_eq!(parse_policy(""), PolicyConfig::default());
        assert_eq!(
            parse_policy("[validation]\ntest = \"bun test\"\n"),
            PolicyConfig::default()
        );
        assert_eq!(parse_policy("not toml at all = ="), PolicyConfig::default());
    }

    #[test]
    fn reads_the_keys_it_knows_and_ignores_the_rest() {
        let policy = parse_policy(
            "[policy]\nretry = 2\nreview = true\nauto_merge = true\ntimeout_secs = 120\nsomething = 1\n",
        );
        assert_eq!(
            policy,
            PolicyConfig {
                retry: 2,
                fix_checks: 0,
                review: true,
                auto_approve: false,
                auto_merge: true,
                max_diff_lines: None,
                protected_files: Vec::new(),
                budget_per_task_usd: None,
                reassign: 0,
                timeout_secs: 120,
                sandbox: SandboxProfile::Standard,
                network: NetworkPolicy::Unrestricted,
                limits: Limits::default(),
                review_executor: ReviewExecutor::Auto,
            }
        );
    }

    #[test]
    fn reads_the_auto_approve_gates() {
        let policy = parse_policy(
            "[policy]\nmax_diff_lines = 300\nprotected_files = [\"migrations/*\", \"Cargo.lock\"]\nbudget_per_task_usd = 2.0\n",
        );
        assert_eq!(policy.max_diff_lines, Some(300));
        assert_eq!(policy.protected_files, ["migrations/*", "Cargo.lock"]);
        assert_eq!(policy.budget_per_task_usd, Some(2.0));
        assert_eq!(
            parse_policy("[policy]\nbudget_per_task_usd = 2\n").budget_per_task_usd,
            Some(2.0)
        );
    }

    #[test]
    fn reads_reassign() {
        let policy = parse_policy("[policy]\nreassign = 2\n");
        assert_eq!(policy.reassign, 2);
    }

    #[test]
    fn wrong_types_keep_the_default() {
        let policy = parse_policy(
            "[policy]\nretry = \"x\"\nfix_checks = -1\nreview = \"yes\"\ntimeout_secs = -1\nmax_diff_lines = \"lots\"\nprotected_files = \"Cargo.lock\"\nbudget_per_task_usd = \"free\"\n",
        );
        assert_eq!(policy, PolicyConfig::default());
        assert_eq!(policy.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn sandbox_profile_is_read_from_its_own_table() {
        let policy = parse_policy("[sandbox]\nprofile = \"strict\"\n");
        assert_eq!(policy.sandbox, SandboxProfile::Strict);
    }

    #[test]
    fn bad_sandbox_profile_keeps_the_default() {
        let policy = parse_policy("[sandbox]\nprofile = \"chaos\"\n");
        assert_eq!(policy.sandbox, SandboxProfile::Standard);
    }

    #[test]
    fn network_defaults_to_unrestricted_and_reads_the_three_modes() {
        assert_eq!(parse_policy("").network, NetworkPolicy::Unrestricted);
        assert_eq!(
            parse_policy("[sandbox]\nnetwork = \"none\"\n").network,
            NetworkPolicy::None
        );
        assert_eq!(
            parse_policy("[sandbox]\nnetwork = \"nope\"\n").network,
            NetworkPolicy::Unrestricted
        );
        let NetworkPolicy::Allowlist(hosts) =
            parse_policy("[sandbox]\nnetwork = \"allowlist\"\n").network
        else {
            panic!("expected an allowlist");
        };
        assert_eq!(hosts, DEFAULT_ALLOWED_HOSTS);
    }

    #[test]
    fn allowed_hosts_replace_the_defaults_whichever_key_comes_first() {
        let listed = "allowed_hosts = [\"proxy.internal\", \".github.com\"]";
        for text in [
            format!("[sandbox]\nnetwork = \"allowlist\"\n{listed}\n"),
            format!("[sandbox]\n{listed}\nnetwork = \"allowlist\"\n"),
        ] {
            assert_eq!(
                parse_policy(&text).network,
                NetworkPolicy::Allowlist(vec![
                    "proxy.internal".to_string(),
                    ".github.com".to_string()
                ])
            );
        }
        // Named without an allowlist they change nothing, and a wrong type
        // leaves the defaults standing.
        assert_eq!(
            parse_policy(&format!("[sandbox]\n{listed}\n")).network,
            NetworkPolicy::Unrestricted
        );
        assert_eq!(
            parse_policy("[sandbox]\nnetwork = \"allowlist\"\nallowed_hosts = \"github.com\"\n")
                .network,
            NetworkPolicy::Allowlist(
                DEFAULT_ALLOWED_HOSTS
                    .iter()
                    .map(|h| h.to_string())
                    .collect()
            )
        );
    }

    #[test]
    fn limits_are_read_from_the_sandbox_table_and_default_to_none() {
        assert_eq!(parse_policy("").limits, Limits::default());
        assert_eq!(
            parse_policy("[sandbox]\nmemory_mb = 4096\nprocesses = 256\ncpu_seconds = 3600\n")
                .limits,
            Limits {
                memory_mb: Some(4096),
                processes: Some(256),
                cpu_seconds: Some(3600),
            }
        );
        assert_eq!(
            parse_policy("[sandbox]\nprocesses = 64\n").limits,
            Limits {
                processes: Some(64),
                ..Limits::default()
            }
        );
    }

    #[test]
    fn a_limit_that_is_not_a_positive_integer_is_no_limit() {
        assert_eq!(
            parse_policy("[sandbox]\nmemory_mb = \"lots\"\nprocesses = -1\ncpu_seconds = 0\n")
                .limits,
            Limits::default()
        );
    }

    #[test]
    fn effective_sandbox_prefers_the_task_over_the_policy() {
        let mut spec = sample_spec();
        let policy = PolicyConfig {
            sandbox: SandboxProfile::Strict,
            ..PolicyConfig::default()
        };
        assert_eq!(effective_sandbox(&spec, &policy), SandboxProfile::Strict);
        spec.sandbox = Some(SandboxProfile::Off);
        assert_eq!(effective_sandbox(&spec, &policy), SandboxProfile::Off);
    }

    #[test]
    fn review_executor_config_reads_auto_claude_codex_and_bad_value_warns() {
        assert_eq!(
            parse_policy("[policy]\nreview_executor = \"auto\"\n").review_executor,
            ReviewExecutor::Auto
        );
        assert_eq!(
            parse_policy("[policy]\nreview_executor = \"claude\"\n").review_executor,
            ReviewExecutor::Fixed(Executor::Claude)
        );
        assert_eq!(
            parse_policy("[policy]\nreview_executor = \"codex\"\n").review_executor,
            ReviewExecutor::Fixed(Executor::Codex)
        );
        assert_eq!(
            parse_policy("[policy]\nreview_executor = \"gpt\"\n").review_executor,
            ReviewExecutor::Auto
        );
    }

    #[test]
    fn reviewer_prefers_the_spec_over_the_policy() {
        let mut spec = sample_spec();
        spec.review_executor = Some(Executor::Codex);
        let policy = PolicyConfig {
            review_executor: ReviewExecutor::Fixed(Executor::Claude),
            ..PolicyConfig::default()
        };
        assert_eq!(
            reviewer(&spec, &policy, &[Executor::Claude]),
            Executor::Codex
        );
    }

    #[test]
    fn reviewer_falls_back_to_a_fixed_policy() {
        let spec = sample_spec();
        let policy = PolicyConfig {
            review_executor: ReviewExecutor::Fixed(Executor::Codex),
            ..PolicyConfig::default()
        };
        assert_eq!(
            reviewer(&spec, &policy, &[Executor::Claude, Executor::Codex]),
            Executor::Codex
        );
    }

    #[test]
    fn reviewer_auto_picks_the_other_available_harness() {
        let spec = sample_spec(); // executor: Claude
        let policy = PolicyConfig::default();
        assert_eq!(
            reviewer(&spec, &policy, &[Executor::Claude, Executor::Codex]),
            Executor::Codex
        );
    }

    #[test]
    fn reviewer_auto_falls_back_to_the_same_harness_alone() {
        let spec = sample_spec(); // executor: Claude
        let policy = PolicyConfig::default();
        assert_eq!(
            reviewer(&spec, &policy, &[Executor::Claude]),
            Executor::Claude
        );
        assert_eq!(reviewer(&spec, &policy, &[]), Executor::Claude);
    }

    fn sample_spec() -> TaskSpec {
        TaskSpec {
            repository: "r".into(),
            base_branch: "main".into(),
            prompt: "p".into(),
            executor: lgtm_protocol::Executor::Claude,
            runner: None,
            issue: None,
            linear: None,
            kind: lgtm_protocol::TaskKind::Run,
            parent: None,
            depends_on: vec![],
            depends_on_condition: Default::default(),
            batch: None,
            sandbox: None,
            requirements: vec![],
            goal: None,
            review_executor: None,
            model: None,
            allowed_hosts: Vec::new(),
            session: None,
        }
    }

    fn check(name: &str) -> ValidationResult {
        ValidationResult {
            name: name.to_string(),
            command: format!("bun {name}"),
            ok: false,
            output_tail: format!("{name} blew up"),
        }
    }

    #[test]
    fn fix_prompt_carries_every_failing_check() {
        let (a, b) = (check("test"), check("lint"));
        let prompt = fix_prompt(&[&a, &b]);
        for c in [&a, &b] {
            assert!(prompt.contains(&c.name), "{prompt}");
            assert!(prompt.contains(&c.command), "{prompt}");
            assert!(prompt.contains(&c.output_tail), "{prompt}");
        }
        assert!(prompt.starts_with("These checks failed:"));
        assert!(prompt.ends_with("Fix them. Change nothing unrelated."));
        assert_eq!(failed_names(&[&a, &b]), "test, lint");
    }

    #[test]
    fn review_prompt_carries_the_task_the_diff_and_the_shape() {
        let prompt = review_prompt("add a /health endpoint", "--- a\n+++ b\n");
        assert!(prompt.contains("add a /health endpoint"));
        assert!(prompt.contains("--- a\n+++ b"));
        assert!(prompt.contains(r#""findings""#));
    }

    #[test]
    fn long_diffs_are_truncated() {
        let prompt = review_prompt("goal", &"x".repeat(DIFF_CHARS * 2));
        assert!(prompt.contains("[truncated]"));
        assert!(prompt.len() < DIFF_CHARS * 2);
    }

    #[test]
    fn reads_the_fenced_findings() {
        let review = parse_review(
            "Looks fine.\n```json\n{\"findings\":[{\"severity\":\"blocking\",\"file\":\"a.rs\",\"line\":3,\"message\":\"boom\"}]}\n```",
        );
        assert!(review.has_blocking());
        assert_eq!(review.findings[0].message, "boom");
        assert!(parse_review(r#"{"findings":[]}"#).findings.is_empty());
    }

    #[test]
    fn unparsable_review_is_one_warning() {
        let review = parse_review("I could not review this.");
        assert_eq!(review.findings.len(), 1);
        assert_eq!(review.findings[0].severity, Severity::Warning);
        assert!(
            review.findings[0]
                .message
                .starts_with("review output was not valid JSON"),
            "{}",
            review.findings[0].message
        );
        assert!(!review.has_blocking());
    }
}
