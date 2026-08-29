//! Thin JSON-over-HTTP client for the orchestrator's `/api` routes.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::path::Path;

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
    /// `ca` is an extra PEM root certificate to trust, for orchestrators
    /// serving a self-signed cert (`--ca` / `LGTM_CA`).
    pub fn new(base: String, token: String, ca: Option<&Path>) -> anyhow::Result<Self> {
        let mut builder = reqwest::Client::builder();
        if let Some(path) = ca {
            let pem = std::fs::read(path)
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(&pem)?);
        }
        Ok(Client {
            // Trimmed so callers can always write paths starting with "/".
            base: base.trim_end_matches('/').to_string(),
            token,
            http: builder.build()?,
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| self.unreachable(e))?;
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
        let resp = req.send().await.map_err(|e| self.unreachable(e))?;
        Self::handle(resp).await
    }

    /// A refused connection means the orchestrator isn't there, which is worth
    /// saying plainly instead of passing reqwest's wording through.
    fn unreachable(&self, e: reqwest::Error) -> anyhow::Error {
        if e.is_connect() {
            return anyhow::anyhow!(
                "cannot reach {}: run `lgtm serve` first, or pass --orchestrator",
                self.base
            );
        }
        e.into()
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
