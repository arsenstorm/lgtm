mod app;
mod diff;
mod net;
mod panes;
mod render;
mod sidebar;

use app::LgtmApp;
use gpui::{
    div, px, size, AnyView, App, AppContext as _, Application, Bounds, Context, IntoElement,
    ParentElement as _, Render, Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::{Root, Theme, ThemeMode};
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
    let token = std::env::var("LGTM_TOKEN")
        .ok()
        .or(file.token)
        .or_else(stored_token)?;
    let orchestrator = std::env::var("LGTM_ORCHESTRATOR")
        .ok()
        .or(file.orchestrator)
        .unwrap_or_else(|| DEFAULT_ORCHESTRATOR.to_string());
    Some(Config {
        orchestrator,
        token,
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
    Application::new().run(move |cx: &mut App| {
        gpui_component::init(cx);
        app::init(cx);
        Theme::change(ThemeMode::Light, None, cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("LGTM".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1200.), px(800.)),
                cx,
            ))),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let view: AnyView = match config.clone() {
                Some(config) => cx
                    .new(|cx| {
                        LgtmApp::new(Client::new(config.orchestrator, config.token), window, cx)
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
