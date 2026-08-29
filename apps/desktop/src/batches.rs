//! The Batches page: what was imported, and how far each import has got.

use crate::app::{LgtmApp, Overlay};
use crate::labels::{prompt_preview, status_label};
use crate::tasks::{batch_label, now_ms, relative_age, status_color};
use crate::theme::{
    icon, tokens, Tokens, HEADER_H, ICON, RADIUS, RADIUS_PILL, ROW_H, SPACE, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::Button;
use gpui_component::Sizable as _;
use lgtm_protocol::{Batch, Task};

/// The states a batch card reports, in the order they are shown.
const STATES: [&str; 7] = [
    "queued", "blocked", "running", "review", "approved", "merged", "failed",
];

/// Non-zero task counts for one batch, in `STATES` order.
pub fn counts(batch: &str, tasks: &[Task]) -> Vec<(&'static str, usize)> {
    let mut totals = [0usize; STATES.len()];
    for task in tasks
        .iter()
        .filter(|task| task.spec.batch.as_deref() == Some(batch))
    {
        let state = match status_label(task, tasks) {
            "awaiting_review" => "review",
            "rejected" | "cancelled" => "failed",
            other => other,
        };
        if let Some(at) = STATES.iter().position(|name| *name == state) {
            totals[at] += 1;
        }
    }
    STATES
        .iter()
        .zip(totals)
        .filter(|(_, count)| *count > 0)
        .map(|(name, count)| (*name, count))
        .collect()
}

pub fn page(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let empty = app.batches.is_empty();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(page_header(&t, cx))
        .when(empty, |this| this.child(empty_state(&t, cx)))
        .when(!empty, |this| {
            this.child(
                div()
                    .id("batches")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(SPACE[2]))
                    .p(px(SPACE[2]))
                    .children(app.batches.iter().map(|batch| card(app, batch, &t, cx))),
            )
        })
        .into_any_element()
}

fn page_header(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
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
                .child("Batches"),
        )
        .child(import_button("import-top", cx))
}

fn empty_state(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(SPACE[2]))
        .child(div().text_color(t.muted_fg).child("No batches yet"))
        .child(import_button("import-empty", cx))
}

fn import_button(id: &'static str, cx: &mut Context<LgtmApp>) -> Button {
    Button::new(id)
        .label("Import")
        .outline()
        .small()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.ui.overlay = Overlay::Import;
            cx.notify();
        }))
}

fn card(app: &LgtmApp, batch: &Batch, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let open = app.ui.expanded.contains(&batch.id);
    let rows: Vec<&Task> = app
        .tasks
        .iter()
        .filter(|task| task.spec.batch.as_deref() == Some(batch.id.as_str()))
        .collect();
    div()
        .flex()
        .flex_col()
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(card_header(app, batch, t, cx))
        .when(open, |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .px(px(SPACE[1]))
                    .pb(px(SPACE[1]))
                    .children(rows.into_iter().map(|task| task_row(app, task, t, cx))),
            )
        })
}

/// The clickable row that names the batch and folds its tasks in and out.
fn card_header(
    app: &LgtmApp,
    batch: &Batch,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = batch.id.clone();
    let open = app.ui.expanded.contains(&id);
    let chevron = if open {
        "chevron-down"
    } else {
        "chevron-right"
    };
    div()
        .id(SharedString::from(format!("batch-{id}")))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .p(px(SPACE[2]))
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .child(icon(chevron, ICON, t.muted_fg))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(batch_label(&batch.source)),
        )
        .children(
            counts(&id, &app.tasks)
                .into_iter()
                .map(|(state, count)| pill(state, count, t)),
        )
        .child(age(batch.created_at, t))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.ui.expanded.remove(&id) {
                this.ui.expanded.insert(id.clone());
            }
            cx.notify();
        }))
}

fn age(created_at: u64, t: &Tokens) -> Div {
    div()
        .flex_shrink_0()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .child(format!("{} ago", relative_age(created_at, now_ms())))
}

fn task_row(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = task.id.clone();
    let dot = status_color(task, &app.tasks, t);
    div()
        .id(SharedString::from(format!("batch-task-{id}")))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .hover(|this| this.bg(t.muted))
        .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(dot))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(prompt_preview(&task.spec.prompt, 64)),
        )
        .child(div().child(relative_age(task.created_at, now_ms())))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
}

fn pill(state: &'static str, count: usize, t: &Tokens) -> Div {
    let tone = match state {
        "review" => t.warning,
        "running" => t.info,
        "approved" | "merged" => t.success,
        "failed" => t.danger,
        _ => t.muted_fg,
    };
    div()
        .flex_shrink_0()
        .px(px(SPACE[1]))
        .rounded(px(RADIUS_PILL))
        .bg(t.muted)
        .text_size(px(TEXT_SECONDARY))
        .text_color(tone)
        .child(format!("{state} {count}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec, TaskStatus};

    fn task(id: &str, batch: Option<&str>, status: TaskStatus) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: "https://x/one.git".into(),
                base_branch: "main".into(),
                prompt: "p".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: vec![],
                batch: batch.map(String::from),
                sandbox: None,
                requirements: vec![],
                goal: None,
            },
            status,
            worker: None,
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            executions: Vec::new(),
        }
    }

    #[test]
    fn counts_only_this_batch_and_only_what_is_there() {
        let tasks = vec![
            task("a", Some("b1"), TaskStatus::Running),
            task("b", Some("b1"), TaskStatus::AwaitingReview),
            task("c", Some("b1"), TaskStatus::Cancelled),
            task("d", Some("b2"), TaskStatus::Merged),
            task("e", None, TaskStatus::Running),
        ];
        assert_eq!(
            counts("b1", &tasks),
            vec![("running", 1), ("review", 1), ("failed", 1)]
        );
        assert_eq!(counts("b2", &tasks), vec![("merged", 1)]);
        assert!(counts("nope", &tasks).is_empty());
    }
}
