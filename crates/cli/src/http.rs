//! Thin JSON-over-HTTP client for the orchestrator's `/api` routes.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub struct Client {
    base: String,
    token: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

impl Client {
    pub fn new(base: String, token: String) -> Self {
        Client {
            // Trimmed so callers can always write paths starting with "/".
            base: base.trim_end_matches('/').to_string(),
            token,
            http: reqwest::Client::new(),
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?;
        Self::handle(resp).await
    }

    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> anyhow::Result<T> {
        let mut req = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await?;
        Self::handle(resp).await
    }

    async fn handle<T: DeserializeOwned>(resp: reqwest::Response) -> anyhow::Result<T> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp.json().await?);
        }
        let body = resp.text().await.unwrap_or_default();
        if let Ok(err) = serde_json::from_str::<ErrorBody>(&body) {
            anyhow::bail!("{status}: {}", err.error);
        }
        anyhow::bail!("{status}: {body}");
    }
}
