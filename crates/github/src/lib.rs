//! Minimal GitHub REST client: issue lookup, pull request creation and
//! merging, and check-run aggregation for CI status.

use std::process::Command;

use anyhow::{anyhow, Context};
use lgtm_protocol::{CiState, CiStatus, PullRequest};
use serde::Deserialize;

const API_BASE: &str = "https://api.github.com";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub repo: String,
}

/// Parses `https://github.com/o/r`, `https://github.com/o/r.git`, and
/// `git@github.com:o/r.git`. Anything else, including other hosts, is `None`.
pub fn parse_repo(url: &str) -> Option<Repo> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))?;
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, repo) = rest.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some(Repo {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// Parses `https://github.com/o/r/issues/N`, `o/r#N`, and `github:o/r#N`.
pub fn parse_issue(s: &str) -> Option<(Repo, u64)> {
    if let Some(rest) = s.strip_prefix("https://github.com/") {
        let mut parts = rest.splitn(4, '/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        let issues = parts.next()?;
        let number = parts.next()?;
        if issues != "issues" || owner.is_empty() || repo.is_empty() {
            return None;
        }
        return Some((
            Repo {
                owner: owner.to_string(),
                repo: repo.to_string(),
            },
            number.parse().ok()?,
        ));
    }
    let rest = s.strip_prefix("github:").unwrap_or(s);
    let (repo_part, number_part) = rest.split_once('#')?;
    let (owner, repo) = repo_part.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    Some((
        Repo {
            owner: owner.to_string(),
            repo: repo.to_string(),
        },
        number_part.parse().ok()?,
    ))
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
            http: reqwest::Client::new(),
        }
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
        self.http
            .request(method, format!("{API_BASE}{path}"))
            .bearer_auth(&self.token)
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

    pub async fn create_pull(
        &self,
        repo: &Repo,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> anyhow::Result<PullRequest> {
        let path = format!("/repos/{}/{}/pulls", repo.owner, repo.repo);
        let req = self
            .request(reqwest::Method::POST, &path)
            .json(&serde_json::json!({
                "title": title,
                "body": body,
                "head": head,
                "base": base,
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

const FAILING_CONCLUSIONS: [&str; 4] = ["failure", "timed_out", "cancelled", "action_required"];

/// Pure aggregation used by [`GitHub::checks`]: no runs, or any run not yet
/// `completed`, is `Pending`; any completed run with a failing conclusion is
/// `Failure`; otherwise `Success`.
pub fn aggregate_checks(runs: &[serde_json::Value], fallback_url: &str) -> CiStatus {
    let html_url = |run: &serde_json::Value| -> String {
        run.get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    fn status(run: &serde_json::Value) -> &str {
        run.get("status").and_then(|v| v.as_str()).unwrap_or("")
    }
    fn conclusion(run: &serde_json::Value) -> &str {
        run.get("conclusion").and_then(|v| v.as_str()).unwrap_or("")
    }

    if runs.is_empty() {
        return CiStatus {
            state: CiState::Pending,
            url: fallback_url.to_string(),
        };
    }

    let first_url = html_url(&runs[0]);

    if runs.iter().any(|run| status(run) != "completed") {
        return CiStatus {
            state: CiState::Pending,
            url: first_url,
        };
    }

    if let Some(failing) = runs
        .iter()
        .find(|run| FAILING_CONCLUSIONS.contains(&conclusion(run)))
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
