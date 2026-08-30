//! The `[policy]` table of `<worktree>/.lgtm/config.toml`, and the prompts the
//! policies drive: fixing failing checks, and reviewing the finished diff.

use std::path::Path;

use lgtm_protocol::{Finding, Review, SandboxProfile, Severity, TaskSpec, ValidationResult};

/// How much of the diff the reviewer is shown.
const DIFF_CHARS: usize = 60_000;

/// An hour: long enough for real work, short enough that a wedged agent does
/// not hold a slot overnight.
const DEFAULT_TIMEOUT_SECS: u64 = 3600;

/// What the repository asked the worker to do beyond running the agent once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyConfig {
    /// Extra agent runs after a crash.
    pub retry: u32,
    /// Follow-up runs that try to fix failing checks.
    pub fix_checks: u32,
    /// Review the finished diff with a second agent run.
    pub review: bool,
    pub auto_approve: bool,
    pub auto_merge: bool,
    /// Kill an agent run that has been going this long.
    pub timeout_secs: u64,
    pub sandbox: SandboxProfile,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            retry: 0,
            fix_checks: 0,
            review: false,
            auto_approve: false,
            auto_merge: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            sandbox: SandboxProfile::Standard,
        }
    }
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
/// default in place: a malformed config must not change what the worker does.
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
            "timeout_secs" => seconds(&mut policy.timeout_secs, key, value),
            _ => tracing::warn!("[policy] unknown key {key}, ignoring"),
        }
    }
    policy
}

/// A separate table from `[policy]`: the sandbox profile is a runner
/// concern, not one of the checks-and-approval policies above it.
fn read_sandbox(table: &toml::Table, policy: &mut PolicyConfig) {
    let Some(profile) = table
        .get("sandbox")
        .and_then(toml::Value::as_table)
        .and_then(|section| section.get("profile"))
        .and_then(toml::Value::as_str)
    else {
        return;
    };
    match SandboxProfile::parse(profile) {
        Some(parsed) => policy.sandbox = parsed,
        None => tracing::warn!("[sandbox] profile must be off, standard or strict, ignoring"),
    }
}

fn count(slot: &mut u32, key: &str, value: &toml::Value) {
    match value.as_integer().and_then(|n| u32::try_from(n).ok()) {
        Some(n) => *slot = n,
        None => tracing::warn!("[policy] {key} must be a non-negative integer, ignoring"),
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
                timeout_secs: 120,
                sandbox: SandboxProfile::Standard,
            }
        );
    }

    #[test]
    fn wrong_types_keep_the_default() {
        let policy = parse_policy(
            "[policy]\nretry = \"x\"\nfix_checks = -1\nreview = \"yes\"\ntimeout_secs = -1\n",
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

    fn sample_spec() -> TaskSpec {
        TaskSpec {
            repository: "r".into(),
            base_branch: "main".into(),
            prompt: "p".into(),
            executor: lgtm_protocol::Executor::Claude,
            worker: None,
            issue: None,
            linear: None,
            kind: lgtm_protocol::TaskKind::Run,
            parent: None,
            depends_on: vec![],
            batch: None,
            sandbox: None,
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
