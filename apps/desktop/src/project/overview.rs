//! The Overview tab: the orchestrator's stats, then the newest tasks.

use super::tasks_of;
use crate::app::LgtmApp;
use crate::tasks::duration;
use crate::theme::{section, section_label, Tokens, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, Context, Div, FontWeight, IntoElement, ParentElement as _, Styled as _,
};
use lgtm_protocol::Stats;

/// Tasks shown under the stats.
const RECENT: usize = 5;
/// `text-xl`: the number in a metric.
const METRIC_VALUE: f32 = 20.;

pub(super) fn rows(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    vec![
        stats(app.stats.clone().unwrap_or_default(), t).into_any_element(),
        recent(app, slug, t, cx).into_any_element(),
    ]
}

/// Stats come from `/api/stats`, which counts every task the orchestrator has
/// — there is no per-repository window — so the label says whose they are.
fn stats(stats: Stats, t: &Tokens) -> Div {
    let metrics = [
        ("Tasks", stats.tasks.to_string()),
        ("Median run", duration(stats.median_execution_ms)),
        ("Median queue", duration(stats.median_queue_ms)),
        ("Retries", stats.retried_tasks.to_string()),
        ("Cost", format!("${:.2}", stats.cost_usd)),
    ];
    let count = metrics.len();
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label("All-project stats", t))
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
                .truncate()
                .child(value),
        )
}

fn recent(app: &LgtmApp, slug: &str, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let rows = tasks_of(app, slug);
    section("Recent", t).children(
        rows.into_iter()
            .take(RECENT)
            .map(|task| crate::batches::task_row(app, task, t, cx)),
    )
}
