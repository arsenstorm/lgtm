//! The Inbox page: what is waiting on a person, and what everyone has been
//! doing.

use crate::app::LgtmApp;
use crate::labels::{prompt_preview, status_label};
use crate::tasks::{needs_attention, now_ms, relative_age, repo_slug};
use crate::theme::{tokens, Header, TabularNums as _, Tokens, HEADER_H, SPACE, TEXT_SECONDARY};
use crate::work::{shell, task_row};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use lgtm_client::ActivityLine;
use lgtm_protocol::Task;
use std::collections::HashSet;

/// How much of a detail fits on one row.
const DETAIL: usize = 96;

pub fn page(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let waiting: Vec<&Task> = app
        .tasks
        .iter()
        .filter(|task| needs_attention(task, &app.tasks))
        .collect();
    let quiet = waiting.is_empty() && app.activity.is_empty();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            Header::new("Inbox")
                .render()
                // The header grows to fill a row; in this column it must not.
                .flex_none()
                .h(px(HEADER_H))
                .px(px(SPACE[2]))
                .border_b_1()
                .border_color(t.border),
        )
        .when(quiet, |this| this.child(empty_state(&t)))
        .when(!quiet, |this| {
            this.child(
                div()
                    .id("inbox")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(SPACE[2]))
                    .p(px(SPACE[2]))
                    .children(sections(app, waiting, &t, cx)),
            )
        })
        .into_any_element()
}

fn sections(
    app: &LgtmApp,
    waiting: Vec<&Task>,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    let now = now_ms();
    let running: Vec<&Task> = app
        .tasks
        .iter()
        .filter(|task| status_label(task, &app.tasks) == "running")
        .collect();
    let mut out = vec![match waiting.is_empty() {
        true => shell("Needs attention", t)
            .child(muted("Nothing needs you right now.", t))
            .into_any_element(),
        false => shell("Needs attention", t)
            .children(waiting.into_iter().map(|task| task_row(app, task, t, cx)))
            .into_any_element(),
    }];
    if !running.is_empty() {
        let projects: HashSet<String> = running
            .iter()
            .map(|task| repo_slug(&task.spec.repository))
            .collect();
        out.push(
            shell("Running", t)
                .child(muted(&running_line(running.len(), projects.len()), t))
                .into_any_element(),
        );
    }
    if !app.activity.is_empty() {
        out.push(
            shell("Recent activity", t)
                .child(
                    // Adjacent rows, no gaps: the hover highlight tracks a
                    // fast cursor without appearing to lag between rows.
                    div()
                        .flex()
                        .flex_col()
                        .children(app.activity.iter().map(|line| row(line, now, t, cx))),
                )
                .into_any_element(),
        );
    }
    out
}

/// How much is in flight, and over how many projects.
fn running_line(tasks: usize, projects: usize) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    format!(
        "{tasks} task{} running across {projects} project{}",
        plural(tasks),
        plural(projects)
    )
}

fn muted(text: &str, t: &Tokens) -> Div {
    div().text_color(t.muted_fg).child(text.to_string())
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
        .h(px(32.))
        .px(px(SPACE[1]))
        .rounded(px(8.))
        .flex_shrink_0()
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
        .tabular_nums()
        .truncate()
        .text_size(px(TEXT_SECONDARY))
        .text_color(tone)
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_line_pluralises_each_count_on_its_own() {
        assert_eq!(running_line(1, 1), "1 task running across 1 project");
        assert_eq!(running_line(3, 1), "3 tasks running across 1 project");
        assert_eq!(running_line(2, 2), "2 tasks running across 2 projects");
    }
}
