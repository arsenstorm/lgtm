//! The window bar: a transparent titlebar we draw ourselves, spanning the
//! sidebar and the main pane.

use crate::app::LgtmApp;
use crate::theme::{icon_button, tokens, Tokens, BAR_H, LIGHTS_W, SPACE};
use crate::{panes, session, sidebar};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, ClickEvent, Context, Div, InteractiveElement as _, MouseButton, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled as _, WindowControlArea,
};
use gpui_component::InteractiveElementExt as _;

pub fn bar(app: &mut LgtmApp, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let t = tokens(cx);
    let open = app.ui.sidebar_open;
    let task = app.selected_task().cloned();
    let in_session = app.selected.is_none() && matches!(&app.page, crate::app::Page::Session(_));
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
                .when_some(task, |this, task| {
                    this.child(panes::task_header(app, &task, &t, cx))
                })
                .when(in_session, |this| {
                    this.child(session::session_header(app, &t, cx))
                })
                .when(!open, |this| {
                    this.child(cluster(&t, LIGHTS_W - SPACE[1], cx))
                }),
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
