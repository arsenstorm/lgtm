//! The settings dialog: where this app points, how it looks, and how another
//! machine joins as a worker.

use crate::app::LgtmApp;
use crate::theme::{
    icon_button, panel, scrim, section_label, tokens, Pref, Tokens, HEADER_H, MONO_FONT, RADIUS,
    ROW_H, SPACE, TEXT_MONO,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, relative, AnyElement, ClickEvent, ClipboardItem, Context, Div, FontWeight,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Selectable as _, Sizable as _};
use lgtm_diff::DiffStyle;

const WIDTH: f32 = 640.;
const MAX_BODY_H: f32 = 520.;
/// Index of the Workers section among the dialog's scrolled children.
pub const WORKERS_SECTION: usize = 2;

/// `lgtm worker` takes a ws(s) URL; the app holds the http(s) one.
fn ws_url(orchestrator: &str) -> String {
    match orchestrator.trim_end_matches('/').split_once("://") {
        Some(("http", rest)) => format!("ws://{rest}"),
        Some(("https", rest)) => format!("wss://{rest}"),
        _ => orchestrator.trim_end_matches('/').to_string(),
    }
}

/// The line a person pastes on the machine they want to add.
pub fn join_line(orchestrator: &str, token: &str) -> String {
    format!("lgtm worker {} --token {token}", ws_url(orchestrator))
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(HEADER_H))
                        .px(px(SPACE[2]))
                        .border_b_1()
                        .border_color(t.border)
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Settings"),
                        )
                        .child(
                            icon_button("settings-close", "x", true, &t).on_click(cx.listener(
                                |this, _: &ClickEvent, window, cx| this.close_overlay(window, cx),
                            )),
                        ),
                )
                .child(
                    div()
                        .id("settings-body")
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[3]))
                        .max_h(px(MAX_BODY_H))
                        .overflow_y_scroll()
                        .track_scroll(&app.settings_scroll)
                        .p(px(SPACE[2]))
                        .child(orchestrator(app, &t))
                        .child(appearance(app, &t, cx))
                        .child(workers(app, &t, cx))
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

fn orchestrator(app: &LgtmApp, t: &Tokens) -> Div {
    let token = match app.token_source {
        "LGTM_TOKEN" => "environment (LGTM_TOKEN)".to_string(),
        other => other.to_string(),
    };
    section("Orchestrator", t)
        .child(line("URL", app.orchestrator.clone(), t))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(div().w(px(120.)).text_color(t.muted_fg).child("Connection"))
                .child(
                    div()
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(if app.reachable { t.success } else { t.danger }),
                )
                .child(if app.reachable {
                    "Connected"
                } else {
                    "Unreachable"
                }),
        )
        .child(line("Token", token, t))
}

fn appearance(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let current = crate::theme::pref(cx);
    let style = app.review.style;
    section("Appearance", t)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(div().w(px(120.)).text_color(t.muted_fg).child("Theme"))
                .children(Pref::ALL.map(|pref| {
                    Button::new(SharedString::from(format!("theme-{}", pref.label())))
                        .label(pref.label())
                        .xsmall()
                        .ghost()
                        .selected(pref == current)
                        .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                            crate::theme::set_pref(pref, window, cx);
                            cx.notify();
                        }))
                })),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(div().w(px(120.)).text_color(t.muted_fg).child("Diff"))
                .children(
                    [(DiffStyle::Unified, "Unified"), (DiffStyle::Split, "Split")].map(
                        |(value, label)| {
                            Button::new(SharedString::from(format!("diff-{label}")))
                                .label(label)
                                .xsmall()
                                .ghost()
                                .selected(style == value)
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.review.set_style(value);
                                    cx.notify();
                                }))
                        },
                    ),
                ),
        )
}

fn workers(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let join = join_line(&app.orchestrator, &app.token);
    let copy = join.clone();
    section("Workers", t)
        .when(app.workers.is_empty(), |this| {
            this.child(div().text_color(t.muted_fg).child("None connected"))
        })
        .children(app.workers.iter().map(|worker| {
            let kind = if worker.info.ephemeral {
                "ephemeral"
            } else {
                "fixed"
            };
            div().flex().items_center().h(px(ROW_H)).child(format!(
                "{} · {kind} · {}/{}",
                worker.info.name,
                worker.running.len(),
                worker.info.slots
            ))
        }))
        .child(
            div()
                .pt(px(SPACE[1]))
                .text_color(t.muted_fg)
                .child("Add a machine — paste this on it:"),
        )
        .child(
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
                ),
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
            "lgtm worker ws://127.0.0.1:4750 --token abc"
        );
        assert_eq!(
            join_line("https://host:4750/", "abc"),
            "lgtm worker wss://host:4750 --token abc"
        );
    }
}
