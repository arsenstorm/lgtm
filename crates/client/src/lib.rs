//! Reusable client for the orchestrator's `/api` routes: JSON-over-HTTP plus
//! the task events WebSocket. This is the library form of `crates/cli`'s
//! `http.rs` + `run.rs`, for other Rust frontends (e.g. the desktop app).

use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

#[derive(Clone)]
pub struct Client {
    base: String,
    token: String,
    http: reqwest::Client,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

/// Body of `GET /api/tasks/:id`.
#[derive(serde::Deserialize, Clone, Debug)]
pub struct TaskDetail {
    pub task: lgtm_protocol::Task,
    pub events: Vec<lgtm_protocol::StoredEvent>,
}

#[derive(Serialize)]
struct FollowUp<'a> {
    text: &'a str,
}

impl Client {
    pub fn new(orchestrator: impl Into<String>, token: impl Into<String>) -> Self {
        Client {
            // Trimmed so URLs are always built by appending a path starting with "/".
            base: orchestrator.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http: reqwest::Client::new(),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?;
        Self::handle(resp).await
    }

    async fn post<T: DeserializeOwned>(
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

    pub async fn workers(&self) -> anyhow::Result<Vec<lgtm_protocol::WorkerStatus>> {
        self.get("/api/workers").await
    }

    pub async fn tasks(&self) -> anyhow::Result<Vec<lgtm_protocol::Task>> {
        self.get("/api/tasks").await
    }

    pub async fn task(&self, id: &str) -> anyhow::Result<TaskDetail> {
        self.get(&format!("/api/tasks/{id}")).await
    }

    pub async fn create_task(
        &self,
        spec: &lgtm_protocol::TaskSpec,
    ) -> anyhow::Result<lgtm_protocol::Task> {
        self.post("/api/tasks", Some(spec)).await
    }

    pub async fn cancel(&self, id: &str) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(&format!("/api/tasks/{id}/cancel"), None::<&()>)
            .await
    }

    pub async fn approve(&self, id: &str) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(&format!("/api/tasks/{id}/approve"), None::<&()>)
            .await
    }

    pub async fn reject(&self, id: &str) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(&format!("/api/tasks/{id}/reject"), None::<&()>)
            .await
    }

    pub async fn tell(&self, id: &str, text: &str) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(
            &format!("/api/tasks/{id}/message"),
            Some(&FollowUp { text }),
        )
        .await
    }

    /// Opens the events socket from event index `from` and returns a stream
    /// of stored events that ends when the server closes it.
    pub async fn events(&self, id: &str, from: usize) -> anyhow::Result<EventStream> {
        let mut request = self.events_url(id, from)?.into_client_request()?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.token))?,
        );
        let (stream, _) = tokio_tungstenite::connect_async(request).await?;
        Ok(EventStream { stream })
    }

    /// `ws(s)://host/api/tasks/<id>/events[?from=n]` (query omitted when 0).
    pub fn events_url(&self, id: &str, from: usize) -> anyhow::Result<String> {
        let (scheme, rest) = if let Some(rest) = self.base.strip_prefix("https://") {
            ("wss://", rest)
        } else if let Some(rest) = self.base.strip_prefix("http://") {
            ("ws://", rest)
        } else {
            anyhow::bail!("orchestrator URL must start with http:// or https://");
        };
        if from == 0 {
            Ok(format!("{scheme}{rest}/api/tasks/{id}/events"))
        } else {
            Ok(format!("{scheme}{rest}/api/tasks/{id}/events?from={from}"))
        }
    }
}

pub struct EventStream {
    stream: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl EventStream {
    /// Next stored event; `None` when the socket closed or errored. Non-text
    /// frames are skipped; a text frame that fails to parse is skipped with
    /// no error.
    pub async fn next(&mut self) -> Option<lgtm_protocol::StoredEvent> {
        while let Some(msg) = self.stream.next().await {
            let Message::Text(text) = msg.ok()? else {
                continue;
            };
            if let Ok(stored) = serde_json::from_str(&text) {
                return Some(stored);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_url_from_zero_has_no_query() {
        let client = Client::new("http://127.0.0.1:4750", "tok");
        assert_eq!(
            client.events_url("abc", 0).unwrap(),
            "ws://127.0.0.1:4750/api/tasks/abc/events"
        );
    }

    #[test]
    fn events_url_from_nonzero_appends_query() {
        let client = Client::new("http://127.0.0.1:4750", "tok");
        assert_eq!(
            client.events_url("abc", 7).unwrap(),
            "ws://127.0.0.1:4750/api/tasks/abc/events?from=7"
        );
    }

    #[test]
    fn events_url_https_becomes_wss() {
        let client = Client::new("https://example.com", "tok");
        assert_eq!(
            client.events_url("abc", 0).unwrap(),
            "wss://example.com/api/tasks/abc/events"
        );
    }

    #[test]
    fn events_url_rejects_non_http_scheme() {
        let client = Client::new("ftp://example.com", "tok");
        assert!(client.events_url("abc", 0).is_err());
    }
}
