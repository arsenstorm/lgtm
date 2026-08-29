//! The HTTP proxy an `allowlist` run is pointed at: the one route out of the
//! sandbox, so a host the repository did not name is refused rather than
//! reached. It speaks the two forms a proxy client uses — `CONNECT host:port`
//! for TLS, and an absolute-form request line for plain HTTP — and forwards
//! bytes without looking inside them.

use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{copy_bidirectional, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection Established\r\n\r\n";
const FORBIDDEN: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const BAD_REQUEST: &[u8] =
    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const BAD_GATEWAY: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

/// Binds on the loopback and serves until the returned task is aborted. Every
/// refused host is sent on `denied` once per connection; the caller decides
/// what to report.
pub async fn serve(
    allowed: Vec<String>,
    denied: UnboundedSender<String>,
) -> std::io::Result<(SocketAddr, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(accept(listener, Arc::new(allowed), denied));
    Ok((addr, handle))
}

async fn accept(listener: TcpListener, allowed: Arc<Vec<String>>, denied: UnboundedSender<String>) {
    while let Ok((client, _)) = listener.accept().await {
        tokio::spawn(handle(client, allowed.clone(), denied.clone()));
    }
}

/// Where one request wants to go.
struct Target {
    host: String,
    port: u16,
    /// A tunnel rather than a request to forward.
    connect: bool,
}

async fn handle(client: TcpStream, allowed: Arc<Vec<String>>, denied: UnboundedSender<String>) {
    let mut client = BufReader::new(client);
    let mut head = String::new();
    if client.read_line(&mut head).await.is_err() {
        return;
    }
    let Some(target) = parse(&head) else {
        let _ = client.get_mut().write_all(BAD_REQUEST).await;
        return;
    };
    if !permitted(&allowed, &target.host) {
        let _ = denied.send(target.host);
        let _ = client.get_mut().write_all(FORBIDDEN).await;
        return;
    }
    let Ok(mut upstream) = TcpStream::connect((target.host.as_str(), target.port)).await else {
        let _ = client.get_mut().write_all(BAD_GATEWAY).await;
        return;
    };
    let _ = match target.connect {
        true => tunnel(&mut client, &mut upstream).await,
        false => forward(&mut client, &mut upstream, &head).await,
    };
}

/// The tunnelled protocol is none of the proxy's business, so the request's
/// remaining headers are read and dropped and the rest is raw bytes.
async fn tunnel(
    client: &mut BufReader<TcpStream>,
    upstream: &mut TcpStream,
) -> std::io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if client.read_line(&mut line).await? == 0 || line.trim().is_empty() {
            break;
        }
    }
    client.get_mut().write_all(ESTABLISHED).await?;
    pipe(client, upstream, &[]).await
}

/// The origin server is given the request line as the client wrote it,
/// absolute form and all: every server accepts it, and rewriting it would
/// mean parsing what this proxy deliberately does not read.
async fn forward(
    client: &mut BufReader<TcpStream>,
    upstream: &mut TcpStream,
    head: &str,
) -> std::io::Result<()> {
    pipe(client, upstream, head.as_bytes()).await
}

/// Reading a line buffers whatever followed it, and `copy_bidirectional`
/// works on the socket, not the reader: those bytes go upstream by hand or
/// they are lost.
async fn pipe(
    client: &mut BufReader<TcpStream>,
    upstream: &mut TcpStream,
    head: &[u8],
) -> std::io::Result<()> {
    let buffered = client.buffer().to_vec();
    client.consume(buffered.len());
    if !head.is_empty() || !buffered.is_empty() {
        upstream.write_all(head).await?;
        upstream.write_all(&buffered).await?;
    }
    copy_bidirectional(client.get_mut(), upstream).await?;
    Ok(())
}

fn parse(head: &str) -> Option<Target> {
    let mut parts = head.split_whitespace();
    let method = parts.next()?;
    let uri = parts.next()?;
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = authority(uri, 443)?;
        return Some(Target {
            host,
            port,
            connect: true,
        });
    }
    let (host, port) = authority(uri.strip_prefix("http://")?.split('/').next()?, 80)?;
    Some(Target {
        host,
        port,
        connect: false,
    })
}

fn authority(text: &str, default: u16) -> Option<(String, u16)> {
    let (host, port) = match text.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (text, default),
    };
    (!host.is_empty()).then(|| (host.to_ascii_lowercase(), port))
}

/// Exact, or any name under it when the entry starts with a dot.
fn permitted(allowed: &[String], host: &str) -> bool {
    allowed.iter().any(|entry| match entry.strip_prefix('.') {
        Some(domain) => host == domain || host.ends_with(entry.as_str()),
        None => host == entry,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::AsyncReadExt;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    #[test]
    fn matching_is_exact_unless_the_entry_starts_with_a_dot() {
        let allowed = ["github.com".to_string(), ".crates.io".to_string()];
        assert!(permitted(&allowed, "github.com"));
        assert!(!permitted(&allowed, "api.github.com"));
        assert!(!permitted(&allowed, "evilgithub.com"));
        assert!(permitted(&allowed, "crates.io"));
        assert!(permitted(&allowed, "static.crates.io"));
        assert!(!permitted(&allowed, "notcrates.io"));
    }

    #[test]
    fn both_request_forms_name_a_host_and_a_default_port() {
        let connect = parse("CONNECT API.github.com:443 HTTP/1.1\r\n").expect("connect");
        assert_eq!(
            (connect.host.as_str(), connect.port, connect.connect),
            ("api.github.com", 443, true)
        );
        let plain = parse("GET http://crates.io/api/v1 HTTP/1.1\r\n").expect("get");
        assert_eq!(
            (plain.host.as_str(), plain.port, plain.connect),
            ("crates.io", 80, false)
        );
        assert_eq!(
            parse("GET http://host:8080/x HTTP/1.1\r\n")
                .expect("port")
                .port,
            8080
        );
        assert_eq!(parse("CONNECT host HTTP/1.1\r\n").expect("bare").port, 443);
        assert!(parse("GET /relative HTTP/1.1\r\n").is_none());
        assert!(parse("GET\r\n").is_none());
    }

    /// Echoes back everything it is sent, one connection at a time.
    async fn echo() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("echo bind");
        let addr = listener.local_addr().expect("echo addr");
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let (mut read, mut write) = sock.split();
                    let _ = tokio::io::copy(&mut read, &mut write).await;
                });
            }
        });
        addr
    }

    async fn proxy(allowed: &[&str]) -> (SocketAddr, UnboundedReceiver<String>) {
        let (tx, rx) = unbounded_channel();
        let allowed = allowed.iter().map(|h| h.to_string()).collect();
        let (addr, _) = serve(allowed, tx).await.expect("proxy bind");
        (addr, rx)
    }

    async fn request(proxy: SocketAddr, head: &str) -> TcpStream {
        let mut client = TcpStream::connect(proxy).await.expect("dial proxy");
        client.write_all(head.as_bytes()).await.expect("request");
        client
    }

    async fn reply(client: &mut TcpStream) -> String {
        let mut buf = [0u8; 128];
        let n = client.read(&mut buf).await.expect("reply");
        String::from_utf8_lossy(&buf[..n]).to_string()
    }

    #[tokio::test]
    async fn an_allowed_connect_tunnels_bytes_to_the_target() {
        let target = echo().await;
        let (proxy_addr, _denied) = proxy(&["127.0.0.1"]).await;
        let mut client = request(
            proxy_addr,
            &format!("CONNECT 127.0.0.1:{} HTTP/1.1\r\n\r\n", target.port()),
        )
        .await;
        assert!(reply(&mut client).await.starts_with("HTTP/1.1 200"));

        client.write_all(b"ping").await.expect("write");
        let mut buf = [0u8; 4];
        client.read_exact(&mut buf).await.expect("echo back");
        assert_eq!(&buf, b"ping");
    }

    #[tokio::test]
    async fn a_denied_host_is_refused_and_reported() {
        let (proxy_addr, mut denied) = proxy(&["github.com"]).await;
        let mut client = request(proxy_addr, "CONNECT evil.example:443 HTTP/1.1\r\n\r\n").await;
        assert!(reply(&mut client).await.starts_with("HTTP/1.1 403"));
        assert_eq!(denied.recv().await.as_deref(), Some("evil.example"));
    }

    #[tokio::test]
    async fn a_plain_request_reaches_the_origin_as_it_was_written() {
        let target = echo().await;
        let (proxy_addr, _denied) = proxy(&["127.0.0.1"]).await;
        let head = format!(
            "GET http://127.0.0.1:{}/x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            target.port()
        );
        let mut client = request(proxy_addr, &head).await;
        let mut echoed = vec![0u8; head.len()];
        client.read_exact(&mut echoed).await.expect("echo back");
        assert_eq!(String::from_utf8_lossy(&echoed), head);
    }

    #[tokio::test]
    async fn a_denied_plain_request_never_leaves_the_proxy() {
        let (proxy_addr, mut denied) = proxy(&[".github.com"]).await;
        let mut client = request(
            proxy_addr,
            "POST http://evil.example/x HTTP/1.1\r\nHost: evil.example\r\n\r\n",
        )
        .await;
        assert!(reply(&mut client).await.starts_with("HTTP/1.1 403"));
        assert_eq!(denied.recv().await.as_deref(), Some("evil.example"));
    }

    /// Ignored: it needs the real internet, and a machine without it would
    /// fail a test about the proxy for a reason that is not the proxy.
    #[tokio::test]
    #[ignore]
    async fn a_real_client_reaches_an_allowed_host_and_no_other() {
        let (addr, mut denied) = proxy(&["api.github.com"]).await;
        assert!(curl(addr, "https://api.github.com").await);
        assert!(!curl(addr, "https://example.com").await);
        assert_eq!(denied.recv().await.as_deref(), Some("example.com"));
    }

    async fn curl(proxy: SocketAddr, url: &str) -> bool {
        tokio::process::Command::new("curl")
            .args(["-sS", "-m", "20", "-o", "/dev/null", "-x"])
            .arg(format!("http://{proxy}"))
            .arg(url)
            .status()
            .await
            .expect("curl")
            .success()
    }
}
