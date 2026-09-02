//! The Overview tab: how this project is doing, then its newest tasks.

use super::tasks_of;
use crate::app::LgtmApp;
use crate::labels::status_label;
use crate::theme::{section, section_label, TabularNums as _, Tokens, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, Context, Div, FontWeight, IntoElement, ParentElement as _, Styled as _,
};
use lgtm_protocol::{Task, TaskStatus};

/// Tasks shown under the stats.
const RECENT: usize = 5;
/// `text-xl`: the number in a metric.
const METRIC_VALUE: f32 = 21.;

/// What the strip counts. `all` is every task the window holds, because a
/// task's label depends on what it is waiting for.
#[derive(Default, PartialEq, Debug)]
pub(super) struct Numbers {
    pub running: usize,
    pub needs_review: usize,
    pub blocked: usize,
    pub completed: usize,
    pub cost_usd: f64,
}

pub(super) fn numbers(tasks: &[&Task], all: &[Task]) -> Numbers {
    let mut out = Numbers::default();
    for task in tasks {
        match status_label(task, all) {
            "running" => out.running += 1,
            "awaiting review" | "conflicted" => out.needs_review += 1,
            "blocked" => out.blocked += 1,
            _ => {}
        }
        if matches!(task.status, TaskStatus::Approved | TaskStatus::Merged) {
            out.completed += 1;
        }
        out.cost_usd += task.result.as_ref().map_or(0.0, |result| result.cost_usd);
    }
    out
}

pub(super) fn rows(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    vec![
        stats(numbers(&tasks_of(app, slug), &app.tasks), t).into_any_element(),
        recent(app, slug, t, cx).into_any_element(),
    ]
}

fn stats(n: Numbers, t: &Tokens) -> Div {
    let metrics = [
        ("Running", n.running.to_string()),
        ("Needs review", n.needs_review.to_string()),
        ("Blocked", n.blocked.to_string()),
        ("Completed", n.completed.to_string()),
        ("Cost", format!("${:.2}", n.cost_usd)),
    ];
    let count = metrics.len();
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label("Stats", t))
        .child(
            div().flex().children(
                metrics
                    .into_iter()
                    .enumerate()
                    .map(|(at, (label, value))| metric(label, value, at, count, t)),
            ),
        )
}

fn metric(label: &'static str, value: String, at: usize, count: usize, t: &Tokens) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .when(at > 0, |this| {
            this.pl(px(SPACE[2])).border_l_1().border_color(t.border)
        })
        .when(at + 1 < count, |this| this.pr(px(SPACE[2])))
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(label),
        )
        .child(
            div()
                .text_size(px(METRIC_VALUE))
                .font_weight(FontWeight::MEDIUM)
                .tabular_nums()
                .truncate()
                .child(value),
        )
}

fn recent(app: &LgtmApp, slug: &str, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let rows = tasks_of(app, slug);
    section("Recent", t).children(
        rows.into_iter()
            .take(RECENT)
            .map(|task| crate::work::task_row(app, task, t, cx)),
    )
}
