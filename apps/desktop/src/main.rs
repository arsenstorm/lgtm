mod app;
mod assets;
mod batches;
mod changes;
mod composer;
mod home;
mod import;
mod keys;
mod net;
mod palette;
mod panes;
mod render;
mod review;
mod settings;
mod sidebar;
mod theme;
mod titlebar;

use app::LgtmApp;
use gpui::{
    div, point, px, size, AnyView, App, AppContext as _, Application, Bounds, Context, IntoElement,
    ParentElement as _, Render, Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::Root;
use lgtm_orchestrator::token::{self, TokenSource};
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const DEFAULT_ORCHESTRATOR: &str = "http://127.0.0.1:4750";
const DEFAULT_PORT: u16 = 4750;
const MISSING_CONFIG: &str =
    "Set LGTM_TOKEN, run `lgtm serve` on this machine, or write ~/.lgtm/desktop.toml";

#[derive(Default, Deserialize)]
struct FileConfig {
    orchestrator: Option<String>,
    token: Option<String>,
    embedded_orchestrator: Option<bool>,
}

#[derive(Clone)]
pub struct Config {
    pub orchestrator: String,
    pub token: String,
    /// Where the token came from, shown in the settings panel.
    pub token_source: &'static str,
    /// The `embedded_orchestrator` preference, as the settings panel shows it.
    pub embedded: bool,
    /// This process is running the orchestrator it talks to.
    pub hosted: bool,
    /// Join line for other machines, only when hosted (the app's URL is
    /// loopback; other machines need the advertised address).
    pub join: Option<String>,
}

fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("desktop.toml")
}

fn read_file_config(data_dir: &Path) -> FileConfig {
    std::fs::read_to_string(config_path(data_dir))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Rewrites only `embedded_orchestrator`, leaving any other key in the file
/// (and anything this app doesn't know about) alone.
pub fn save_embedded(data_dir: &Path, enabled: bool) -> anyhow::Result<()> {
    let path = config_path(data_dir);
    let mut table: toml::Table = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or_default();
    table.insert(
        "embedded_orchestrator".into(),
        toml::Value::Boolean(enabled),
    );
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(path, table.to_string())?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    External,
    Hosted,
}

/// This app runs the orchestrator only when it is asked to, nothing points it
/// at another machine, and nothing is already answering here.
fn decide_mode(embedded: bool, env_override: bool, reachable: bool) -> Mode {
    if embedded && !env_override && !reachable {
        Mode::Hosted
    } else {
        Mode::External
    }
}

/// The port to host on: the one the configured URL names.
fn port_of(url: &str) -> u16 {
    url.trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|port| port.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn source_label(source: TokenSource) -> &'static str {
    match source {
        TokenSource::Env => "LGTM_TOKEN",
        TokenSource::Generated => "generated",
        _ => "~/.lgtm/token",
    }
}

fn stored_token(data_dir: &Path) -> Option<String> {
    let token = std::fs::read_to_string(token::stored_token_path(data_dir)).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// Resolves where this app points, and starts an orchestrator in this process
/// when nothing else is serving one. `None` means there is no token to use and
/// no orchestrator to mint one.
fn startup() -> Option<Config> {
    let data_dir = token::data_dir(None);
    let file = read_file_config(&data_dir);
    let embedded = file.embedded_orchestrator.unwrap_or(true);
    let env_orchestrator = std::env::var("LGTM_ORCHESTRATOR").ok();
    let env_override = env_orchestrator.is_some();
    let orchestrator = env_orchestrator
        .or(file.orchestrator)
        .unwrap_or_else(|| DEFAULT_ORCHESTRATOR.to_string());
    let (token, token_source) = match std::env::var("LGTM_TOKEN").ok() {
        Some(token) => (Some(token), "LGTM_TOKEN"),
        None => match file.token {
            Some(token) => (Some(token), "desktop.toml"),
            None => (stored_token(&data_dir), "~/.lgtm/token"),
        },
    };
    let reachable = token
        .as_deref()
        .is_some_and(|token| net::reachable(&orchestrator, token));
    match decide_mode(embedded, env_override, reachable) {
        Mode::External => Some(Config {
            orchestrator,
            token: token?,
            token_source,
            embedded,
            hosted: false,
            join: None,
        }),
        Mode::Hosted => host(orchestrator, data_dir, embedded),
    }
}

/// Starts the orchestrator and a worker for it on the app's tokio runtime.
fn host(orchestrator: String, data_dir: PathBuf, embedded: bool) -> Option<Config> {
    let (token, source) = token::resolve_or_create(None, &data_dir)
        .inspect_err(|e| eprintln!("cannot resolve a token: {e:#}"))
        .ok()?;
    let port = port_of(&orchestrator);
    let serve = lgtm_orchestrator::ServeOptions {
        bind: SocketAddr::from(([0, 0, 0, 0], port)),
        token: token.clone(),
        data_dir,
        tls: None,
        provision: None,
    };
    let ip = lgtm_orchestrator::local::advertised_ip();
    let join = settings::join_line(&format!("http://{ip}:{port}"), &token);
    eprintln!("hosting the orchestrator on port {port}; join another machine:  {join}");
    net::runtime().spawn(async move {
        // A bind failure lands here: the app carries on as a plain client and
        // the unreachable banner says so.
        if let Err(e) =
            lgtm_orchestrator::local::serve_local(lgtm_orchestrator::local::LocalOptions {
                serve,
                worker: true,
                worker_name: lgtm_agent::default_name(),
                worker_slots: lgtm_agent::default_slots(),
            })
            .await
        {
            eprintln!("embedded orchestrator stopped: {e:#}");
        }
    });
    Some(Config {
        orchestrator,
        token,
        token_source: source_label(source),
        embedded,
        hosted: true,
        join: Some(join),
    })
}

struct MissingConfig;

impl Render for MissingConfig {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(MISSING_CONFIG)
    }
}

fn main() {
    // ring and aws-lc-rs can both be linked; rustls will not guess.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let config = startup();
    Application::new()
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            theme::init(cx);
            keys::init(cx);

            let options = WindowOptions {
                // The app draws its own bar, so the system one only contributes the
                // traffic lights.
                titlebar: Some(TitlebarOptions {
                    title: None,
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(12.))),
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1200.), px(800.)),
                    cx,
                ))),
                window_min_size: Some(size(px(1000.), px(680.))),
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                let view: AnyView = match config.clone() {
                    Some(config) => cx.new(|cx| LgtmApp::new(config, window, cx)).into(),
                    None => cx.new(|_| MissingConfig).into(),
                };
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("open window");
            // An orchestrator hosted in this process must not outlive the window.
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    std::process::exit(0);
                }
            })
            .detach();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_hosts_only_when_asked_and_nothing_else_answers() {
        assert_eq!(decide_mode(true, false, false), Mode::Hosted);
        assert_eq!(decide_mode(true, false, true), Mode::External);
        assert_eq!(decide_mode(true, true, false), Mode::External);
        assert_eq!(decide_mode(false, false, false), Mode::External);
    }

    #[test]
    fn port_of_reads_the_url_and_falls_back() {
        assert_eq!(port_of("http://127.0.0.1:4761"), 4761);
        assert_eq!(port_of("https://host:4761/"), 4761);
        assert_eq!(port_of("http://host"), DEFAULT_PORT);
    }

    #[test]
    fn embedded_orchestrator_round_trips_without_losing_other_keys() {
        let dir = std::env::temp_dir().join(format!("lgtm-desktop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(config_path(&dir), "orchestrator = \"http://host:1\"\n").unwrap();

        save_embedded(&dir, false).unwrap();
        let file = read_file_config(&dir);
        assert_eq!(file.embedded_orchestrator, Some(false));
        assert_eq!(file.orchestrator.as_deref(), Some("http://host:1"));

        save_embedded(&dir, true).unwrap();
        assert_eq!(read_file_config(&dir).embedded_orchestrator, Some(true));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
