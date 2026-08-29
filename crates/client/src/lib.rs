//! Reusable client for the orchestrator's `/api` routes: JSON-over-HTTP plus
//! the task events WebSocket. This is the library form of `crates/cli`'s
//! `http.rs` + `run.rs`, for other Rust frontends (e.g. the desktop app).

use std::sync::Arc;

use futures_util::StreamExt;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::CertificateDer;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

#[derive(Clone)]
pub struct Client {
    base: String,
    token: String,
    http: reqwest::Client,
    connector: Option<Connector>,
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

#[derive(Serialize)]
struct FromIssue<'a> {
    issue: &'a str,
    base_branch: &'a str,
    executor: lgtm_protocol::Executor,
    worker: Option<&'a str>,
}

/// Body of `POST /api/batches`.
#[derive(Serialize, Clone, Debug)]
pub struct BatchRequest {
    pub source: lgtm_protocol::BatchSource,
    pub repository: Option<String>,
    pub base_branch: String,
    pub executor: lgtm_protocol::Executor,
    pub worker: Option<String>,
    pub plan: bool,
    pub approve_plans: bool,
    pub max: u32,
    pub dry_run: bool,
}

/// One issue found for a batch, previewed before (or instead of) import.
#[derive(Deserialize, Clone, Debug)]
pub struct IssuePreview {
    pub key: String,
    pub title: String,
    pub url: String,
}

/// Body of `POST /api/batches`.
#[derive(Deserialize, Clone, Debug)]
pub struct BatchResponse {
    pub batch: Option<lgtm_protocol::Batch>,
    pub issues: Vec<IssuePreview>,
}

/// Body of `GET /api/batches/:id`.
#[derive(Deserialize, Clone, Debug)]
pub struct BatchDetail {
    pub batch: lgtm_protocol::Batch,
    pub summary: lgtm_protocol::BatchSummary,
    pub tasks: Vec<lgtm_protocol::Task>,
}

#[derive(Serialize)]
struct FromLinear<'a> {
    issue: &'a str,
    repository: &'a str,
    base_branch: &'a str,
    executor: lgtm_protocol::Executor,
    worker: Option<&'a str>,
}

impl Client {
    pub fn new(orchestrator: impl Into<String>, token: impl Into<String>) -> Self {
        Client {
            // Trimmed so URLs are always built by appending a path starting with "/".
            base: orchestrator.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http: reqwest::Client::new(),
            connector: None,
        }
    }

    /// Like [`Client::new`], but trusts `ca_pem` (a PEM-encoded CA certificate)
    /// in addition to the platform's webpki roots, for both the HTTP client
    /// and the events WebSocket.
    pub fn with_ca(
        orchestrator: impl Into<String>,
        token: impl Into<String>,
        ca_pem: &[u8],
    ) -> anyhow::Result<Self> {
        // `reqwest::Certificate::from_pem` and rustls's own PEM iterator both
        // parse leniently and quietly accept zero certificates, so garbage
        // input has to be rejected here instead.
        let certs: Vec<CertificateDer<'static>> =
            CertificateDer::pem_slice_iter(ca_pem).collect::<Result<_, _>>()?;
        anyhow::ensure!(!certs.is_empty(), "no certificate found in CA PEM");

        let http = reqwest::Client::builder()
            .add_root_certificate(reqwest::Certificate::from_pem(ca_pem)?)
            .build()?;

        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for cert in certs {
            roots.add(cert)?;
        }

        // Mirrors reqwest's own fallback (it can't rely on a process-wide
        // default having been installed either): reuse one if present,
        // otherwise construct ring's provider directly.
        let provider = rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .unwrap_or_else(|| Arc::new(rustls::crypto::ring::default_provider()));
        let tls_config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();

        Ok(Client {
            base: orchestrator.into().trim_end_matches('/').to_string(),
            token: token.into(),
            http,
            connector: Some(Connector::Rustls(Arc::new(tls_config))),
        })
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

    pub async fn merge(&self, id: &str) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(&format!("/api/tasks/{id}/merge"), None::<&()>)
            .await
    }

    pub async fn create_task_from_issue(
        &self,
        issue: &str,
        base_branch: &str,
        executor: lgtm_protocol::Executor,
        worker: Option<&str>,
    ) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(
            "/api/tasks/from-issue",
            Some(&FromIssue {
                issue,
                base_branch,
                executor,
                worker,
            }),
        )
        .await
    }

    pub async fn create_task_from_linear(
        &self,
        issue: &str,
        repository: &str,
        base_branch: &str,
        executor: lgtm_protocol::Executor,
        worker: Option<&str>,
    ) -> anyhow::Result<lgtm_protocol::Task> {
        self.post(
            "/api/tasks/from-linear",
            Some(&FromLinear {
                issue,
                repository,
                base_branch,
                executor,
                worker,
            }),
        )
        .await
    }

    pub async fn create_batch(&self, req: &BatchRequest) -> anyhow::Result<BatchResponse> {
        self.post("/api/batches", Some(req)).await
    }

    pub async fn batches(&self) -> anyhow::Result<Vec<lgtm_protocol::Batch>> {
        self.get("/api/batches").await
    }

    pub async fn batch(&self, id: &str) -> anyhow::Result<BatchDetail> {
        self.get(&format!("/api/batches/{id}")).await
    }

    /// Opens the events socket from event index `from` and returns a stream
    /// of stored events that ends when the server closes it.
    pub async fn events(&self, id: &str, from: usize) -> anyhow::Result<EventStream> {
        let mut request = self.events_url(id, from)?.into_client_request()?;
        request.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", self.token))?,
        );
        let (stream, _) = tokio_tungstenite::connect_async_tls_with_config(
            request,
            None,
            false,
            self.connector.clone(),
        )
        .await?;
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

    #[test]
    fn with_ca_rejects_garbage_pem() {
        assert!(Client::with_ca("http://127.0.0.1:4750", "tok", b"not a cert").is_err());
    }

    #[test]
    fn with_ca_accepts_valid_pem() {
        const CA_PEM: &str = "-----BEGIN CERTIFICATE-----
MIIDBTCCAe2gAwIBAgIUUjUIP0qEa6ELPvnHZFrTQRFtRL4wDQYJKoZIhvcNAQEL
BQAwEjEQMA4GA1UEAwwHdGVzdC1jYTAeFw0yNjA4MjgxNTM3MjZaFw0zNjA4MjUx
NTM3MjZaMBIxEDAOBgNVBAMMB3Rlc3QtY2EwggEiMA0GCSqGSIb3DQEBAQUAA4IB
DwAwggEKAoIBAQDqzDjLDMeIstJ2P6X0YZjIaE0TFGOQzOu/cC6JaJAl34mYpnhx
4iDDdVnRRpGJA4Gw1k0un3CS7pVXzP5zQ09YX4a7OU45DgmUop4qsQ+wEFiraSbh
Kr8IbOOUYPAAPWoBYVfD5UU7AadjHDpMSZJGYRs9xjVHSq1OkgSPuOtt7+Q8LmTG
16e9+AiRcGP2fGn3nDs9QnBjo0hVmFIGyqk2VNs85gHcRyJUvMe+fRmSgKoe1wHJ
LiLo6JxnC10ouPu1LfWaOqlI1kxaO0hrdb1MVIlFX9uJ0tGe1HvLLyxPKw6UMlQH
Q8l5U68nMHakBxS+phAGq94EepdVbVkKGKrdAgMBAAGjUzBRMB0GA1UdDgQWBBSI
e4KzmpkVNuoL3YMqPC6Kkrx0hDAfBgNVHSMEGDAWgBSIe4KzmpkVNuoL3YMqPC6K
krx0hDAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQAQNHoO3sAd
fhjKMarwb62f31EyVQjwQ4mOxwTdkGYht/BDl20QRG6I8Tr7w85IPiVbEBOVLD8j
kjGbx3fExY0mU3AOaNnlkHdJHxRb9Z1mw2C5ft1TJ6xzK7WtfDz5FGtXzBixXnGh
DVR02ANTzuuwrtjR4jAtN3wy/MxYrFKFNSPXg73CwY8lXQJOSa79E93GMaq4bzIN
nnBzN0YNELYH6eqRHIfgnaDK25gpDEjsTTwXLPUhC7gyPzNtssmfrvK560WaZVkz
3j3ezdbBUxQcZZigtPfO48ZAbeFhEXtscfRA59144WP6hHUIhD/x6ghw8GBvOi41
etSSXseWw5Zl
-----END CERTIFICATE-----
";
        let client = Client::with_ca("https://127.0.0.1:4750", "tok", CA_PEM.as_bytes()).unwrap();
        assert!(matches!(client.connector, Some(Connector::Rustls(_))));
    }
}
