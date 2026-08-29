//! Folding a pull request's check runs into one CI status.

use lgtm_protocol::{CiState, CiStatus};

fn run_str<'a>(run: &'a serde_json::Value, key: &str) -> &'a str {
    run.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

fn html_url(run: &serde_json::Value) -> String {
    run_str(run, "html_url").to_string()
}

const FAILING_CONCLUSIONS: [&str; 4] = ["failure", "timed_out", "cancelled", "action_required"];

/// Pure aggregation used by [`GitHub::checks`]: no runs, or any run not yet
/// `completed`, is `Pending`; any completed run with a failing conclusion is
/// `Failure`; otherwise `Success`.
pub fn aggregate_checks(runs: &[serde_json::Value], fallback_url: &str) -> CiStatus {
    if runs.is_empty() {
        return CiStatus {
            state: CiState::Pending,
            url: fallback_url.to_string(),
        };
    }

    let first_url = html_url(&runs[0]);

    if runs.iter().any(|run| run_str(run, "status") != "completed") {
        return CiStatus {
            state: CiState::Pending,
            url: first_url,
        };
    }

    if let Some(failing) = runs
        .iter()
        .find(|run| FAILING_CONCLUSIONS.contains(&run_str(run, "conclusion")))
    {
        return CiStatus {
            state: CiState::Failure,
            url: html_url(failing),
        };
    }

    CiStatus {
        state: CiState::Success,
        url: first_url,
    }
}
