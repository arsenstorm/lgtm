//! Minimal Linear GraphQL client: issue lookup, workflow states, moving an
//! issue between states, and posting comments.

use anyhow::anyhow;
use serde_json::Value;

const API_URL: &str = "https://api.linear.app/graphql";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub team_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowState {
    pub id: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Started,
    InReview,
    Completed,
}

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
    !team.is_empty()
        && team.chars().all(|c| c.is_ascii_uppercase())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
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

#[derive(Clone)]
pub struct Linear {
    key: String,
    http: reqwest::Client,
}

impl Linear {
    pub fn new(key: impl Into<String>) -> Self {
        Linear {
            key: key.into(),
            http: reqwest::Client::new(),
        }
    }

    /// `LINEAR_API_KEY`, else `None`.
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("LINEAR_API_KEY").ok()?;
        if key.is_empty() {
            return None;
        }
        Some(Linear::new(key))
    }

    async fn query(&self, query: &str, variables: Value) -> anyhow::Result<Value> {
        let resp = self
            .http
            .post(API_URL)
            .header("Authorization", &self.key)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({ "query": query, "variables": variables }))
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow!("linear {status}: {body}"));
        }
        let value: Value = serde_json::from_str(&body)?;
        if let Some(message) = value
            .get("errors")
            .and_then(|e| e.as_array())
            .and_then(|errors| errors.first())
            .and_then(|error| error.get("message"))
            .and_then(|m| m.as_str())
        {
            return Err(anyhow!("linear: {message}"));
        }
        Ok(value)
    }

    pub async fn issue(&self, identifier: &str) -> anyhow::Result<Issue> {
        let query = "query($id: String!) { issue(id: $id) { id identifier title description url team { id } } }";
        let value = self
            .query(query, serde_json::json!({ "id": identifier }))
            .await?;
        parse_issue_response(&value)
    }

    pub async fn states(&self, team_id: &str) -> anyhow::Result<Vec<WorkflowState>> {
        let query =
            "query($teamId: String!) { team(id: $teamId) { states { nodes { id name type } } } }";
        let value = self
            .query(query, serde_json::json!({ "teamId": team_id }))
            .await?;
        let nodes = value
            .pointer("/data/team/states/nodes")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow!("linear: missing team states in response"))?;
        nodes
            .iter()
            .map(|node| {
                Ok(WorkflowState {
                    id: field_str(node, "id")?,
                    name: field_str(node, "name")?,
                    kind: field_str(node, "type")?,
                })
            })
            .collect()
    }

    /// Issues of team `team_key` (e.g. "ENG") whose workflow state is named
    /// `state_name`, oldest first.
    pub async fn issues_in_state(
        &self,
        team_key: &str,
        state_name: &str,
    ) -> anyhow::Result<Vec<Issue>> {
        let query = "query($team: String!, $state: String!) { issues(filter: { team: { key: { eq: $team } }, state: { name: { eq: $state } } }, first: 100, orderBy: createdAt) { nodes { id identifier title description url team { id } } } }";
        let value = self
            .query(
                query,
                serde_json::json!({ "team": team_key, "state": state_name }),
            )
            .await?;
        parse_issue_list(&value)
    }

    pub async fn move_issue(&self, issue_id: &str, state_id: &str) -> anyhow::Result<()> {
        let query = "mutation($id: String!, $stateId: String!) { issueUpdate(id: $id, input: { stateId: $stateId }) { success } }";
        let value = self
            .query(
                query,
                serde_json::json!({ "id": issue_id, "stateId": state_id }),
            )
            .await?;
        require_success(&value, "/data/issueUpdate/success")
    }

    pub async fn comment(&self, issue_id: &str, body: &str) -> anyhow::Result<()> {
        let query = "mutation($issueId: String!, $body: String!) { commentCreate(input: { issueId: $issueId, body: $body }) { success } }";
        let value = self
            .query(
                query,
                serde_json::json!({ "issueId": issue_id, "body": body }),
            )
            .await?;
        require_success(&value, "/data/commentCreate/success")
    }
}

fn field_str(value: &Value, field: &str) -> anyhow::Result<String> {
    value
        .get(field)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("linear: missing field {field} in response"))
}

fn require_success(value: &Value, pointer: &str) -> anyhow::Result<()> {
    if value.pointer(pointer).and_then(|v| v.as_bool()) == Some(true) {
        Ok(())
    } else {
        Err(anyhow!("linear: mutation failed"))
    }
}

fn parse_issue_response(value: &Value) -> anyhow::Result<Issue> {
    let issue = value
        .pointer("/data/issue")
        .ok_or_else(|| anyhow!("linear: missing issue in response"))?;
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
    nodes
        .iter()
        .map(|issue| {
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
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_issue_accepts_identifier() {
        assert_eq!(parse_issue("ENG-123"), Some("ENG-123".to_string()));
    }

    #[test]
    fn parse_issue_accepts_url() {
        assert_eq!(
            parse_issue("https://linear.app/w/issue/ENG-123"),
            Some("ENG-123".to_string())
        );
    }

    #[test]
    fn parse_issue_accepts_url_with_trailing_path() {
        assert_eq!(
            parse_issue("https://linear.app/w/issue/ENG-123/some-title"),
            Some("ENG-123".to_string())
        );
    }

    #[test]
    fn parse_issue_rejects_lowercase() {
        assert_eq!(parse_issue("eng-1"), None);
    }

    #[test]
    fn parse_issue_rejects_url_without_identifier() {
        assert_eq!(parse_issue("https://linear.app/w/issue/"), None);
    }

    #[test]
    fn parse_issue_rejects_empty() {
        assert_eq!(parse_issue(""), None);
    }

    fn sample_states() -> Vec<WorkflowState> {
        vec![
            WorkflowState {
                id: "1".into(),
                name: "Backlog".into(),
                kind: "backlog".into(),
            },
            WorkflowState {
                id: "2".into(),
                name: "In Progress".into(),
                kind: "started".into(),
            },
            WorkflowState {
                id: "3".into(),
                name: "Done".into(),
                kind: "completed".into(),
            },
        ]
    }

    #[test]
    fn pick_state_started() {
        let states = sample_states();
        assert_eq!(
            pick_state(&states, Target::Started),
            Some(states[1].clone())
        );
    }

    #[test]
    fn pick_state_completed() {
        let states = sample_states();
        assert_eq!(
            pick_state(&states, Target::Completed),
            Some(states[2].clone())
        );
    }

    #[test]
    fn pick_state_in_review_missing_is_none() {
        let states = sample_states();
        assert_eq!(pick_state(&states, Target::InReview), None);
    }

    #[test]
    fn pick_state_in_review_case_insensitive() {
        let mut states = sample_states();
        states.push(WorkflowState {
            id: "4".into(),
            name: "In Review".into(),
            kind: "started".into(),
        });
        assert_eq!(
            pick_state(&states, Target::InReview),
            Some(states[3].clone())
        );
    }

    #[test]
    fn parse_issue_response_with_null_description() {
        let value = json!({
            "data": {
                "issue": {
                    "id": "abc",
                    "identifier": "ENG-123",
                    "title": "Title",
                    "description": null,
                    "url": "https://linear.app/w/issue/ENG-123",
                    "team": { "id": "team-1" }
                }
            }
        });
        let issue = parse_issue_response(&value).unwrap();
        assert_eq!(
            issue,
            Issue {
                id: "abc".into(),
                identifier: "ENG-123".into(),
                title: "Title".into(),
                description: String::new(),
                url: "https://linear.app/w/issue/ENG-123".into(),
                team_id: "team-1".into(),
            }
        );
    }

    #[test]
    fn parse_issue_list_parses_nodes_with_null_description() {
        let value = json!({
            "data": {
                "issues": {
                    "nodes": [
                        {
                            "id": "1",
                            "identifier": "ENG-1",
                            "title": "First",
                            "description": "has body",
                            "url": "https://linear.app/w/issue/ENG-1",
                            "team": { "id": "team-1" }
                        },
                        {
                            "id": "2",
                            "identifier": "ENG-2",
                            "title": "Second",
                            "description": null,
                            "url": "https://linear.app/w/issue/ENG-2",
                            "team": { "id": "team-1" }
                        }
                    ]
                }
            }
        });
        let issues = parse_issue_list(&value).unwrap();
        assert_eq!(
            issues,
            vec![
                Issue {
                    id: "1".into(),
                    identifier: "ENG-1".into(),
                    title: "First".into(),
                    description: "has body".into(),
                    url: "https://linear.app/w/issue/ENG-1".into(),
                    team_id: "team-1".into(),
                },
                Issue {
                    id: "2".into(),
                    identifier: "ENG-2".into(),
                    title: "Second".into(),
                    description: String::new(),
                    url: "https://linear.app/w/issue/ENG-2".into(),
                    team_id: "team-1".into(),
                },
            ]
        );
    }

    #[test]
    fn parse_issue_list_missing_issues_is_error() {
        let value = json!({ "data": {} });
        assert!(parse_issue_list(&value).is_err());
    }
}
