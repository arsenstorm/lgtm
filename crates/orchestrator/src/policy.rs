//! What the repository's `[policy]` lets the orchestrator do on its own. Pure:
//! the callers do the storing, the sending and the GitHub calls.

use lgtm_protocol::{
    CiState, Policy, ReviewState, Severity, Task, TaskEvent, TaskKind, TaskResult, TaskStatus,
};

#[derive(Debug, PartialEq, Eq)]
pub enum AutoAction {
    Approve,
    Merge,
}

impl AutoAction {
    fn as_str(&self) -> &'static str {
        match self {
            AutoAction::Approve => "approve",
            AutoAction::Merge => "merge",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Decision {
    pub action: AutoAction,
    pub allowed: bool,
    pub reasons: Vec<String>,
}

impl Decision {
    pub fn event(&self) -> TaskEvent {
        TaskEvent::PolicyDecision {
            action: self.action.as_str().to_string(),
            allowed: self.allowed,
            reasons: self.reasons.clone(),
        }
    }
}

/// What policy has to say about `task` right now: `None` when the policy does
/// not ask for anything at this status.
pub fn decide(task: &Task) -> Option<Decision> {
    let result = task.result.as_ref()?;
    let policy = result.policy.as_ref()?;
    match task.status {
        // A plan is approved by creating its steps, which is a decision
        // about work, not a diff to wave through.
        TaskStatus::AwaitingReview if policy.auto_approve && task.spec.kind == TaskKind::Run => {
            Some(approve(result, policy))
        }
        TaskStatus::Approved if policy.auto_merge && task.pull_request.is_some() => merge(task),
        _ => None,
    }
}

fn approve(result: &TaskResult, policy: &Policy) -> Decision {
    let mut reasons = refusals(result, policy);
    let allowed = reasons.is_empty();
    if allowed {
        reasons = vec!["checks passed".into(), "no blocking findings".into()];
        if policy.max_diff_lines.is_some() {
            reasons.push(format!("{} lines", diff_lines(&result.diff)));
        }
    }
    Decision {
        action: AutoAction::Approve,
        allowed,
        reasons,
    }
}

/// Every reason not to approve, not just the first: a developer reading this
/// afterwards wants the whole list.
fn refusals(result: &TaskResult, policy: &Policy) -> Vec<String> {
    let failed = result
        .validation
        .iter()
        .filter(|check| !check.ok)
        .map(|check| format!("check {} failed", check.name));
    let blocking = result
        .review
        .iter()
        .flat_map(|review| &review.findings)
        .filter(|finding| finding.severity == Severity::Blocking)
        .map(|finding| format!("blocking review finding: {}", finding.message));
    let mut reasons: Vec<String> = failed.chain(blocking).collect();
    if let Some(max) = policy.max_diff_lines {
        let lines = diff_lines(&result.diff);
        if lines > max {
            reasons.push(format!("diff is {lines} lines, limit {max}"));
        }
    }
    reasons.extend(
        result
            .changed_files
            .iter()
            .filter(|path| protected(policy, path))
            .map(|path| format!("touches protected file {path}")),
    );
    if let Some(budget) = policy.budget_per_task_usd {
        if result.cost_usd > budget {
            let cost = result.cost_usd;
            reasons.push(format!("cost ${cost:.2} over budget ${budget:.2}"));
        }
    }
    reasons
}

/// A pending CI run is not a refusal to merge: nothing is decided yet.
fn merge(task: &Task) -> Option<Decision> {
    let state = task.ci.as_ref()?.state;
    if state == CiState::Pending {
        return None;
    }
    let changes_requested = task
        .pr_review
        .as_ref()
        .is_some_and(|review| review.state == ReviewState::ChangesRequested);
    let mut reasons = vec![format!("ci {}", ci_word(state))];
    if changes_requested {
        reasons.push("pr review requested changes".into());
    }
    Some(Decision {
        action: AutoAction::Merge,
        allowed: state == CiState::Success && !changes_requested,
        reasons,
    })
}

fn ci_word(state: CiState) -> &'static str {
    match state {
        CiState::Pending => "pending",
        CiState::Success => "success",
        CiState::Failure => "failure",
    }
}

/// Added and removed lines, without the `+++`/`---` file headers.
fn diff_lines(diff: &str) -> u32 {
    let count = diff
        .lines()
        .filter(|line| changed_line(line, '+', "+++") || changed_line(line, '-', "---"))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn changed_line(line: &str, mark: char, header: &str) -> bool {
    line.starts_with(mark) && !line.starts_with(header)
}

fn protected(policy: &Policy, path: &str) -> bool {
    policy
        .protected_files
        .iter()
        .any(|pattern| glob_match(pattern, path))
}

/// `*` stands for any run of characters, `/` included; nothing else is special.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let Some((head, rest)) = pattern.split_once('*') else {
        return pattern == path;
    };
    let Some(tail) = path.strip_prefix(head) else {
        return false;
    };
    (0..=tail.len())
        .filter(|at| tail.is_char_boundary(*at))
        .any(|at| glob_match(rest, &tail[at..]))
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
