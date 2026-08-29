//! Reading issues out of GraphQL responses.

use anyhow::anyhow;
use serde_json::Value;

use crate::Issue;

pub(crate) fn field_str(value: &Value, field: &str) -> anyhow::Result<String> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("linear: missing field {field} in response"))
}

pub(crate) fn require_success(value: &Value, pointer: &str) -> anyhow::Result<()> {
    if value.pointer(pointer).and_then(|v| v.as_bool()) == Some(true) {
        Ok(())
    } else {
        Err(anyhow!("linear: mutation failed"))
    }
}

pub(crate) fn parse_issue_response(value: &Value) -> anyhow::Result<Issue> {
    let issue = value
        .pointer("/data/issue")
        .ok_or_else(|| anyhow!("linear: missing issue in response"))?;
    issue_from_node(issue)
}

fn issue_from_node(issue: &Value) -> anyhow::Result<Issue> {
    Ok(Issue {
        id: field_str(issue, "id")?,
        identifier: field_str(issue, "identifier")?,
        title: field_str(issue, "title")?,
        description: issue
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        url: field_str(issue, "url")?,
        team_id: field_str(
            issue
                .get("team")
                .ok_or_else(|| anyhow!("linear: missing team in response"))?,
            "id",
        )?,
    })
}

/// Pure half: parses `data.issues.nodes` into `Issue`s (description null → "").
pub fn parse_issue_list(v: &Value) -> anyhow::Result<Vec<Issue>> {
    let nodes = v
        .pointer("/data/issues/nodes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("linear: missing issues in response"))?;
    nodes.iter().map(issue_from_node).collect()
}
