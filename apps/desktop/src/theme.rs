//! The design system: the shadcn "neutral" palette the Tauri app used, pushed
//! into gpui-component's `Theme` so its own widgets (buttons, inputs,
//! dropdowns) match the hand-rolled parts of the shell.

use gpui::{
    div, px, rgb, rgba, App, Div, Entity, FontWeight, Global, Hsla, ParentElement as _,
    Styled as _, Window,
};
use gpui_component::input::{Input, InputState};
use gpui_component::{ActiveTheme as _, Theme, ThemeMode};

/// 8-pt scale with a 4 for tight gaps.
pub const SPACE: [f32; 7] = [4., 8., 12., 16., 24., 32., 48.];
/// `text-sm`.
pub const TEXT_BODY: f32 = 14.;
/// `text-xs`: secondary copy, section labels, status bars.
pub const TEXT_SECONDARY: f32 = 12.;
/// File paths and code.
pub const TEXT_MONO: f32 = 12.;
/// `text-[11px]`: the +/- counts next to a file.
pub const TEXT_COUNT: f32 = 11.;
/// `text-xl`: the welcome title.
pub const TEXT_TITLE: f32 = 20.;
/// One diff/log row.
pub const LINE_MONO: f32 = 20.;
pub const MONO_FONT: &str = "Menlo";
/// gpui resolves this to the platform UI font (San Francisco on macOS).
pub const UI_FONT: &str = ".SystemUIFont";

/// `--radius`: cards and comment cards.
pub const RADIUS: f32 = 10.;
/// `rounded-2xl`: buttons, inputs, tabs, badges, icon tiles — the signature.
pub const RADIUS_PILL: f32 = 16.;

/// `h-11`: the window and task headers.
pub const HEADER_H: f32 = 44.;
/// `h-6`: the status/footer bars.
pub const STATUS_H: f32 = 24.;
/// One sidebar row.
pub const ROW_H: f32 = 28.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub bg: Hsla,
    pub fg: Hsla,
    pub card: Hsla,
    pub popover: Hsla,
    pub primary: Hsla,
    pub primary_fg: Hsla,
    /// `--muted`/`--secondary`/`--accent`: one grey for fills, hovers, selection.
    pub muted: Hsla,
    pub muted_fg: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    /// `bg-input/50`: what a borderless field is filled with.
    pub input_fill: Hsla,
    pub ring: Hsla,
    pub sidebar: Hsla,
    pub sidebar_border: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub info: Hsla,
    pub danger: Hsla,
    pub diff_add: Hsla,
    pub diff_del: Hsla,
    pub diff_add_bg: Hsla,
    pub diff_add_emph: Hsla,
    pub diff_del_bg: Hsla,
    pub diff_del_emph: Hsla,
    /// Context gutter and hunk separator: the background nudged toward the text.
    pub gutter: Hsla,
    pub hunk_bg: Hsla,
    pub selection: Hsla,
}

/// `amount` of `base` mixed over `bg`, both packed `0xRRGGBB`. This is how
/// `@pierre/diffs` builds its row tints and gutter fills.
fn mix(bg: u32, base: u32, amount: f32) -> Hsla {
    let channel = |shift: u32| {
        let (b, f) = ((bg >> shift) & 0xff, (base >> shift) & 0xff);
        (b as f32 + (f as f32 - b as f32) * amount).round() as u32
    };
    rgb((channel(16) << 16) | (channel(8) << 8) | channel(0)).into()
}

pub fn dark() -> Tokens {
    const BG: u32 = 0x0a_0a_0a;
    const FG: u32 = 0xfa_fa_fa;
    const ADD: u32 = 0x5e_cc_71;
    const DEL: u32 = 0xff_67_62;
    Tokens {
        bg: rgb(BG).into(),
        fg: rgb(FG).into(),
        card: rgb(0x1b1b1b).into(),
        popover: rgb(0x1b1b1b).into(),
        primary: rgb(0xebebeb).into(),
        primary_fg: rgb(0x1b1b1b).into(),
        muted: rgb(0x2b2b2b).into(),
        muted_fg: rgb(0xa3a3a3).into(),
        border: rgba(0xffffff1a).into(),
        input: rgba(0xffffff26).into(),
        input_fill: rgba(0xffffff13).into(),
        ring: rgb(0x8a8a8a).into(),
        sidebar: rgb(0x1b1b1b).into(),
        sidebar_border: rgba(0xffffff1a).into(),
        success: rgb(0x34d399).into(),
        warning: rgb(0xfbbf24).into(),
        info: rgb(0xa78bfa).into(),
        danger: rgb(0xff5c5c).into(),
        diff_add: rgb(ADD).into(),
        diff_del: rgb(DEL).into(),
        diff_add_bg: mix(BG, ADD, 0.20),
        diff_add_emph: rgba(0x5ecc7133).into(),
        diff_del_bg: mix(BG, DEL, 0.20),
        diff_del_emph: rgba(0xff676233).into(),
        gutter: mix(BG, FG, 0.075),
        hunk_bg: mix(BG, FG, 0.075),
        selection: rgba(0xebebeb40).into(),
    }
}

pub fn light() -> Tokens {
    const BG: u32 = 0xff_ff_ff;
    const FG: u32 = 0x0b_0b_0b;
    const ADD: u32 = 0x0d_be_4e;
    const DEL: u32 = 0xff_2e_3f;
    Tokens {
        bg: rgb(BG).into(),
        fg: rgb(FG).into(),
        card: rgb(0xffffff).into(),
        popover: rgb(0xffffff).into(),
        primary: rgb(0x1b1b1b).into(),
        primary_fg: rgb(0xfafafa).into(),
        muted: rgb(0xf5f5f5).into(),
        muted_fg: rgb(0x737373).into(),
        border: rgb(0xebebeb).into(),
        input: rgb(0xebebeb).into(),
        input_fill: rgb(0xf5f5f5).into(),
        ring: rgb(0xb4b4b4).into(),
        sidebar: rgb(0xfafafa).into(),
        sidebar_border: rgb(0xebebeb).into(),
        success: rgb(0x059669).into(),
        warning: rgb(0xd97706).into(),
        info: rgb(0x7c3aed).into(),
        danger: rgb(0xdc2626).into(),
        diff_add: rgb(ADD).into(),
        diff_del: rgb(DEL).into(),
        diff_add_bg: mix(BG, ADD, 0.12),
        diff_add_emph: rgba(0x0dbe4e26).into(),
        diff_del_bg: mix(BG, DEL, 0.12),
        diff_del_emph: rgba(0xff2e3f26).into(),
        gutter: mix(BG, FG, 0.015),
        hunk_bg: mix(BG, FG, 0.015),
        selection: rgba(0x1b1b1b26).into(),
    }
}

/// The 3% lift a hovered diff row gets, applied to whatever tint it carries so
/// an addition row stays green under the cursor.
pub fn lighten(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l + 0.03).min(1.),
        ..color
    }
}

/// A borderless `bg-input/50` pill: every text field in the reference design.
pub fn field(state: &Entity<InputState>, t: &Tokens) -> Input {
    Input::new(state)
        .bordered(false)
        .bg(t.input_fill)
        .rounded(px(RADIUS_PILL))
}

/// `text-xs uppercase tracking-wide text-muted-foreground`. gpui has no
/// letter-spacing, so the wide tracking of the original is lost.
pub fn section_label(text: &str, t: &Tokens) -> Div {
    div()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .font_weight(FontWeight::MEDIUM)
        .child(text.to_uppercase())
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
    let dark_mode = mode.is_dark();
    let tokens = if dark_mode { dark() } else { light() };
    paint(&tokens, dark_mode, cx);
    if let Some(window) = window {
        window.refresh();
    }
}

fn paint(t: &Tokens, dark_mode: bool, cx: &mut App) {
    let theme = Theme::global_mut(cx);
    theme.font_family = UI_FONT.into();
    theme.font_size = px(TEXT_BODY);
    theme.mono_font_family = MONO_FONT.into();
    theme.mono_font_size = px(TEXT_MONO);
    // Every widget rounds off `radius`, and everything gpui-component draws for
    // us is pill-shaped in the reference design.
    theme.radius = px(RADIUS_PILL);
    theme.radius_lg = px(RADIUS_PILL);
    theme.shadow = false;

    let c = &mut theme.colors;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn all(t: &Tokens) -> [Hsla; 26] {
        [
            t.bg,
            t.fg,
            t.card,
            t.popover,
            t.primary,
            t.primary_fg,
            t.muted,
            t.muted_fg,
            t.border,
            t.input,
            t.input_fill,
            t.ring,
            t.sidebar,
            t.sidebar_border,
            t.success,
            t.warning,
            t.info,
            t.danger,
            t.diff_add,
            t.diff_del,
            t.diff_add_bg,
            t.diff_add_emph,
            t.diff_del_bg,
            t.diff_del_emph,
            t.gutter,
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

    #[test]
    fn mixing_stays_between_the_two_ends() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), rgb(0x000000).into());
        assert_eq!(mix(0x000000, 0xffffff, 1.0), rgb(0xffffff).into());
        assert_eq!(mix(0x000000, 0xffffff, 0.5), rgb(0x808080).into());
    }

    /// The row tint has to stay a tint: closer to the page than to the sign
    /// colour, or the diff turns into a wall of green and red.
    #[test]
    fn diff_row_tint_is_closer_to_the_background() {
        let t = dark();
        assert_eq!(t.diff_add_bg, mix(0x0a0a0a, 0x5ecc71, 0.2));
        assert_ne!(t.diff_add_bg, t.diff_add);
    }
}
