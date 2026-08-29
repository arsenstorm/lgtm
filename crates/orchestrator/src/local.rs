//! Running the orchestrator and a worker for it in the same process, so one
//! machine is useful on its own. Shared by `lgtm serve` and, eventually, the
//! desktop app.

use std::net::{Ipv4Addr, UdpSocket};

use crate::{serve, ServeOptions};

/// Address another machine can dial this orchestrator at: the Tailscale IP
/// when there is one, else this host's LAN address, else loopback.
pub fn advertised_ip() -> String {
    tailscale_ip()
        .or_else(|| first_non_loopback_ipv4().map(|ip| ip.to_string()))
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn tailscale_ip() -> Option<String> {
    let out = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line.lines().next()?.trim();
    first.parse::<Ipv4Addr>().ok().map(|ip| ip.to_string())
}

/// The local address the OS would use to reach the outside world. No packet
/// is sent: a connected UDP socket only fixes the route.
pub fn first_non_loopback_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("10.255.255.255:1").ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// The two lines `lgtm serve` prints once it is up. `scheme` is the HTTP
/// scheme; the join command's ws/wss scheme follows from it.
pub fn join_line(scheme: &str, ip: &str, port: u16, token: &str) -> String {
    let ws = if scheme == "https" { "wss" } else { "ws" };
    format!(
        "lgtm is listening on {scheme}://127.0.0.1:{port}\n\
         join another machine:  lgtm worker {ws}://{ip}:{port} --token {token}"
    )
}

/// The join line for other machines, computed from the bind address (specific
/// host) or the advertised ip (unspecified host).
pub fn join_line_for(opts: &ServeOptions) -> String {
    let ip = if opts.bind.ip().is_unspecified() {
        advertised_ip()
    } else {
        opts.bind.ip().to_string()
    };
    let scheme = if opts.tls.is_some() { "https" } else { "http" };
    join_line(scheme, &ip, opts.bind.port(), &opts.token)
}

pub struct LocalOptions {
    pub serve: ServeOptions,
    pub worker: bool,
    pub worker_name: String,
    pub worker_slots: u32,
}

/// Starts the orchestrator and, when `worker`, a local worker connecting to
/// 127.0.0.1:<port> (wss + the served cert as CA when tls). Returns when the
/// server stops.
pub async fn serve_local(opts: LocalOptions) -> anyhow::Result<()> {
    if opts.worker {
        let scheme = if opts.serve.tls.is_some() {
            "wss"
        } else {
            "ws"
        };
        let port = opts.serve.bind.port();
        let worker_opts = lgtm_agent::WorkerOptions {
            orchestrator: format!("{scheme}://127.0.0.1:{port}"),
            token: opts.serve.token.clone(),
            name: opts.worker_name,
            data_dir: opts.serve.data_dir.clone(),
            slots: opts.worker_slots,
            ephemeral: false,
            max_tasks: 1,
            ca: opts.serve.tls.as_ref().map(|(cert, _)| cert.clone()),
        };
        tokio::spawn(async move {
            // Long enough for the listener below to be accepting connections;
            // the worker's own reconnect loop covers the rest.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match lgtm_agent::run(worker_opts).await {
                Ok(()) => tracing::warn!("local worker stopped"),
                Err(e) => tracing::warn!("local worker stopped: {e:#}"),
            }
        });
    }
    serve(opts.serve).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_line_reads_as_two_copyable_lines() {
        assert_eq!(
            join_line("http", "100.64.0.1", 4750, "abc"),
            "lgtm is listening on http://127.0.0.1:4750\n\
             join another machine:  lgtm worker ws://100.64.0.1:4750 --token abc"
        );
    }

    #[test]
    fn join_line_uses_wss_under_tls() {
        assert!(join_line("https", "100.64.0.1", 4750, "abc")
            .contains("lgtm worker wss://100.64.0.1:4750 --token abc"));
    }

    #[test]
    fn first_non_loopback_ipv4_finds_this_machine() {
        assert!(first_non_loopback_ipv4().is_some());
    }
}
