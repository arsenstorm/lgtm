//! The settings dialog: where this app points, how it looks, and how another
//! machine joins as a runner.

use crate::app::LgtmApp;
use crate::theme::{
    field, icon, modal_header, panel, scrim, tokens, Pick, Pref, Tokens, ICON, MONO_FONT, RADIUS,
    RADIUS_PILL, SPACE, TEXT_MONO,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, relative, AnyElement, ClickEvent, ClipboardItem, Context, Div,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::switch::Switch;
use gpui_component::{Selectable as _, Sizable as _};
use lgtm_diff::DiffStyle;

const WIDTH: f32 = 720.;
const HEIGHT: f32 = 480.;
const NAV_W: f32 = 180.;
/// The label column every row shares, so the controls line up.
const LABEL_W: f32 = 120.;
/// A row is taller than its text: the list reads as a list, not a paragraph.
const ROW_H: f32 = 40.;
const NAV_ROW_H: f32 = 32.;
/// Index of the join line among the Orchestrator pane's rows.
pub const RUNNERS_SECTION: usize = 7;
/// One dropdown: its trigger, and the menu it opens.
const TRIGGER_H: f32 = 28.;
const MENU_W: f32 = 160.;

/// One page of the dialog. The left nav picks it; the right pane shows it.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Section {
    #[default]
    General,
    Orchestrator,
    Models,
}

impl Section {
    const ALL: [Section; 3] = [Section::General, Section::Orchestrator, Section::Models];

    fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Orchestrator => "Orchestrator",
            Section::Models => "Models",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::General => "settings",
            Section::Orchestrator => "server",
            Section::Models => "cpu",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::General => "section-general",
            Section::Orchestrator => "section-orchestrator",
            Section::Models => "section-models",
        }
    }
}

/// `lgtm runner` takes a ws(s) URL; the app holds the http(s) one.
fn ws_url(orchestrator: &str) -> String {
    match orchestrator.trim_end_matches('/').split_once("://") {
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("https", rest)) => format!("wss://{rest}"),
        _ => orchestrator.trim_end_matches('/').to_string(),
    }
}

/// The line a person pastes on the machine they want to add.
pub fn join_line(orchestrator: &str, token: &str) -> String {
    format!("lgtm runner {} --token {token}", ws_url(orchestrator))
}

pub fn view(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    scrim("settings-scrim", &t)
        .pt(relative(0.12))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            panel(&t)
                .id("settings")
                .w(px(WIDTH))
                .on_click(|_, _, cx| cx.stop_propagation())
                .h(px(HEIGHT))
                .child(modal_header("Settings", "settings-close", &t, cx))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .child(nav(app, &t, cx))
                        .child(pane(app, &t, cx)),
                ),
        )
        .into_any_element()
}

fn nav(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .gap(px(SPACE[0]))
        .w(px(NAV_W))
        .p(px(SPACE[1]))
        .border_r_1()
        .border_color(t.border)
        .children(Section::ALL.map(|section| nav_row(section, app.ui.settings_section, t, cx)))
}

fn nav_row(
    section: Section,
    current: Section,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    let selected = section == current;
    div()
        .id(section.id())
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(NAV_ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(RADIUS_PILL))
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .when(selected, |this| this.bg(t.muted))
        .child(icon(
            section.icon(),
            ICON,
            if selected { t.fg } else { t.muted_fg },
        ))
        .child(section.label())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.ui.settings_section = section;
            // The open dropdown belongs to the pane being left.
            this.ui.settings_menu = None;
            cx.notify();
        }))
}

fn pane(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    div()
        .id("settings-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .overflow_y_scroll()
        .track_scroll(&app.ui.settings_scroll)
        .p(px(SPACE[2]))
        .children(match app.ui.settings_section {
            Section::General => general(app, t, cx),
            Section::Orchestrator => orchestrator(app, t, cx),
            Section::Models => models(app, t, cx),
        })
}

/// One row: the shared label column, then whatever the row controls.
fn row(label: &'static str, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .min_h(px(ROW_H))
        .child(
            div()
                .w(px(LABEL_W))
                .flex_none()
                .text_color(t.muted_fg)
                .child(label),
        )
}

/// A footnote under a row, hanging off the control column.
fn note(text: &'static str, t: &Tokens) -> Div {
    div()
        .pb(px(SPACE[1]))
        .pl(px(LABEL_W + SPACE[1]))
        .text_color(t.muted_fg)
        .child(text)
}

fn line(label: &'static str, value: impl Into<SharedString>, t: &Tokens) -> Div {
    row(label, t).child(div().flex_1().min_w_0().truncate().child(value.into()))
}

fn orchestrator(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let token = match app.link.token_source {
        "LGTM_TOKEN" => "environment (LGTM_TOKEN)".to_string(),
        other => other.to_string(),
    };
    // When this app hosts the orchestrator its own URL is loopback, so the
    // startup-computed line (advertised address) is the one to hand out.
    let join = app
        .link
        .join
        .clone()
        .unwrap_or_else(|| join_line(&app.link.orchestrator, &app.link.token));
    vec![
        line("URL", app.link.orchestrator.clone(), t).into_any_element(),
        line(
            "Mode",
            if app.link.hosted {
                "hosted by this app"
            } else {
                "external"
            },
            t,
        )
        .into_any_element(),
        connection_row(app.link.reachable, t).into_any_element(),
        line("Token", token, t).into_any_element(),
        embedded_row(app.link.embedded, t, cx).into_any_element(),
        note("Takes effect the next time you open the app.", t).into_any_element(),
        note("Runners live on the project page.", t).into_any_element(),
        join_row(join, t, cx).into_any_element(),
    ]
}

fn connection_row(reachable: bool, t: &Tokens) -> Div {
    let (tone, word) = if reachable {
        (t.success, "Connected")
    } else {
        (t.danger, "Unreachable")
    };
    row("Connection", t)
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(tone))
        .child(word)
}

fn embedded_row(embedded: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    row("Embedded", t).child(
        Switch::new("embedded-orchestrator")
            .checked(embedded)
            .label("Run the orchestrator inside this app")
            .small()
            .on_click(cx.listener(|this, checked: &bool, _, cx| {
                this.link.embedded = *checked;
                let dir = lgtm_orchestrator::token::data_dir(None);
                if let Err(e) = crate::save_embedded(&dir, *checked) {
                    this.set_error(format!("cannot save the setting: {e}"), cx);
                }
                cx.notify();
            })),
    )
}

/// What a task the composer starts is run with. Orchestration is stored and
/// shown, but only an orchestrator this app hosts would ever read it.
fn models(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let held = crate::theme::models(cx);
    vec![
        row("Executor", t)
            .child(dropdown(
                "executor",
                Pick::of(held.executor),
                &Pick::HARNESSES,
                app,
                t,
                cx,
                |models, pick| models.executor = pick.executor().unwrap_or_default(),
            ))
            .into_any_element(),
        row("Model", t)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(field(&app.inputs.model, t).small()),
            )
            .into_any_element(),
        row("Review", t)
            .child(dropdown(
                "review",
                held.review,
                &Pick::REVIEW,
                app,
                t,
                cx,
                |models, pick| models.review = pick,
            ))
            .into_any_element(),
        row("Orchestrate", t)
            .child(dropdown(
                "orchestrate",
                held.orchestrate,
                &Pick::ORCHESTRATE,
                app,
                t,
                cx,
                |models, pick| models.orchestrate = pick,
            ))
            .into_any_element(),
        note(
            "Orchestration is stored only; this app does not run an orchestrator loop.",
            t,
        )
        .into_any_element(),
    ]
}

/// A menu on a trigger: the app's own, since only one of these can be open and
/// the choice list is four items at most.
#[allow(clippy::too_many_arguments)]
fn dropdown(
    id: &'static str,
    current: Pick,
    options: &'static [Pick],
    app: &LgtmApp,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
    set: fn(&mut crate::theme::Models, Pick),
) -> Div {
    let open = app.ui.settings_menu == Some(id);
    div()
        .relative()
        .child(trigger(id, current, open, t, cx))
        .when(open, |this| {
            this.child(dismiss(id, cx))
                .child(menu(id, current, options, t, cx, set))
        })
}

fn trigger(
    id: &'static str,
    current: Pick,
    open: bool,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("trigger-{id}")))
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(TRIGGER_H))
        .w(px(MENU_W))
        .px(px(SPACE[1]))
        .rounded(px(RADIUS_PILL))
        .bg(t.input_fill)
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .child(div().flex_1().min_w_0().truncate().child(current.label()))
        .child(icon("chevron-down", ICON, t.muted_fg))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            let was = this.ui.settings_menu == Some(id);
            this.ui.settings_menu = (!was).then_some(id);
            cx.notify();
        }))
        .when(open, |this| this.bg(t.muted))
}

fn menu(
    id: &'static str,
    current: Pick,
    options: &'static [Pick],
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
    set: fn(&mut crate::theme::Models, Pick),
) -> Div {
    div()
        .absolute()
        .top(px(TRIGGER_H + SPACE[0]))
        .left(px(0.))
        .w(px(MENU_W))
        .flex()
        .flex_col()
        .p(px(SPACE[0]))
        .rounded(px(RADIUS))
        .bg(t.popover)
        .border_1()
        .border_color(t.border)
        .occlude()
        .children(options.iter().map(|pick| {
            let pick = *pick;
            div()
                .id(SharedString::from(format!("{id}-{}", pick.label())))
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .h(px(TRIGGER_H))
                .px(px(SPACE[1]))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|this| this.bg(t.muted))
                .child(div().flex_1().min_w_0().child(pick.label()))
                .when(pick == current, |this| this.child(icon("check", 14., t.fg)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let mut models = crate::theme::models(cx);
                    set(&mut models, pick);
                    crate::theme::set_models(models, cx);
                    this.ui.settings_menu = None;
                    cx.notify();
                }))
        }))
}

/// A click anywhere else closes the open dropdown.
fn dismiss(id: &'static str, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("dismiss-{id}")))
        .absolute()
        .top(px(-4000.))
        .left(px(-4000.))
        .w(px(8000.))
        .h(px(8000.))
        .occlude()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.ui.settings_menu = None;
            cx.notify();
        }))
}

fn general(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    vec![
        row("Theme", t)
            .children(theme_buttons(cx))
            .into_any_element(),
        row("Diff", t)
            .children(diff_buttons(app.review.style, cx))
            .into_any_element(),
        row("Notifications", t)
            .child(
                Switch::new("notify")
                    .checked(crate::theme::notify(cx))
                    .label("Tell me when a task needs a person")
                    .small()
                    .on_click(cx.listener(|_, checked: &bool, _, cx| {
                        crate::theme::set_notify(*checked, cx);
                        cx.notify();
                    })),
            )
            .into_any_element(),
        line(
            "Version",
            format!("lgtm-desktop {}", env!("CARGO_PKG_VERSION")),
            t,
        )
        .into_any_element(),
    ]
}

fn theme_buttons(cx: &mut Context<LgtmApp>) -> [Button; 3] {
    let current = crate::theme::pref(cx);
    Pref::ALL.map(|pref| {
        Button::new(SharedString::from(format!("theme-{}", pref.label())))
            .label(pref.label())
            .xsmall()
            .ghost()
            .selected(pref == current)
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                crate::theme::set_pref(pref, window, cx);
                cx.notify();
            }))
    })
}

fn diff_buttons(style: DiffStyle, cx: &mut Context<LgtmApp>) -> [Button; 2] {
    [(DiffStyle::Unified, "Unified"), (DiffStyle::Split, "Split")].map(|(value, label)| {
        Button::new(SharedString::from(format!("diff-{label}")))
            .label(label)
            .xsmall()
            .ghost()
            .selected(style == value)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.review.set_style(value);
                cx.notify();
            }))
    })
}

/// The join line in a mono box, with a Copy button beside it: what a person
/// pastes on the machine they are adding.
fn join_row(join: String, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let copy = join.clone();
    row("Add a machine", t)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .px(px(SPACE[1]))
                .py(px(SPACE[0]))
                .rounded(px(RADIUS))
                .bg(t.muted)
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .child(join),
        )
        .child(
            Button::new("copy-join")
                .label("Copy")
                .small()
                .outline()
                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_join_line_uses_the_websocket_scheme() {
        assert_eq!(
            join_line("http://127.0.0.1:4750", "abc"),
            "lgtm runner ws://127.0.0.1:4750 --token abc"
        );
        assert_eq!(
            join_line("https://host:4750/", "abc"),
            "lgtm runner wss://host:4750 --token abc"
        );
    }
}
