//! The Plans tab: every plan version the project's goals produced, newest
//! first, each one unfolding into its steps.

use super::muted;
use crate::app::LgtmApp;
use crate::tasks::{now_ms, relative_age};
use crate::theme::{icon, Tokens, ICON, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use lgtm_protocol::{PlanStatus, PlanVersion};

pub(super) fn rows(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    if app.plans.is_empty() {
        return vec![muted("No plans in this project yet.", t)];
    }
    app.plans
        .iter()
        .map(|plan| card(app, plan, t, cx).into_any_element())
        .collect()
}

fn key(plan: &PlanVersion) -> String {
    format!("plan:{}:{}", plan.task, plan.version)
}

fn card(app: &LgtmApp, plan: &PlanVersion, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let key = key(plan);
    let open = app.ui.expanded.contains(&key);
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .p(px(SPACE[2]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(head(plan, open, key, t, cx))
        .when(open, |this| {
            this.children(
                plan.plan
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(at, step)| self::step(at, step, t)),
            )
        })
}

fn head(
    plan: &PlanVersion,
    open: bool,
    key: String,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> impl IntoElement {
    let steps = plan.plan.steps.len();
    div()
        .id(SharedString::from(key.clone()))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .cursor_pointer()
        .child(icon(
            if open {
                "chevron-down"
            } else {
                "chevron-right"
            },
            ICON,
            t.muted_fg,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(format!("v{} · {steps} steps", plan.version)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(tone(plan.status, t))
                .child(status_label(plan.status)),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(relative_age(plan.created_at, now_ms())),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.ui.expanded.remove(&key) {
                this.ui.expanded.insert(key.clone());
            }
            cx.notify();
        }))
}

fn step(at: usize, step: &lgtm_protocol::PlanStep, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .pl(px(SPACE[3]))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(format!("{}. {}", at + 1, step.title)),
        )
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(step.prompt.clone()),
        )
        .when(!step.depends_on.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.muted_fg)
                    .child(format!("after: {}", step.depends_on.join(", "))),
            )
        })
}

fn status_label(status: PlanStatus) -> &'static str {
    match status {
        PlanStatus::AwaitingApproval => "awaiting approval",
        PlanStatus::Approved => "approved",
        PlanStatus::Rejected => "rejected",
        PlanStatus::Replanning => "replanning",
        PlanStatus::Superseded => "superseded",
    }
}

fn tone(status: PlanStatus, t: &Tokens) -> gpui::Hsla {
    match status {
        PlanStatus::AwaitingApproval => t.warning,
        PlanStatus::Approved => t.success,
        PlanStatus::Rejected => t.danger,
        PlanStatus::Replanning => t.info,
        PlanStatus::Superseded => t.muted_fg,
    }
}
