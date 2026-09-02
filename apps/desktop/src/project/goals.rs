//! Goal cards: one per goal, with the tasks it is made of.

use super::goals_of;
use crate::app::LgtmApp;
use crate::labels::goal_status_label;
use crate::tasks::goal_color;
use crate::theme::{Tokens, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, Styled as _,
};
use lgtm_protocol::{BatchSummary, GoalSummary};

pub(super) fn cards(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    goals_of(app, slug)
        .into_iter()
        .map(|summary| card(app, summary, t, cx).into_any_element())
        .collect()
}

/// Non-zero task counts, folded the way the batch cards fold them.
pub fn counts(tasks: &BatchSummary) -> Vec<(&'static str, usize)> {
    [
        ("queued", tasks.queued),
        ("blocked", tasks.blocked),
        ("running", tasks.running),
        ("review", tasks.awaiting_review),
        ("approved", tasks.approved),
        ("merged", tasks.merged),
        ("failed", tasks.failed + tasks.cancelled + tasks.rejected),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .map(|(state, count)| (state, count as usize))
    .collect()
}

/// The card carries an id of its own: a task listed under a goal is listed
/// again in the section below it, and the two rows must not share one id.
fn card(
    app: &LgtmApp,
    summary: &GoalSummary,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    let owner = app.owner_name(summary.goal.created_by.as_deref());
    let id = summary.goal.id.clone();
    let rows = app
        .tasks
        .iter()
        .filter(move |task| task.spec.goal.as_deref() == Some(id.as_str()));
    div()
        .id(SharedString::from(format!("goal-card-{}", summary.goal.id)))
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .p(px(SPACE[2]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(head(summary, owner, t))
        .children(rows.map(|task| crate::work::task_row(app, task, t, cx)))
}

fn head(summary: &GoalSummary, owner: Option<String>, t: &Tokens) -> Div {
    let tone = goal_color(summary.status, t);
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(tone))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(summary.goal.objective.clone()),
        )
        .when_some(crate::app::owner_label(owner, t), |this, owner| {
            this.child(owner)
        })
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(tone)
                .child(goal_status_label(summary.status)),
        )
        .children(
            counts(&summary.tasks)
                .into_iter()
                .map(|(state, count)| crate::work::pill(state, count, t)),
        )
}
