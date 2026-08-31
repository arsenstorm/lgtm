//! The Activity page: what everyone in the workspace has been doing.

use crate::app::LgtmApp;
use crate::labels::prompt_preview;
use crate::tasks::{now_ms, relative_age};
use crate::theme::{tokens, Tokens, HEADER_H, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _,
};
use lgtm_client::ActivityLine;

/// How much of a detail fits on one row.
const DETAIL: usize = 96;

pub fn page(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let empty = app.activity.is_empty();
    let now = now_ms();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(header(&t))
        .when(empty, |this| this.child(empty_state(&t)))
        .when(!empty, |this| {
            this.child(
                div()
                    .id("activity")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(SPACE[2]))
                    .p(px(SPACE[2]))
                    .children(app.activity.iter().map(|line| row(line, now, &t, cx))),
            )
        })
        .into_any_element()
}

fn header(t: &Tokens) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(HEADER_H))
        .px(px(SPACE[2]))
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .flex_1()
                .font_weight(FontWeight::MEDIUM)
                .child("Activity"),
        )
}

fn empty_state(t: &Tokens) -> Div {
    div().flex_1().flex().items_center().justify_center().child(
        div()
            .text_color(t.muted_fg)
            .child("Nothing has happened yet."),
    )
}

fn row(line: &ActivityLine, now: u64, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let task = line.task.clone();
    div()
        .id(SharedString::from(format!(
            "activity:{}:{}",
            line.task, line.at
        )))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .p(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .child(cell(relative_age(line.at, now), 48., t.muted_fg))
        .when_some(
            crate::app::owner_label(line.owner.clone(), t),
            |this, owner| this.child(owner),
        )
        .child(div().flex_shrink_0().child(line.event.clone()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(prompt_preview(&line.detail, DETAIL)),
        )
        .child(cell(line.task.clone(), 96., t.muted_fg))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(task.clone(), cx)))
}

fn cell(text: String, width: f32, tone: gpui::Hsla) -> Div {
    div()
        .w(px(width))
        .flex_shrink_0()
        .truncate()
        .text_size(px(TEXT_SECONDARY))
        .text_color(tone)
        .child(text)
}
