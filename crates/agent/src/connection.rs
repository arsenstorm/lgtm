//! Outbound WebSocket connection to the orchestrator, with reconnect.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::{SinkExt, StreamExt};
use lgtm_protocol::{
    OrchestratorMessage, TaskEvent, TaskId, WorkerInfo, WorkerMessage, PROTOCOL_VERSION,
};
use rustls_pki_types::{pem::PemObject, CertificateDer};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::Connector;

use crate::runner;

const RETRY: Duration = Duration::from_secs(3);
/// Time the writer gets to put `Goodbye` on the wire before the process exits.
const FLUSH: Duration = Duration::from_secs(1);
// The orchestrator pings every 15s; silence this long means it is gone.
const READ_TIMEOUT: Duration = Duration::from_secs(45);

/// Shared state every task runner needs.
pub struct Ctx {
    pub data_dir: PathBuf,
    pub tx: mpsc::UnboundedSender<WorkerMessage>,
    pub running: Mutex<HashMap<TaskId, oneshot::Sender<()>>>,
    /// Mirror path per task, so a discard after a restart of the task still
    /// knows which bare clone owns the worktree.
    pub mirrors: Mutex<HashMap<TaskId, PathBuf>>,
    /// Exit once `max_tasks` runs have ended and nothing is running.
    pub ephemeral: bool,
    pub max_tasks: u32,
    /// Runs that ended in Completed, Failed or Cancelled.
    pub finished: AtomicU32,
    /// Set once, so the goodbye is sent at most once.
    exiting: AtomicBool,
}

impl Ctx {
    pub fn new(
        data_dir: PathBuf,
        tx: mpsc::UnboundedSender<WorkerMessage>,
        ephemeral: bool,
        max_tasks: u32,
    ) -> Self {
        Self {
            data_dir,
            tx,
            running: Mutex::new(HashMap::new()),
            mirrors: Mutex::new(HashMap::new()),
            ephemeral,
            max_tasks,
            finished: AtomicU32::new(0),
            exiting: AtomicBool::new(false),
        }
    }

    pub fn emit(&self, task_id: &str, event: TaskEvent) {
        let _ = self.tx.send(WorkerMessage::Event {
            task_id: task_id.to_string(),
            event,
        });
    }

    pub fn fail(&self, task_id: &str, err: impl std::fmt::Display) {
        self.emit(
            task_id,
            TaskEvent::Failed {
                error: format!("{err:#}"),
            },
        );
    }

    /// One run ended; count it and leave if this worker is done.
    pub fn task_finished(&self) {
        self.finished.fetch_add(1, Ordering::Relaxed);
        self.check_exit();
    }

    /// Queues the goodbye when an ephemeral worker has nothing left to do.
    /// The session loop owns the receiver, so it performs the actual exit.
    pub fn check_exit(&self) {
        let running = self.running.lock().expect("running map poisoned").len();
        let finished = self.finished.load(Ordering::Relaxed);
        if !should_exit(self.ephemeral, finished, self.max_tasks, running) {
            return;
        }
        if self.exiting.swap(true, Ordering::Relaxed) {
            return;
        }
        tracing::info!("ephemeral worker ran {finished} task(s), cleaning up and saying goodbye");
        cleanup(&self.data_dir);
        let _ = self.tx.send(WorkerMessage::Goodbye);
    }
}

pub const fn should_exit(ephemeral: bool, finished: u32, max_tasks: u32, running: usize) -> bool {
    ephemeral && finished >= max_tasks && running == 0
}

/// Everything an ephemeral worker leaves behind on its disposable machine.
fn cleanup(data_dir: &Path) {
    for dir in ["worktrees", "repos"] {
        let _ = std::fs::remove_dir_all(data_dir.join(dir));
    }
}

fn certificates(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    let certs = CertificateDer::pem_slice_iter(pem)
        .collect::<Result<Vec<_>, _>>()
        .context("parse CA PEM")?;
    if certs.is_empty() {
        bail!("no certificates in CA PEM");
    }
    Ok(certs)
}

/// Number of certificates in `pem`, or an error if it holds none.
pub fn load_roots(pem: &[u8]) -> Result<usize> {
    certificates(pem).map(|certs| certs.len())
}

/// A rustls connector trusting the webpki roots plus the certificates in `pem`.
/// Without one, `connect_async_tls_with_config` uses the webpki roots alone.
pub fn ca_connector(pem: &[u8]) -> Result<Connector> {
    let mut roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    for cert in certificates(pem)? {
        roots.add(cert).context("add CA certificate")?;
    }
    // Both ring and aws-lc-rs can be linked into one binary; name the
    // provider so this never depends on a process-wide default.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Connector::Rustls(Arc::new(config)))
}

/// Whether a session ended because the orchestrator dropped it (retry) or
/// because the worker said goodbye on purpose (the caller should exit).
enum Ended {
    Disconnected,
    Done,
}

/// The orchestrator refused this worker's hello; retrying would only repeat
/// the refusal, so the caller must give up instead of reconnecting.
#[derive(Debug)]
pub struct Rejected(pub String);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "orchestrator rejected this worker: {}", self.0)
    }
}

impl std::error::Error for Rejected {}

/// How to reach the orchestrator and who this worker says it is.
pub struct Link {
    pub url: String,
    pub token: String,
    pub info: WorkerInfo,
    pub connector: Option<Connector>,
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub async fn run(
    link: Link,
    ctx: Arc<Ctx>,
    mut rx: mpsc::UnboundedReceiver<WorkerMessage>,
) -> Result<()> {
    loop {
        match session(&link, &ctx, &mut rx).await {
            Ok(Ended::Done) => return Ok(()),
            Ok(Ended::Disconnected) => tracing::warn!("disconnected"),
            Err(err) if err.downcast_ref::<Rejected>().is_some() => return Err(err),
            Err(err) => tracing::warn!("connection failed: {err:#}"),
        }
        tokio::time::sleep(RETRY).await;
    }
}

async fn session(
    link: &Link,
    ctx: &Arc<Ctx>,
    rx: &mut mpsc::UnboundedReceiver<WorkerMessage>,
) -> Result<Ended> {
    let (mut sink, mut stream) = handshake(link, ctx).await?.split();
    loop {
        tokio::select! {
            outbound = rx.recv() => {
                let Some(msg) = outbound else { return Ok(Ended::Disconnected) };
                if let Some(ended) = send(&mut sink, msg).await? {
                    return Ok(ended);
                }
            }
            inbound = tokio::time::timeout(READ_TIMEOUT, stream.next()) => {
                let Ok(inbound) = inbound else {
                    return Ok(Ended::Disconnected);
                };
                if let Some(ended) = receive(inbound, ctx)? {
                    return Ok(ended);
                }
            }
        }
    }
}

/// Connects and exchanges hello for hello_ack.
async fn handshake(link: &Link, ctx: &Arc<Ctx>) -> Result<Socket> {
    let url = &link.url;
    let (mut ws, _) =
        tokio_tungstenite::connect_async_tls_with_config(url, None, false, link.connector.clone())
            .await
            .with_context(|| format!("connect {url}"))?;
    let running = ctx
        .running
        .lock()
        .expect("running map poisoned")
        .keys()
        .cloned()
        .collect();
    let hello = WorkerMessage::Hello {
        token: link.token.clone(),
        info: link.info.clone(),
        running,
        version: PROTOCOL_VERSION,
    };
    ws.send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await?;
    match ws.next().await {
        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
            Ok(OrchestratorMessage::HelloAck) => {}
            Ok(OrchestratorMessage::Rejected { reason }) => return Err(Rejected(reason).into()),
            _ => bail!("expected hello_ack, got {text}"),
        },
        other => bail!("expected hello_ack, got {other:?}"),
    }
    tracing::info!("connected to {url}");
    Ok(ws)
}

async fn send<S>(sink: &mut S, msg: WorkerMessage) -> Result<Option<Ended>>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let goodbye = matches!(msg, WorkerMessage::Goodbye);
    sink.send(Message::Text(serde_json::to_string(&msg)?.into()))
        .await?;
    if !goodbye {
        return Ok(None);
    }
    let _ = sink.close().await;
    tokio::time::sleep(FLUSH).await;
    tracing::info!("goodbye sent, exiting");
    Ok(Some(Ended::Done))
}

fn receive(
    inbound: Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    ctx: &Arc<Ctx>,
) -> Result<Option<Ended>> {
    match inbound {
        Some(Ok(Message::Text(text))) => match serde_json::from_str(&text) {
            Ok(msg) => dispatch(msg, ctx),
            Err(err) => tracing::warn!("bad frame: {err} ({text})"),
        },
        Some(Ok(Message::Close(_))) | None => return Ok(Some(Ended::Disconnected)),
        Some(Ok(_)) => {}
        Some(Err(err)) => return Err(err.into()),
    }
    Ok(None)
}

fn dispatch(msg: OrchestratorMessage, ctx: &Arc<Ctx>) {
    match msg {
        OrchestratorMessage::HelloAck => {}
        // Only ever arrives during handshake, handled there.
        OrchestratorMessage::Rejected { .. } => {}
        OrchestratorMessage::Start { task } => {
            let (cancel_tx, cancel_rx) = oneshot::channel();
            ctx.running
                .lock()
                .expect("running map poisoned")
                .insert(task.id.clone(), cancel_tx);
            tokio::spawn(runner::run_task(*task, ctx.clone(), cancel_rx));
        }
        OrchestratorMessage::Message { task_id, text } => {
            let (cancel_tx, cancel_rx) = oneshot::channel();
            ctx.running
                .lock()
                .expect("running map poisoned")
                .insert(task_id.clone(), cancel_tx);
            tokio::spawn(runner::follow_up(task_id, text, ctx.clone(), cancel_rx));
        }
        OrchestratorMessage::Cancel { task_id } => {
            let sender = ctx
                .running
                .lock()
                .expect("running map poisoned")
                .remove(&task_id);
            if let Some(sender) = sender {
                let _ = sender.send(());
            }
        }
        OrchestratorMessage::Push { task_id } => {
            tokio::spawn(runner::push_task(task_id, ctx.clone()));
        }
        OrchestratorMessage::Discard { task_id } => {
            tokio::spawn(runner::discard_task(task_id, ctx.clone()));
        }
    }
    ctx.check_exit();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `openssl req -x509 -newkey rsa:2048 -nodes -subj /CN=lgtm-test-ca`.
    const CA_PEM: &[u8] = b"-----BEGIN CERTIFICATE-----
MIIDDzCCAfegAwIBAgIUL9+SxWy7zOLDfESd5GHjrLqrq84wDQYJKoZIhvcNAQEL
BQAwFzEVMBMGA1UEAwwMbGd0bS10ZXN0LWNhMB4XDTI2MDgyODE1MzI1OVoXDTM2
MDgyNTE1MzI1OVowFzEVMBMGA1UEAwwMbGd0bS10ZXN0LWNhMIIBIjANBgkqhkiG
9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1DRagyqESgy98ItoZPy3omVrhfuWui0/7PKX
AKa626m4guh+0+zsD+CGORHbllbQa00T28K673w1YfXodQ+HFp4dzBsO5qbjydPh
rv3VheCIBoBHysEkEF1JwAmVe0tPYpG1Q1z7V1vC7lYjD6sa0xL4VkaFSEGLZPVr
oFJJxylBAnxgQvHP7a77W23Kl4x2PegTw2Vi8MbrAeSjsQBUheVmNTaIx5FfZ+vP
DdJ8rLLToU7JY1AcPALsf8RC9UCX5KSQOEIV3M6XSUMNbOe7MmpDmMDMojNXV+Br
K16vBfBpkLMKnjpPD1KIaL40XOg335JGP1OBxf1X01Ys2h+NwQIDAQABo1MwUTAd
BgNVHQ4EFgQUuJsEAOgnoN+wFQgxEpTKmj1R2SQwHwYDVR0jBBgwFoAUuJsEAOgn
oN+wFQgxEpTKmj1R2SQwDwYDVR0TAQH/BAUwAwEB/zANBgkqhkiG9w0BAQsFAAOC
AQEAOsmJQ7fcrmPeMAnZICyRzE+CP9CWLVdwgPVBJMyCPQZITgyN6XdJ0ovVW/xq
5LNm69Rf5BtU8Yax6KTmJvQkovpANYOxrOlgPEzqYerGnTLd6GsFj7GOVR8J/dpx
YOMpdJRkNCrph7xzxwjE8A6eQ0gLT4yA/ezVGf8J1vPy4vJou9izYky+aRv3KAya
0MJs8aUGH8x0/5+TzeOgrYhI/lU3c5NshDQV/StC4SJzWVzgHAKbFarc3YBD/c1C
9E9IAT5vyydNPWnX1TXDAiCZAide9s9b+UmtKmBsYL0dI7jvz9/yuVzmKzDtP+L5
yir6+pWbTODJFSqCYKsj4RTsbw==
-----END CERTIFICATE-----
";

    #[test]
    fn only_a_done_ephemeral_worker_with_nothing_running_exits() {
        assert!(should_exit(true, 1, 1, 0));
        assert!(!should_exit(false, 1, 1, 0));
        assert!(!should_exit(true, 1, 2, 0));
        assert!(!should_exit(true, 2, 1, 1));
    }

    #[test]
    fn ca_pem_parses_and_garbage_does_not() {
        assert_eq!(load_roots(CA_PEM).unwrap(), 1);
        assert!(ca_connector(CA_PEM).is_ok());
        assert!(load_roots(b"not a certificate").is_err());
        // A PEM block decodes to bytes without being a certificate; rustls is
        // the one that rejects it.
        let junk = b"-----BEGIN CERTIFICATE-----\nzzzz\n-----END CERTIFICATE-----\n";
        assert!(ca_connector(junk).is_err());
    }
}
