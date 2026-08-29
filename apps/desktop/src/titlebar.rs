//! The window bar: a transparent titlebar we draw ourselves, spanning the
//! sidebar and the main pane.

use crate::app::LgtmApp;
use crate::sidebar;
use crate::theme::{glyph, tokens, Tokens, BAR_H, LIGHTS_W, SPACE};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, ClickEvent, Context, Div, InteractiveElement as _, MouseButton, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled as _, WindowControlArea,
};
use gpui_component::InteractiveElementExt as _;

pub fn bar(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let t = tokens(cx);
    let open = app.sidebar_open;
    div()
        .id("window-bar")
        .flex()
        .flex_shrink_0()
        .h(px(BAR_H))
        .window_control_area(WindowControlArea::Drag)
        .on_double_click(|_, window, _| window.titlebar_double_click())
        // macOS has no drag region for a transparent titlebar, so a press that
        // turns into a move hands the window to the compositor.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.dragging = true),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.dragging = false),
        )
        .on_mouse_move(cx.listener(|this, _, window, _| {
            if this.dragging {
                this.dragging = false;
                window.start_window_move();
            }
        }))
        .when(open, |this| {
            this.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .w(px(sidebar::WIDTH))
                    .h_full()
                    .bg(t.sidebar)
                    .border_r_1()
                    .border_color(t.sidebar_border)
                    .child(cluster(app, &t, cx)),
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
                .when(!open, |this| this.child(cluster(app, &t, cx)))
                .child(div().flex_1())
                .child(glyph("settings-menu", "…", true, &t).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(false, cx)),
                )),
        )
}

/// Sidebar toggle and task history, inset past the traffic lights.
fn cluster(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let back = app.can_go_back();
    let forward = app.can_go_forward();
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .pl(px(LIGHTS_W))
        .child(
            glyph("toggle-sidebar", "◧", true, t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_sidebar(cx))),
        )
        .child(div().w(px(SPACE[1])))
        .child(
            glyph("history-back", "‹", back, t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_back(cx))),
        )
        .child(
            glyph("history-forward", "›", forward, t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.go_forward(cx))),
        )
}
