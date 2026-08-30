//! Minimal GitHub REST client: issue lookup, pull request creation and
//! merging, check-run aggregation for CI status, and pull request reviews.

mod app;
mod checks;
mod refs;
mod reviews;

use std::process::Command;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use lgtm_protocol::{CiStatus, PrReview, PullRequest};
use serde::Deserialize;

pub use app::{jwt, GithubApp};
pub use checks::aggregate_checks;
pub use refs::{parse_issue, parse_repo, Repo};
pub use reviews::aggregate_reviews;

const API_BASE: &str = "https://api.github.com";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewPull {
    pub repo: Repo,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub html_url: String,
}

#[derive(Clone)]
pub struct GitHub {
    token: String,
    /// `None` unless a GitHub App is configured, which is what lets a push
    /// carry a token scoped to the one repository instead of this one.
    app: Option<Arc<GithubApp>>,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: String,
}

#[derive(Deserialize)]
struct IssueResponse {
    number: u64,
    title: String,
    body: Option<String>,
    html_url: String,
}

#[derive(Deserialize)]
struct PullResponse {
    number: u64,
    html_url: String,
}

#[derive(Deserialize)]
struct MergeableResponse {
    mergeable: Option<bool>,
}

impl GitHub {
    pub fn new(token: impl Into<String>) -> Self {
        GitHub {
            token: token.into(),
            app: None,
            http: reqwest::Client::new(),
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// `GITHUB_TOKEN`, else the stdout of `gh auth token`, else `None`.
    pub fn from_env() -> Option<Self> {
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                return Some(GitHub::new(token));
            }
        }
        let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
        let token = String::from_utf8(output.stdout).ok()?;
        let token = token.trim();
        if token.is_empty() {
            return None;
        }
        Some(GitHub::new(token))
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.request_as(method, path, &self.token)
    }

    fn request_as(
        &self,
        method: reqwest::Method,
        path: &str,
        auth: &str,
    ) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{API_BASE}{path}"))
            .bearer_auth(auth)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "lgtm")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    async fn send_json<T: for<'de> Deserialize<'de>>(
        req: reqwest::RequestBuilder,
    ) -> anyhow::Result<T> {
        let resp = req.send().await.context("sending github request")?;
        let status = resp.status();
        let body = resp.text().await.context("reading github response body")?;
        if !status.is_success() {
            let message = serde_json::from_str::<ErrorBody>(&body)
                .map(|e| e.message)
                .unwrap_or(body);
            return Err(anyhow!("github {status}: {message}"));
        }
        serde_json::from_str(&body).context("parsing github response body")
    }

    pub async fn issue(&self, repo: &Repo, number: u64) -> anyhow::Result<Issue> {
        let path = format!("/repos/{}/{}/issues/{number}", repo.owner, repo.repo);
        let resp: IssueResponse =
            Self::send_json(self.request(reqwest::Method::GET, &path)).await?;
        Ok(Issue {
            number: resp.number,
            title: resp.title,
            body: resp.body.unwrap_or_default(),
            html_url: resp.html_url,
        })
    }

    /// Open issues carrying `label`, oldest first, pull requests excluded.
    pub async fn issues_with_label(&self, repo: &Repo, label: &str) -> anyhow::Result<Vec<Issue>> {
        // Only spaces are escaped; labels with other reserved characters
        // (e.g. `#`, `&`) would need full percent-encoding.
        let label = label.replace(' ', "%20");
        let path = format!(
            "/repos/{}/{}/issues?labels={label}&state=open&per_page=100&sort=created&direction=asc",
            repo.owner, repo.repo
        );
        let items: Vec<serde_json::Value> =
            Self::send_json(self.request(reqwest::Method::GET, &path)).await?;
        Ok(parse_issue_list(&items))
    }

    pub async fn create_pull(&self, pull: &NewPull) -> anyhow::Result<PullRequest> {
        let path = format!("/repos/{}/{}/pulls", pull.repo.owner, pull.repo.repo);
        let req = self
            .request(reqwest::Method::POST, &path)
            .json(&serde_json::json!({
                "title": pull.title,
                "body": pull.body,
                "head": pull.head,
                "base": pull.base,
            }));
        let resp: PullResponse = Self::send_json(req).await?;
        Ok(PullRequest {
            number: resp.number,
            url: resp.html_url,
        })
    }

    /// Aggregates check runs for `sha` into a single [`CiStatus`].
    pub async fn checks(&self, repo: &Repo, sha: &str) -> anyhow::Result<CiStatus> {
        let path = format!(
            "/repos/{}/{}/commits/{sha}/check-runs",
            repo.owner, repo.repo
        );
        let resp: serde_json::Value =
            Self::send_json(self.request(reqwest::Method::GET, &path)).await?;
        let runs = resp
            .get("check_runs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let fallback_url = format!(
            "https://github.com/{}/{}/commit/{sha}",
            repo.owner, repo.repo
        );
        Ok(aggregate_checks(&runs, &fallback_url))
    }

    /// The pull request's human review state: [`aggregate_reviews`].
    pub async fn pull_reviews(&self, repo: &Repo, number: u64) -> anyhow::Result<Option<PrReview>> {
        let path = format!("/repos/{}/{}/pulls/{number}/reviews", repo.owner, repo.repo);
        let reviews: Vec<serde_json::Value> =
            Self::send_json(self.request(reqwest::Method::GET, &path)).await?;
        Ok(aggregate_reviews(&reviews))
    }

    /// `mergeable` field of the pull request, `None` when GitHub hasn't
    /// computed it yet.
    pub async fn pull_mergeable(&self, repo: &Repo, number: u64) -> anyhow::Result<Option<bool>> {
        let path = format!("/repos/{}/{}/pulls/{number}", repo.owner, repo.repo);
        let resp: MergeableResponse =
            Self::send_json(self.request(reqwest::Method::GET, &path)).await?;
        Ok(resp.mergeable)
    }

    pub async fn merge_pull(&self, repo: &Repo, number: u64) -> anyhow::Result<()> {
        let path = format!("/repos/{}/{}/pulls/{number}/merge", repo.owner, repo.repo);
        let req = self
            .request(reqwest::Method::PUT, &path)
            .json(&serde_json::json!({ "merge_method": "squash" }));
        let _: serde_json::Value = Self::send_json(req).await?;
        Ok(())
    }
}

/// Pure half of [`GitHub::issues_with_label`]: drops entries carrying a
/// `pull_request` key; a null `body` becomes `""`.
pub fn parse_issue_list(items: &[serde_json::Value]) -> Vec<Issue> {
    items
        .iter()
        .filter(|item| item.get("pull_request").is_none())
        .filter_map(|item| {
            Some(Issue {
                number: item.get("number")?.as_u64()?,
                title: item.get("title")?.as_str()?.to_string(),
                body: item
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                html_url: item.get("html_url")?.as_str()?.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::CiState;
    use serde_json::json;

    #[test]
    fn parse_repo_accepts_https() {
        assert_eq!(
            parse_repo("https://github.com/o/r"),
            Some(Repo {
                owner: "o".into(),
                repo: "r".into()
            })
        );
    }

    #[test]
    fn parse_repo_accepts_https_with_git_suffix() {
        assert_eq!(
            parse_repo("https://github.com/o/r.git"),
            Some(Repo {
                owner: "o".into(),
                repo: "r".into()
            })
        );
    }

    #[test]
    fn parse_repo_accepts_ssh() {
        assert_eq!(
            parse_repo("git@github.com:o/r.git"),
            Some(Repo {
                owner: "o".into(),
                repo: "r".into()
            })
        );
    }

    #[test]
    fn parse_repo_rejects_other_hosts_and_empty() {
        assert_eq!(parse_repo("https://gitlab.com/o/r"), None);
        assert_eq!(parse_repo(""), None);
    }

    #[test]
    fn parse_issue_accepts_https() {
        assert_eq!(
            parse_issue("https://github.com/o/r/issues/7"),
            Some((
                Repo {
                    owner: "o".into(),
                    repo: "r".into()
                },
                7
            ))
        );
    }

    #[test]
    fn parse_issue_accepts_shorthand() {
        assert_eq!(
            parse_issue("o/r#7"),
            Some((
                Repo {
                    owner: "o".into(),
                    repo: "r".into()
                },
                7
            ))
        );
    }

    #[test]
    fn parse_issue_accepts_github_prefix() {
        assert_eq!(
            parse_issue("github:o/r#7"),
            Some((
                Repo {
                    owner: "o".into(),
                    repo: "r".into()
                },
                7
            ))
        );
    }

    #[test]
    fn parse_issue_rejects_non_match() {
        assert_eq!(parse_issue("not an issue reference"), None);
    }

    #[test]
    fn aggregate_checks_empty_is_pending_with_fallback() {
        let status = aggregate_checks(&[], "https://github.com/o/r/commit/abc");
        assert_eq!(status.state, CiState::Pending);
        assert_eq!(status.url, "https://github.com/o/r/commit/abc");
    }

    #[test]
    fn aggregate_checks_in_progress_plus_success_is_pending() {
        let runs = vec![
            json!({"status": "in_progress", "conclusion": null, "html_url": "u1"}),
            json!({"status": "completed", "conclusion": "success", "html_url": "u2"}),
        ];
        let status = aggregate_checks(&runs, "fallback");
        assert_eq!(status.state, CiState::Pending);
    }

    #[test]
    fn aggregate_checks_all_success_uses_first_run_url() {
        let runs = vec![
            json!({"status": "completed", "conclusion": "success", "html_url": "u1"}),
            json!({"status": "completed", "conclusion": "success", "html_url": "u2"}),
        ];
        let status = aggregate_checks(&runs, "fallback");
        assert_eq!(status.state, CiState::Success);
        assert_eq!(status.url, "u1");
    }

    #[test]
    fn aggregate_checks_one_failure_uses_failing_run_url() {
        let runs = vec![
            json!({"status": "completed", "conclusion": "success", "html_url": "u1"}),
            json!({"status": "completed", "conclusion": "failure", "html_url": "u2"}),
        ];
        let status = aggregate_checks(&runs, "fallback");
        assert_eq!(status.state, CiState::Failure);
        assert_eq!(status.url, "u2");
    }

    #[test]
    fn parse_issue_list_drops_prs_and_defaults_null_body() {
        let items = vec![
            json!({
                "number": 1,
                "title": "first issue",
                "body": "has a body",
                "html_url": "https://github.com/o/r/issues/1",
            }),
            json!({
                "number": 2,
                "title": "a pull request",
                "body": "not an issue",
                "html_url": "https://github.com/o/r/pull/2",
                "pull_request": {"url": "https://api.github.com/repos/o/r/pulls/2"},
            }),
            json!({
                "number": 3,
                "title": "third issue",
                "body": null,
                "html_url": "https://github.com/o/r/issues/3",
            }),
        ];
        let issues = parse_issue_list(&items);
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].number, 1);
        assert_eq!(issues[0].title, "first issue");
        assert_eq!(issues[0].html_url, "https://github.com/o/r/issues/1");
        assert_eq!(issues[0].body, "has a body");
        assert_eq!(issues[1].number, 3);
        assert_eq!(issues[1].title, "third issue");
        assert_eq!(issues[1].html_url, "https://github.com/o/r/issues/3");
        assert_eq!(issues[1].body, "");
    }
}
