//! The Activity and Plan tabs.

use super::muted;
use crate::app::LgtmApp;
use crate::render::Kind;
use crate::theme::{Tokens, LINE_MONO, MONO_FONT, SPACE, TEXT_MONO, TEXT_ROW, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{div, px, AnyElement, FontWeight, IntoElement, ParentElement as _, Styled as _};
use lgtm_protocol::Task;

/// The one genuinely monospace pane: a stream of command lines and output,
/// where a colour means something is wrong rather than something happened.
pub(super) fn activity(app: &LgtmApp, t: &Tokens) -> AnyElement {
    if app.lines.is_empty() {
        return muted("Nothing yet.", t);
    }
    div()
        .flex()
        .flex_col()
        .font_family(MONO_FONT)
        .text_size(px(TEXT_MONO))
        .line_height(px(LINE_MONO))
        .children(app.lines.iter().map(|line| {
            let color = match line.kind {
                Kind::Text | Kind::Message => t.fg,
                Kind::Tool | Kind::Status => t.muted_fg,
                Kind::Stderr => t.danger,
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
        .gap(px(SPACE[3]))
        .text_size(px(TEXT_ROW))
        .children(plan.steps.iter().enumerate().map(|(i, step)| {
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE[0]))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(SPACE[1]))
                        .child(div().font_weight(FontWeight::MEDIUM).child(format!(
                            "{}. {}",
                            i + 1,
                            step.title
                        )))
                        .child(
                            div()
                                .text_size(px(TEXT_SECONDARY))
                                .text_color(t.muted_fg)
                                .child(step.key.clone()),
                        ),
                )
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
