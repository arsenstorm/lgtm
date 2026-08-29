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
use lgtm_client::Client;
use serde::Deserialize;

const DEFAULT_ORCHESTRATOR: &str = "http://127.0.0.1:4750";
const MISSING_CONFIG: &str =
    "Set LGTM_TOKEN, run `lgtm serve` on this machine, or write ~/.lgtm/desktop.toml";

#[derive(Default, Deserialize)]
struct FileConfig {
    orchestrator: Option<String>,
    token: Option<String>,
}

#[derive(Clone)]
struct Config {
    orchestrator: String,
    token: String,
    /// Where the token came from, shown in the settings panel.
    token_source: &'static str,
}

fn stored_token() -> Option<String> {
    let path = dirs::home_dir()?.join(".lgtm/token");
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn load_config() -> Option<Config> {
    let file: FileConfig = dirs::home_dir()
        .map(|home| home.join(".lgtm/desktop.toml"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default();
    let (token, token_source) = match std::env::var("LGTM_TOKEN").ok() {
        Some(token) => (token, "LGTM_TOKEN"),
        None => match file.token {
            Some(token) => (token, "desktop.toml"),
            None => (stored_token()?, "~/.lgtm/token"),
        },
    };
    let orchestrator = std::env::var("LGTM_ORCHESTRATOR")
        .ok()
        .or(file.orchestrator)
        .unwrap_or_else(|| DEFAULT_ORCHESTRATOR.to_string());
    Some(Config {
        orchestrator,
        token,
        token_source,
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
    let config = load_config();
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
                    Some(config) => cx
                        .new(|cx| {
                            LgtmApp::new(
                                Client::new(config.orchestrator.clone(), config.token.clone()),
                                config.orchestrator,
                                config.token,
                                config.token_source,
                                window,
                                cx,
                            )
                        })
                        .into(),
                    None => cx.new(|_| MissingConfig).into(),
                };
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("open window");
            cx.activate(true);
        });
}
