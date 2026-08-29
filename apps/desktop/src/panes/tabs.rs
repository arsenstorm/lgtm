//! The Activity and Plan tabs.

use super::muted;
use crate::app::LgtmApp;
use crate::render::Kind;
use crate::theme::{Tokens, SPACE};
use gpui::prelude::FluentBuilder as _;
use gpui::{div, px, AnyElement, FontWeight, IntoElement, ParentElement as _, Styled as _};
use lgtm_protocol::Task;

pub(super) fn activity(app: &LgtmApp, t: &Tokens) -> AnyElement {
    if app.lines.is_empty() {
        return muted("Nothing yet.", t);
    }
    div()
        .flex()
        .flex_col()
        .children(app.lines.iter().map(|line| {
            let color = match line.kind {
                Kind::Text => t.fg,
                Kind::Tool => t.info,
                Kind::Stderr => t.danger,
                Kind::Message => t.warning,
                Kind::Status => t.muted_fg,
            };
            div().text_color(color).child(line.text.clone())
        }))
        .into_any_element()
}

pub(super) fn plan_pane(task: &Task, t: &Tokens) -> AnyElement {
    let Some(plan) = task.result.as_ref().and_then(|r| r.plan.as_ref()) else {
        return muted("No plan.", t);
    };
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[2]))
        .children(plan.steps.iter().enumerate().map(|(i, step)| {
            div()
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::BOLD).child(format!(
                    "{}. {}  {}",
                    i + 1,
                    step.key,
                    step.title
                )))
                .child(div().text_color(t.muted_fg).child(step.prompt.clone()))
                .when(!step.depends_on.is_empty(), |this| {
                    this.child(
                        div()
                            .text_color(t.muted_fg)
                            .child(format!("after: {}", step.depends_on.join(", "))),
                    )
                })
        }))
        .into_any_element()
}
