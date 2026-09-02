//! The design system: the shadcn "neutral" palette the Tauri app used, pushed
//! into gpui-component's `Theme` so its own widgets (buttons, inputs,
//! dropdowns) match the hand-rolled parts of the shell.

mod palette;
mod prefs;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, Div, Entity, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::input::{Input, InputState};
use gpui_component::ActiveTheme as _;

/// 8-pt scale with a 4 for tight gaps.
pub const SPACE: [f32; 7] = [4., 8., 12., 16., 24., 32., 48.];
/// `text-sm`.
pub const TEXT_BODY: f32 = 15.;
/// `text-xs`: secondary copy, section labels, status bars.
pub const TEXT_SECONDARY: f32 = 13.;
/// One list row: sidebar entries, menu items, composer controls.
pub const TEXT_ROW: f32 = 14.;
/// File paths and code.
pub const TEXT_MONO: f32 = 13.;
/// `text-[11px]`: the +/- counts next to a file.
pub const TEXT_COUNT: f32 = 12.;
/// One diff/log row.
pub const LINE_MONO: f32 = 21.;
pub const MONO_FONT: &str = "Menlo";
/// gpui resolves this to the platform UI font (San Francisco on macOS).
pub const UI_FONT: &str = ".SystemUIFont";

/// `--radius`: cards and comment cards.
pub const RADIUS: f32 = 10.;
/// `rounded-2xl`: buttons, inputs, tabs, badges, icon tiles — the signature.
pub const RADIUS_PILL: f32 = 16.;
/// `rounded-md`: one list row, in the rail or in a dialog's nav. A pill this
/// short would read as a lozenge, not a row.
pub const ROW_RADIUS: f32 = 8.;

/// `h-11`: the window and task headers.
pub const HEADER_H: f32 = 44.;
/// `h-6`: the status/footer bars.
pub const STATUS_H: f32 = 24.;
/// One sidebar row.
pub const ROW_H: f32 = 28.;
/// The window bar drawn over the sidebar and the main pane.
pub const BAR_H: f32 = 38.;
/// The traffic lights plus their breathing room.
pub const LIGHTS_W: f32 = 78.;

/// The composer, layer by layer. Codex builds its prompt out of luminance
/// alone: a darker panel behind, a lighter card over it, one hairline edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Composer {
    pub rear: Hsla,
    pub card: Hsla,
    pub edge: Hsla,
    pub placeholder: Hsla,
    /// The dimmer of the two label greys: icons, the second half of a control.
    pub secondary: Hsla,
    pub primary: Hsla,
    pub divider: Hsla,
    pub send_bg: Hsla,
    pub send_fg: Hsla,
    pub send_disabled_bg: Hsla,
    pub send_disabled_fg: Hsla,
}

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
    /// A translucent hover/active fill. The sidebar is alpha over the blurred
    /// window, so an opaque grey there reads as a pasted-on slab; this wash
    /// blends with whatever is behind it.
    pub wash: Hsla,
    pub muted_fg: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    /// `bg-input/50`: what a borderless field is filled with.
    pub input_fill: Hsla,
    pub ring: Hsla,
    pub sidebar: Hsla,
    pub sidebar_border: Hsla,
    /// One grey for every sidebar row and icon: the rail reads as one quiet
    /// list, and the active pill alone marks the current row.
    pub sidebar_fg: Hsla,
    /// Section labels and "show more": clearly dimmer than the rows they head.
    pub sidebar_muted: Hsla,
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
    /// The scrim a modal lays over the window.
    pub overlay: Hsla,
    pub composer: Composer,
}

/// The 3% lift a hovered diff row gets, applied to whatever tint it carries so
/// an addition row stays green under the cursor.
pub fn lighten(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l + 0.03).min(1.),
        ..color
    }
}

/// `font-variant-numeric: tabular-nums`. Digits share one width, so a ticking
/// age, duration or cost doesn't nudge whatever sits beside it.
pub trait TabularNums: gpui::Styled + Sized {
    fn tabular_nums(mut self) -> Self {
        let style = self.text_style().get_or_insert_with(Default::default);
        style.font_features = Some(gpui::FontFeatures(std::sync::Arc::new(vec![(
            "tnum".into(),
            1,
        )])));
        self
    }
}
impl<T: gpui::Styled> TabularNums for T {}

/// A borderless `bg-input/50` pill: every text field in the reference design.
pub fn field(state: &Entity<InputState>, t: &Tokens) -> Input {
    Input::new(state)
        .bordered(false)
        .bg(t.input_fill)
        .rounded(px(RADIUS_PILL))
}

/// A calm section name used across the app.
pub fn section_label(text: &str, t: &Tokens) -> Div {
    div()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .font_weight(FontWeight::MEDIUM)
        .child(text.to_string())
}

/// A section name followed by its content.
pub fn section(text: &str, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label(text, t))
}

/// The common anatomy of every user-facing header: an optional leading
/// control, a truncating title, supporting details, and trailing actions.
pub struct Header {
    title: SharedString,
    leading: Vec<AnyElement>,
    details: Vec<AnyElement>,
    actions: Vec<AnyElement>,
}

impl Header {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            leading: Vec::new(),
            details: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading.push(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(GLYPH))
                .child(element)
                .into_any_element(),
        );
        self
    }

    pub fn detail(mut self, element: impl IntoElement) -> Self {
        self.details.push(element.into_any_element());
        self
    }

    pub fn details(mut self, elements: impl IntoIterator<Item = AnyElement>) -> Self {
        self.details.extend(elements);
        self
    }

    pub fn action(mut self, element: impl IntoElement) -> Self {
        self.actions.push(element.into_any_element());
        self
    }

    pub fn render(self) -> Div {
        let content = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(SPACE[1]))
            .children(self.leading)
            .child(
                div()
                    .flex_shrink()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.title),
            )
            .children(self.details);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(SPACE[1]))
            .h_full()
            .child(content)
            .children(self.actions)
    }
}

/// The scrim a modal lays over the window. Children stack from the top, so a
/// panel can sit a fixed distance down the window.
pub fn scrim(id: &'static str, t: &Tokens) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .bg(t.overlay)
        .flex()
        .flex_col()
        .items_center()
}

/// A modal panel: the popover surface with the card radius.
pub fn panel(t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .overflow_hidden()
        .rounded(px(RADIUS))
        .bg(t.popover)
        .border_1()
        .border_color(t.border)
}

/// A modal's title row with its close cross.
pub fn modal_header(
    title: &'static str,
    close_id: &'static str,
    t: &Tokens,
    cx: &mut gpui::Context<crate::app::LgtmApp>,
) -> Div {
    Header::new(title)
        .action(icon_button(close_id, "x", true, t).on_click(
            cx.listener(|this, _: &gpui::ClickEvent, window, cx| this.close_overlay(window, cx)),
        ))
        .render()
        // The header grows to fill a row; in a modal's column it must not.
        .flex_none()
        .h(px(HEADER_H))
        .px(px(SPACE[2]))
        .border_b_1()
        .border_color(t.border)
}

/// One Lucide icon, tinted with `color`. gpui paints an SVG as an alpha mask,
/// so the colour has to be set on the element itself — it isn't inherited.
pub fn icon(name: &str, size: f32, color: Hsla) -> impl IntoElement {
    gpui::svg()
        .path(format!("icons/{name}.svg"))
        .flex_none()
        .size(px(size))
        .text_color(color)
}

/// A square icon button: the window bar's controls, the composer's `+`, the
/// close crosses. Muted until hovered, when the icon goes full strength.
pub fn icon_button(
    id: impl Into<gpui::SharedString>,
    name: &str,
    enabled: bool,
    t: &Tokens,
) -> Stateful<Div> {
    let group: gpui::SharedString = id.into();
    div()
        .id(group.clone())
        .group(group.clone())
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(GLYPH))
        .h(px(GLYPH))
        .rounded(px(GLYPH / 2.))
        .when(enabled, |this| {
            this.cursor_pointer().hover(|this| this.bg(t.wash))
        })
        .child(
            gpui::svg()
                .path(format!("icons/{name}.svg"))
                .flex_none()
                .size(px(ICON))
                .text_color(if enabled { t.muted_fg } else { t.border })
                .when(enabled, |this| {
                    this.group_hover(group, |this| this.text_color(t.fg))
                }),
        )
}

/// The icon button's box.
pub const GLYPH: f32 = 24.;
/// Every icon in the chrome is drawn at this size.
pub const ICON: f32 = 16.;

pub use palette::{dark, light};
pub use prefs::{
    apply, config, init, models, notify, persist, pref, set_models, set_notify, set_pref, Models,
    Pick, Pref,
};

pub fn tokens(cx: &App) -> Tokens {
    if cx.theme().mode.is_dark() {
        dark()
    } else {
        light()
    }
}
