//! The settings dialog: where this app points, how it looks, and how another
//! machine joins as a runner.

use crate::app::LgtmApp;
use crate::theme::{
    field, icon, modal_header, panel, scrim, section_label, tokens, Pick, Pref, Tokens, ICON,
    MONO_FONT, RADIUS, RADIUS_PILL, SPACE, TEXT_MONO,
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

const WIDTH: f32 = 640.;
const MAX_BODY_H: f32 = 520.;
/// Index of the Runners section among the dialog's scrolled children.
pub const RUNNERS_SECTION: usize = 4;
/// One dropdown: its trigger, and the menu it opens.
const TRIGGER_H: f32 = 28.;
const MENU_W: f32 = 160.;

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
                .child(modal_header("Settings", "settings-close", &t, cx))
                .child(
                    div()
                        .id("settings-body")
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[3]))
                        .max_h(px(MAX_BODY_H))
                        .overflow_y_scroll()
                        .track_scroll(&app.ui.settings_scroll)
                        .p(px(SPACE[2]))
                        .child(orchestrator(app, &t, cx))
                        .child(models(app, &t, cx))
                        .child(appearance(app, &t, cx))
                        .child(notifications(&t, cx))
                        .child(runners(app, &t, cx))
                        .child(about(&t)),
                ),
        )
        .into_any_element()
}

fn section(title: &'static str, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label(title, t))
}

fn line(label: &'static str, value: impl Into<SharedString>, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().w(px(120.)).text_color(t.muted_fg).child(label))
        .child(div().flex_1().min_w_0().truncate().child(value.into()))
}

fn orchestrator(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let token = match app.link.token_source {
        "LGTM_TOKEN" => "environment (LGTM_TOKEN)".to_string(),
        other => other.to_string(),
    };
    section("Orchestrator", t)
        .child(line("URL", app.link.orchestrator.clone(), t))
        .child(line(
            "Mode",
            if app.link.hosted {
                "hosted by this app"
            } else {
                "external"
            },
            t,
        ))
        .child(connection_row(app.link.reachable, t))
        .child(line("Token", token, t))
        .child(embedded_row(app.link.embedded, t, cx))
        .child(
            div()
                .pl(px(120. + SPACE[1]))
                .text_color(t.muted_fg)
                .child("Takes effect the next time you open the app."),
        )
}

fn connection_row(reachable: bool, t: &Tokens) -> Div {
    let (tone, word) = if reachable {
        (t.success, "Connected")
    } else {
        (t.danger, "Unreachable")
    };
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().w(px(120.)).text_color(t.muted_fg).child("Connection"))
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(tone))
        .child(word)
}

fn embedded_row(embedded: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().w(px(120.)).text_color(t.muted_fg).child("Embedded"))
        .child(
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
fn models(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let held = crate::theme::models(cx);
    section("Models", t)
        .child(choice_row("Executor", t).child(dropdown(
            "executor",
            Pick::of(held.executor),
            &Pick::HARNESSES,
            app,
            t,
            cx,
            |models, pick| models.executor = pick.executor().unwrap_or_default(),
        )))
        .child(
            choice_row("Model", t).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(field(&app.inputs.model, t).small()),
            ),
        )
        .child(choice_row("Review", t).child(dropdown(
            "review",
            held.review,
            &Pick::REVIEW,
            app,
            t,
            cx,
            |models, pick| models.review = pick,
        )))
        .child(choice_row("Orchestrate", t).child(dropdown(
            "orchestrate",
            held.orchestrate,
            &Pick::ORCHESTRATE,
            app,
            t,
            cx,
            |models, pick| models.orchestrate = pick,
        )))
        .child(
            div()
                .pl(px(120. + SPACE[1]))
                .text_color(t.muted_fg)
                .child("Orchestration is stored only; this app does not run an orchestrator loop."),
        )
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

fn appearance(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    section("Appearance", t)
        .child(choice_row("Theme", t).children(theme_buttons(cx)))
        .child(choice_row("Diff", t).children(diff_buttons(app.review.style, cx)))
}

fn notifications(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    section("Notifications", t).child(
        Switch::new("notify")
            .checked(crate::theme::notify(cx))
            .label("Tell me when a task needs a person")
            .small()
            .on_click(cx.listener(|_, checked: &bool, _, cx| {
                crate::theme::set_notify(*checked, cx);
                cx.notify();
            })),
    )
}

fn choice_row(label: &'static str, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().w(px(120.)).text_color(t.muted_fg).child(label))
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

fn runners(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    // When this app hosts the orchestrator its own URL is loopback, so the
    // startup-computed line (advertised address) is the one to hand out.
    let join = app
        .link
        .join
        .clone()
        .unwrap_or_else(|| join_line(&app.link.orchestrator, &app.link.token));
    section("Runners", t)
        .child(
            div()
                .text_color(t.muted_fg)
                .child("Runners moved to the project page."),
        )
        .child(
            div()
                .pt(px(SPACE[1]))
                .text_color(t.muted_fg)
                .child("Add a machine — paste this on it:"),
        )
        .child(join_row(join, t, cx))
}

/// The join line in a mono box, with a Copy button beside it.
fn join_row(join: String, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let copy = join.clone();
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
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

fn about(t: &Tokens) -> Div {
    section("About", t).child(
        div()
            .text_color(t.muted_fg)
            .child(format!("lgtm-desktop {}", env!("CARGO_PKG_VERSION"))),
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
