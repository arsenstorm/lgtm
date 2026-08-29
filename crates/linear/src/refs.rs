//! Naming an issue or a workflow state.

use crate::{Target, WorkflowState};

/// Parses `ENG-123` or `https://linear.app/<workspace>/issue/ENG-123[/anything]`.
pub fn parse_issue(s: &str) -> Option<String> {
    if let Some(rest) = s
        .strip_prefix("https://linear.app/")
        .and_then(|rest| rest.split_once("/issue/").map(|(_, tail)| tail))
    {
        let identifier = rest.split('/').next().unwrap_or("");
        return is_issue_identifier(identifier).then(|| identifier.to_string());
    }
    is_issue_identifier(s).then(|| s.to_string())
}

fn is_issue_identifier(s: &str) -> bool {
    let Some((team, number)) = s.rsplit_once('-') else {
        return false;
    };
    all_of(team, char::is_ascii_uppercase) && all_of(number, char::is_ascii_digit)
}

fn all_of(s: &str, pred: fn(&char) -> bool) -> bool {
    !s.is_empty() && s.chars().all(|c| pred(&c))
}

/// Started → first state with kind "started"; Completed → first with kind
/// "completed"; InReview → the state whose name equals "in review"
/// case-insensitively, else `None`.
pub fn pick_state(states: &[WorkflowState], target: Target) -> Option<WorkflowState> {
    match target {
        Target::Started => states.iter().find(|s| s.kind == "started").cloned(),
        Target::Completed => states.iter().find(|s| s.kind == "completed").cloned(),
        Target::InReview => states
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case("in review"))
            .cloned(),
    }
}
