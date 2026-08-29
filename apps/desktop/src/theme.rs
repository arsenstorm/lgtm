//! The design system: one palette per mode, pushed into gpui-component's
//! `Theme` so its own widgets (buttons, inputs, dropdowns) match the hand-rolled
//! parts of the shell.

use gpui::{px, rgb, rgba, App, Global, Hsla, Window};
use gpui_component::{ActiveTheme as _, Theme, ThemeMode};

/// 8-pt scale with a 4 for tight gaps.
pub const SPACE: [f32; 7] = [4., 8., 12., 16., 24., 32., 48.];
pub const TEXT_BODY: f32 = 13.;
pub const TEXT_SECONDARY: f32 = 12.;
pub const TEXT_MONO: f32 = 11.5;
pub const TEXT_TITLE: f32 = 15.;
pub const MONO_FONT: &str = "Menlo";
/// gpui resolves this to the platform UI font (San Francisco on macOS).
pub const UI_FONT: &str = ".SystemUIFont";

pub const RADIUS: f32 = 8.;
pub const RADIUS_LG: f32 = 12.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface_raised: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub accent_fg: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub diff_add_bg: Hsla,
    pub diff_add_emph: Hsla,
    pub diff_del_bg: Hsla,
    pub diff_del_emph: Hsla,
    pub hunk_bg: Hsla,
    pub selection: Hsla,
}

pub fn dark() -> Tokens {
    Tokens {
        bg: rgb(0x0F1115).into(),
        surface: rgb(0x161A20).into(),
        surface_raised: rgb(0x1D222A).into(),
        border: rgb(0x2A303A).into(),
        text: rgb(0xE6E8EC).into(),
        text_muted: rgb(0x8B93A1).into(),
        accent: rgb(0x7C9CFF).into(),
        accent_fg: rgb(0x0B0E14).into(),
        success: rgb(0x3DD68C).into(),
        warning: rgb(0xF5B54A).into(),
        danger: rgb(0xFF6B6B).into(),
        diff_add_bg: rgba(0x3DD68C1F).into(),
        diff_add_emph: rgba(0x3DD68C59).into(),
        diff_del_bg: rgba(0xFF6B6B1F).into(),
        diff_del_emph: rgba(0xFF6B6B59).into(),
        hunk_bg: rgb(0x1A1F27).into(),
        selection: rgba(0x7C9CFF2E).into(),
    }
}

pub fn light() -> Tokens {
    Tokens {
        bg: rgb(0xFFFFFF).into(),
        surface: rgb(0xF6F7F9).into(),
        surface_raised: rgb(0xFFFFFF).into(),
        border: rgb(0xE3E6EB).into(),
        text: rgb(0x1B1F27).into(),
        text_muted: rgb(0x6B7280).into(),
        accent: rgb(0x3B6CF6).into(),
        accent_fg: rgb(0xFFFFFF).into(),
        success: rgb(0x178A55).into(),
        warning: rgb(0xB7791F).into(),
        danger: rgb(0xD64545).into(),
        diff_add_bg: rgba(0x178A551A).into(),
        diff_add_emph: rgba(0x178A5547).into(),
        diff_del_bg: rgba(0xD645451A).into(),
        diff_del_emph: rgba(0xD6454547).into(),
        hunk_bg: rgb(0xF0F2F5).into(),
        selection: rgba(0x3B6CF624).into(),
    }
}

pub fn tokens(cx: &App) -> Tokens {
    if cx.theme().mode.is_dark() {
        dark()
    } else {
        light()
    }
}

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

    /// `LGTM_THEME=system|dark|light` overrides the default at startup.
    fn from_env() -> Self {
        match std::env::var("LGTM_THEME").as_deref() {
            Ok("dark") => Pref::Dark,
            Ok("light") => Pref::Light,
            _ => Pref::System,
        }
    }
}

struct Current(Pref);
impl Global for Current {}

pub fn init(cx: &mut App) {
    gpui_component::init(cx);
    cx.set_global(Current(Pref::from_env()));
    apply(None, cx);
}

pub fn pref(cx: &App) -> Pref {
    cx.global::<Current>().0
}

pub fn set_pref(pref: Pref, window: &mut Window, cx: &mut App) {
    cx.set_global(Current(pref));
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
    let tokens = if mode.is_dark() { dark() } else { light() };
    paint(&tokens, cx);
    if let Some(window) = window {
        window.refresh();
    }
}

fn paint(t: &Tokens, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.font_family = UI_FONT.into();
    theme.font_size = px(TEXT_BODY);
    theme.mono_font_family = MONO_FONT.into();
    theme.mono_font_size = px(TEXT_MONO);
    theme.radius = px(RADIUS);
    theme.radius_lg = px(RADIUS_LG);
    theme.shadow = false;

    let c = &mut theme.colors;
    c.background = t.bg;
    c.foreground = t.text;
    c.border = t.border;
    c.muted = t.surface;
    c.muted_foreground = t.text_muted;
    c.accent = t.accent;
    c.accent_foreground = t.accent_fg;
    c.primary = t.accent;
    c.primary_hover = t.accent;
    c.primary_active = t.accent;
    c.primary_foreground = t.accent_fg;
    c.secondary = t.surface_raised;
    c.secondary_hover = t.surface;
    c.secondary_active = t.surface;
    c.secondary_foreground = t.text;
    c.danger = t.danger;
    c.danger_hover = t.danger;
    c.danger_active = t.danger;
    c.danger_foreground = t.accent_fg;
    c.success = t.success;
    c.success_foreground = t.accent_fg;
    c.warning = t.warning;
    c.warning_foreground = t.accent_fg;
    c.input = t.border;
    c.ring = t.accent;
    c.selection = t.selection;
    c.popover = t.surface_raised;
    c.popover_foreground = t.text;
    c.list = t.surface;
    c.list_active = t.selection;
    c.list_active_border = t.accent;
    c.list_hover = t.surface_raised;
    c.sidebar = t.surface;
    c.sidebar_border = t.border;
    c.sidebar_foreground = t.text;
    c.sidebar_accent = t.selection;
    c.sidebar_accent_foreground = t.text;
    c.tab_bar = t.bg;
    c.tab = t.bg;
    c.tab_active = t.bg;
    c.tab_foreground = t.text_muted;
    c.tab_active_foreground = t.text;
    c.switch = t.border;
    c.title_bar = t.bg;
    c.title_bar_border = t.border;
    c.scrollbar = Hsla::transparent_black();
    c.scrollbar_thumb = t.border;
    c.scrollbar_thumb_hover = t.text_muted;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(t: &Tokens) -> [Hsla; 17] {
        [
            t.bg,
            t.surface,
            t.surface_raised,
            t.border,
            t.text,
            t.text_muted,
            t.accent,
            t.accent_fg,
            t.success,
            t.warning,
            t.danger,
            t.diff_add_bg,
            t.diff_add_emph,
            t.diff_del_bg,
            t.diff_del_emph,
            t.hunk_bg,
            t.selection,
        ]
    }

    #[test]
    fn every_dark_token_is_set() {
        for color in all(&dark()) {
            assert_ne!(color, Hsla::default());
        }
    }

    #[test]
    fn every_light_token_is_set() {
        for color in all(&light()) {
            assert_ne!(color, Hsla::default());
        }
    }

    #[test]
    fn dark_and_light_differ() {
        assert_ne!(dark(), light());
    }
}
