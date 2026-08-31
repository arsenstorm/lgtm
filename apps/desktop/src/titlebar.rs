//! The window bar: a transparent titlebar we draw ourselves, spanning the
//! sidebar and the main pane.

use crate::app::LgtmApp;
use crate::sidebar;
use crate::theme::{
    icon_button, tokens, Tokens, BAR_H, LIGHTS_W, RADIUS, ROW_H, SPACE, TEXT_SECONDARY,
};

/// The runner pill: shorter than an icon button, so it reads as a status.
const PILL_H: f32 = 22.;
const POPOVER_W: f32 = 240.;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, ClickEvent, Context, Div, InteractiveElement as _, MouseButton, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled as _, WindowControlArea,
};
use gpui_component::InteractiveElementExt as _;

pub fn bar(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let t = tokens(cx);
    let open = app.ui.sidebar_open;
    draggable(div().id("window-bar"), cx)
        .flex()
        .flex_shrink_0()
        .h(px(BAR_H))
        .when(open, |this| {
            this.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .w(px(sidebar::WIDTH))
                    .h_full()
                    .bg(t.sidebar)
                    .child(cluster(&t, LIGHTS_W, cx)),
            )
        })
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .h_full()
                .px(px(SPACE[1]))
                .bg(t.bg)
                .border_b_1()
                .border_color(t.border)
                .when(!open, |this| {
                    this.child(cluster(&t, LIGHTS_W - SPACE[1], cx))
                })
                .child(div().flex_1())
                .child(runners(app, &t, cx)),
        )
}

/// macOS has no drag region for a transparent titlebar, so a press that
/// turns into a move hands the window to the compositor. The controls sitting
/// on the bar `occlude()` it, or two quick clicks on one of them would reach
/// this double click and zoom the window.
fn draggable(bar: Stateful<Div>, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    bar.window_control_area(WindowControlArea::Drag)
        .on_double_click(|_, window, _| window.titlebar_double_click())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.ui.dragging = true),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.ui.dragging = false),
        )
        .on_mouse_move(cx.listener(|this, _, window, _| {
            if this.ui.dragging {
                this.ui.dragging = false;
                window.start_window_move();
            }
        }))
}

/// How the orchestrator is doing, and the runners it has, in one pill.
fn runners(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .relative()
        .flex_shrink_0()
        .child(pill(app, t, cx))
        .when(app.ui.runner_menu, |this| {
            this.child(dismiss(cx)).child(popover(app, t))
        })
}

fn pill(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let n = app.runners.len();
    let (tone, label) = match app.link.reachable {
        true => (
            t.success,
            format!("Connected · {n} runner{}", if n == 1 { "" } else { "s" }),
        ),
        false => (t.danger, "Disconnected".to_string()),
    };
    div()
        .id("runner-pill")
        .occlude()
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(PILL_H))
        .px(px(SPACE[1]))
        .rounded(px(RADIUS))
        .cursor_pointer()
        .text_size(px(TEXT_SECONDARY))
        .text_color(if app.link.reachable { t.muted_fg } else { tone })
        .hover(|this| this.bg(t.muted))
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(tone))
        .child(label)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            let open = !this.ui.runner_menu;
            this.close_menus(cx);
            this.ui.runner_menu = open;
            cx.notify();
        }))
}

/// One row per runner: what it is called, how loaded it is, what it runs on.
fn popover(app: &LgtmApp, t: &Tokens) -> Div {
    div()
        .absolute()
        .top(px(PILL_H + SPACE[0]))
        .right(px(0.))
        .w(px(POPOVER_W))
        .flex()
        .flex_col()
        .p(px(SPACE[0]))
        .rounded(px(RADIUS))
        .bg(t.popover)
        .border_1()
        .border_color(t.border)
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .occlude()
        .when(app.runners.is_empty(), |this| {
            this.child(div().px(px(SPACE[1])).py(px(SPACE[0])).child("No runners"))
        })
        .children(app.runners.iter().map(|runner| {
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .h(px(ROW_H))
                .px(px(SPACE[1]))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(t.fg)
                        .child(runner.info.name.clone()),
                )
                .child(format!("{}/{}", runner.running.len(), runner.info.slots))
                .child(runner.info.os.clone())
        }))
}

/// A click anywhere else closes the popover; the titlebar is the only
/// positioned ancestor, so this reaches out past its own bounds.
fn dismiss(cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    div()
        .id("runner-dismiss")
        .absolute()
        .top(px(-4000.))
        .left(px(-4000.))
        .w(px(8000.))
        .h(px(8000.))
        .occlude()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_menus(cx)))
}

/// The sidebar toggle, inset past the traffic lights. `pad` lands it on the
/// same window x in both states: open, it sits in the sidebar strip, which has
/// no padding of its own; closed, in the main pane, which does.
fn cluster(t: &Tokens, pad: f32, cx: &mut Context<LgtmApp>) -> Div {
    div().flex().items_center().pl(px(pad)).child(
        icon_button("toggle-sidebar", "panel-left", true, t)
            .occlude()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_sidebar(cx))),
    )
}
