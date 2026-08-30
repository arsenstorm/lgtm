//! The Overview tab: the orchestrator's stats, then the newest tasks.

use super::tasks_of;
use crate::app::LgtmApp;
use crate::tasks::duration;
use crate::theme::{section_label, Tokens, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::{
    div, px, AnyElement, Context, Div, FontWeight, IntoElement, ParentElement as _, Styled as _,
};
use lgtm_protocol::Stats;

/// Tasks shown under the stats.
const RECENT: usize = 5;
/// `text-xl`: the number on a stat tile.
const TILE_VALUE: f32 = 20.;

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
    let tiles = [
        ("Tasks", stats.tasks.to_string()),
        ("Median run", duration(stats.median_execution_ms)),
        ("Median queue", duration(stats.median_queue_ms)),
        ("Cost", format!("${:.2}", stats.cost_usd)),
    ];
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label("Stats · all projects", t))
        .child(
            div()
                .flex()
                .gap(px(SPACE[1]))
                .children(tiles.map(|(label, value)| tile(label, value, t))),
        )
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(format!("{} retried", stats.retried_tasks)),
        )
}

fn tile(label: &'static str, value: String, t: &Tokens) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(label),
        )
        .child(
            div()
                .text_size(px(TILE_VALUE))
                .font_weight(FontWeight::MEDIUM)
                .truncate()
                .child(value),
        )
}

fn recent(app: &LgtmApp, slug: &str, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let rows = tasks_of(app, slug);
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label("Recent", t))
        .children(
            rows.into_iter()
                .take(RECENT)
                .map(|task| crate::batches::task_row(app, task, t, cx)),
        )
}
