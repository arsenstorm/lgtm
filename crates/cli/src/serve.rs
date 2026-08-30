//! `lgtm serve` and `lgtm runner`: the two long-running commands.

use lgtm_orchestrator::token::data_dir;

use crate::cli::{RunnerArgs, ServeArgs};
use crate::require_token;

pub async fn serve(args: ServeArgs, token: Option<String>) -> anyhow::Result<i32> {
    init_tracing();
    let bind_addr: std::net::SocketAddr = args.bind.parse()?;
    let data_dir = data_dir(args.data_dir.clone());
    // Unlike every other subcommand, `serve` mints a token rather than
    // demanding one: it is the machine everyone else joins.
    let (token, source) = lgtm_orchestrator::token::resolve_or_create(token, &data_dir)?;
    if source == lgtm_orchestrator::token::TokenSource::Generated {
        tracing::info!(
            "generated token {token} (saved to {})",
            lgtm_orchestrator::token::stored_token_path(&data_dir).display()
        );
    }
    let tls = match (args.tls_cert.clone(), args.tls_key.clone()) {
        (Some(cert), Some(key)) => Some((cert, key)),
        (None, None) => None,
        _ => anyhow::bail!("pass both --tls-cert and --tls-key"),
    };
    let serve_opts = lgtm_orchestrator::ServeOptions {
        bind: bind_addr,
        token,
        data_dir,
        provision: provision_options(&args, tls.is_some(), bind_addr),
        orchestrate: args.orchestrate,
        models: models(&args.model_for),
        tls,
        webhook: args.webhook.clone(),
        prefer: args.prefer,
    };
    eprintln!("{}", lgtm_orchestrator::local::join_line_for(&serve_opts));
    lgtm_orchestrator::local::serve_local(lgtm_orchestrator::local::LocalOptions {
        serve: serve_opts,
        runner: !args.no_runner,
        runner_name: lgtm_agent::default_name(),
        runner_slots: lgtm_agent::default_slots(),
    })
    .await?;
    Ok(0)
}

pub async fn runner(
    args: RunnerArgs,
    token: Option<String>,
    ca: Option<std::path::PathBuf>,
) -> anyhow::Result<i32> {
    init_tracing();
    let data_dir = data_dir(args.data_dir);
    let token = require_token(token, &data_dir);
    lgtm_agent::run(lgtm_agent::RunnerOptions {
        orchestrator: ws_url(&args.url),
        token,
        // clap only binds one env var per flag; the pre-rename name is read
        // by hand so an existing LGTM_WORKER_NAME setup keeps working.
        name: args
            .name
            .or_else(|| std::env::var("LGTM_WORKER_NAME").ok())
            .unwrap_or_else(lgtm_agent::default_name),
        data_dir,
        slots: args.slots.unwrap_or_else(lgtm_agent::default_slots),
        ephemeral: args.ephemeral,
        max_tasks: args.max_tasks,
        ca,
    })
    .await?;
    Ok(0)
}

/// The model table: `--model-for plan=opus` when given, else `LGTM_MODELS`
/// (`plan=opus,run=sonnet`). Anything that is not a `kind=model` pair is
/// skipped rather than failing a server start.
fn models(flags: &[String]) -> Vec<(String, String)> {
    let env = std::env::var("LGTM_MODELS").unwrap_or_default();
    let from_env: Vec<String> = env.split(',').map(str::to_string).collect();
    let pairs: &[String] = if flags.is_empty() { &from_env } else { flags };
    pairs
        .iter()
        .filter_map(|pair| pair.split_once('='))
        .map(|(kind, model)| (kind.trim().to_string(), model.trim().to_string()))
        .filter(|(kind, model)| !kind.is_empty() && !model.is_empty())
        .collect()
}

fn provision_options(
    args: &ServeArgs,
    tls: bool,
    bind: std::net::SocketAddr,
) -> Option<lgtm_orchestrator::ProvisionOptions> {
    let command = args.provision.clone()?;
    let public_url = args
        .public_url
        .clone()
        .unwrap_or_else(|| default_public_url(&args.bind, tls, &advertised(bind)));
    Some(lgtm_orchestrator::ProvisionOptions {
        command,
        max: args.provision_max,
        public_url,
    })
}

/// A specific bind address is the only one runners can dial; `0.0.0.0`
/// becomes the address this machine advertises.
fn advertised(bind: std::net::SocketAddr) -> String {
    if bind.ip().is_unspecified() {
        lgtm_orchestrator::local::advertised_ip()
    } else {
        bind.ip().to_string()
    }
}

/// Best-guess URL a provisioned runner can reach this orchestrator at, when
/// `--public-url` isn't given: `bind`'s scheme plus host, with `0.0.0.0`
/// (which a runner on another machine can't dial) swapped for the address
/// this machine advertises in its join line.
fn default_public_url(bind: &str, tls: bool, ip: &str) -> String {
    let scheme = if tls { "https" } else { "http" };
    let host = bind.replacen("0.0.0.0", ip, 1);
    format!("{scheme}://{host}")
}

/// `lgtm runner` takes the same URL a person would paste from a browser, so
/// an http(s) one becomes its ws(s) equivalent.
fn ws_url(url: &str) -> String {
    match url.split_once("://") {
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("https", rest)) => format!("wss://{rest}"),
        _ => url.to_string(),
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_table_keeps_only_kind_equals_model_pairs() {
        assert_eq!(
            models(&["plan=opus".into(), " run = sonnet ".into(), "junk".into()]),
            [
                ("plan".to_string(), "opus".to_string()),
                ("run".to_string(), "sonnet".to_string())
            ]
        );
        // No flags and no LGTM_MODELS in this process: nothing is configured.
        assert!(models(&[]).is_empty());
    }

    #[test]
    fn default_public_url_swaps_unreachable_bind_host() {
        assert_eq!(
            default_public_url("0.0.0.0:4750", false, "127.0.0.1"),
            "http://127.0.0.1:4750"
        );
    }

    #[test]
    fn default_public_url_uses_https_scheme_under_tls() {
        assert_eq!(
            default_public_url("0.0.0.0:4750", true, "127.0.0.1"),
            "https://127.0.0.1:4750"
        );
    }

    #[test]
    fn default_public_url_keeps_a_reachable_host() {
        assert_eq!(
            default_public_url("10.0.0.5:4750", false, "100.64.0.1"),
            "http://10.0.0.5:4750"
        );
    }
}
