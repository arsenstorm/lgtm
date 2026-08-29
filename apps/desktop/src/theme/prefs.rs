//! The theme preference: where it is stored, and painting gpui-component's
//! palette from the tokens it picks.

use gpui::{px, App, Global, Hsla, Window};
use gpui_component::{Theme, ThemeColor, ThemeMode};

use super::{dark, light, Tokens, MONO_FONT, RADIUS_PILL, TEXT_BODY, TEXT_MONO, UI_FONT};

/// What the user picked in Settings. `System` follows the OS appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pref {
    System,
    Dark,
    Light,
}

impl Pref {
    pub const ALL: [Pref; 3] = [Pref::System, Pref::Dark, Pref::Light];

    pub fn label(self) -> &'static str {
        match self {
            Pref::System => "System",
            Pref::Dark => "Dark",
            Pref::Light => "Light",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Pref::System => "system",
            Pref::Dark => "dark",
            Pref::Light => "light",
        }
    }

    /// `LGTM_THEME=system|dark|light` overrides what Settings stored.
    fn stored() -> Self {
        let from_config = config()
            .get("theme")
            .and_then(toml::Value::as_str)
            .map(str::to_string);
        match std::env::var("LGTM_THEME").ok().or(from_config).as_deref() {
            Some("dark") => Pref::Dark,
            Some("light") => Pref::Light,
            _ => Pref::System,
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|home| home.join(".lgtm/desktop.toml"))
}

/// `~/.lgtm/desktop.toml`, or an empty table when there isn't one.
pub fn config() -> toml::Table {
    config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

/// Read-modify-write so the token and orchestrator keys survive a preference
/// change. Best effort: an unwritable home directory must not break a button.
pub fn persist(key: &str, value: &str) {
    let Some(path) = config_path() else {
        return;
    };
    let mut table = config();
    table.insert(key.into(), toml::Value::String(value.into()));
    let _ = std::fs::write(path, table.to_string());
}

struct Current(Pref);
impl Global for Current {}

pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    cx.set_global(Current(Pref::stored()));
    apply(None, cx);
}

pub fn pref(cx: &App) -> Pref {
    cx.global::<Current>().0
}

pub fn set_pref(pref: Pref, window: &mut Window, cx: &mut App) {
    cx.set_global(Current(pref));
    persist("theme", pref.key());
    apply(Some(window), cx);
}

/// Re-resolves the mode and repaints gpui-component's palette. Call on start
/// and from `observe_window_appearance` so `System` tracks the OS.
pub fn apply(window: Option<&mut Window>, cx: &mut App) {
    let appearance = window
        .as_ref()
        .map(|window| window.appearance())
        .unwrap_or_else(|| cx.window_appearance());
    let mode = match pref(cx) {
        Pref::System => ThemeMode::from(appearance),
        Pref::Dark => ThemeMode::Dark,
        Pref::Light => ThemeMode::Light,
    };
    // `change` swaps in a whole stock palette, so our overrides go after it.
    Theme::change(mode, None, cx);
    let dark_mode = mode.is_dark();
    let tokens = if dark_mode { dark() } else { light() };
    paint(&tokens, dark_mode, cx);
    if let Some(window) = window {
        window.refresh();
    }
}

fn paint(t: &Tokens, dark_mode: bool, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    paint_type(theme);
    paint_colors(&mut theme.colors, t);
    paint_widgets(&mut theme.colors, t, dark_mode);
}

fn paint_type(theme: &mut Theme) {
    theme.font_family = UI_FONT.into();
    theme.font_size = px(TEXT_BODY);
    theme.mono_font_family = MONO_FONT.into();
    theme.mono_font_size = px(TEXT_MONO);
    // Every widget rounds off `radius`, and everything gpui-component draws for
    // us is pill-shaped in the reference design.
    theme.radius = px(RADIUS_PILL);
    theme.radius_lg = px(RADIUS_PILL);
    theme.shadow = false;
}

/// The palette proper: what shadcn calls the semantic colours.
fn paint_colors(c: &mut ThemeColor, t: &Tokens) {
    c.background = t.bg;
    c.foreground = t.fg;
    c.border = t.border;
    c.muted = t.muted;
    c.muted_foreground = t.muted_fg;
    c.accent = t.muted;
    c.accent_foreground = t.fg;
    c.primary = t.primary;
    c.primary_hover = t.primary;
    c.primary_active = t.primary;
    c.primary_foreground = t.primary_fg;
    c.secondary = t.muted;
    c.secondary_hover = t.muted;
    c.secondary_active = t.muted;
    c.secondary_foreground = t.fg;
    c.danger = t.danger;
    c.danger_hover = t.danger;
    c.danger_active = t.danger;
    c.danger_foreground = t.primary_fg;
    c.success = t.success;
    c.success_foreground = t.primary_fg;
    c.warning = t.warning;
    c.warning_foreground = t.primary_fg;
    c.info = t.info;
    c.info_foreground = t.primary_fg;
    c.input = t.input;
    c.ring = t.ring;
    c.selection = t.selection;
    c.popover = t.popover;
    c.popover_foreground = t.fg;
}

/// Per-widget colours gpui-component reads on top of the palette.
fn paint_widgets(c: &mut ThemeColor, t: &Tokens, dark_mode: bool) {
    c.list = t.bg;
    c.list_active = t.muted;
    c.list_active_border = t.border;
    c.list_hover = t.muted;
    c.sidebar = t.sidebar;
    c.sidebar_border = t.sidebar_border;
    c.sidebar_foreground = t.fg;
    c.sidebar_accent = t.muted;
    c.sidebar_accent_foreground = t.fg;
    // The shadcn tabs list: a `bg-muted` pill whose active tab is `bg-background`.
    c.tab_bar = t.bg;
    c.tab_bar_segmented = t.muted;
    c.tab = t.bg;
    c.tab_active = t.bg;
    c.tab_foreground = t.muted_fg;
    c.tab_active_foreground = t.fg;
    c.switch = t.input;
    // One thumb colour has to read on both the off track and the light `primary`.
    c.switch_thumb = if dark_mode { t.muted_fg } else { t.bg };
    c.title_bar = t.bg;
    c.title_bar_border = t.border;
    c.scrollbar = Hsla::transparent_black();
    c.scrollbar_thumb = t.border;
    c.scrollbar_thumb_hover = t.muted_fg;
}
