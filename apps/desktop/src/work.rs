//! The Work page: everything in flight, grouped by what it is waiting on,
//! and the batches it was imported from.

use crate::app::{LgtmApp, Overlay};
use crate::labels::{prompt_preview, status_label};
use crate::tasks::{batch_label, needs_attention, now_ms, relative_age, status_color};
use crate::theme::{
    icon, section, tokens, Header, TabularNums as _, Tokens, HEADER_H, ICON, RADIUS, RADIUS_PILL,
    ROW_H, SPACE, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _,
};
use gpui_component::button::Button;
use gpui_component::Sizable as _;
use lgtm_protocol::{Batch, Task};

/// The states a batch card reports, in the order they are shown.
const STATES: [&str; 7] = [
    "queued", "blocked", "running", "review", "approved", "merged", "failed",
];

/// Terminal tasks kept on the page: enough to recognise this morning's work,
/// not a history tab.
const COMPLETED: usize = 20;

/// Non-zero task counts for one batch, in `STATES` order.
pub fn counts(batch: &str, tasks: &[Task]) -> Vec<(&'static str, usize)> {
    let mut totals = [0usize; STATES.len()];
    for task in tasks
        .iter()
        .filter(|task| task.spec.batch.as_deref() == Some(batch))
    {
        let state = match status_label(task, tasks) {
            "awaiting review" | "conflicted" => "review",
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
    let empty = app.tasks.is_empty() && app.batches.is_empty();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            Header::new("Work")
                .action(import_button("import-top", cx))
                .render()
                // The header grows to fill a row; in this column it must not.
                .flex_none()
                .h(px(HEADER_H))
                .px(px(SPACE[2]))
                .border_b_1()
                .border_color(t.border),
        )
        .when(empty, |this| this.child(empty_state(&t, cx)))
        .when(!empty, |this| {
            this.child(
                div()
                    .id("work")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(SPACE[2]))
                    .p(px(SPACE[2]))
                    .children(sections(app, &t, cx)),
            )
        })
        .into_any_element()
}

/// Every group with something in it, in the order a person works down them.
/// `app.tasks` arrives newest first, so each group inherits that order.
fn sections(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let label = |task: &&Task| status_label(task, &app.tasks);
    let groups = [
        (
            "Needs attention",
            app.tasks
                .iter()
                .filter(|task| needs_attention(task, &app.tasks))
                .collect::<Vec<&Task>>(),
        ),
        (
            "Running",
            app.tasks
                .iter()
                .filter(|task| matches!(label(task), "running" | "changes requested"))
                .collect(),
        ),
        (
            "Queued",
            app.tasks
                .iter()
                .filter(|task| matches!(label(task), "queued" | "blocked"))
                .collect(),
        ),
    ];
    let completed: Vec<&Task> = app
        .tasks
        .iter()
        .filter(|task| task.status.is_terminal())
        .take(COMPLETED)
        .collect();

    let mut out: Vec<AnyElement> = Vec::new();
    for (name, rows) in groups {
        if rows.is_empty() {
            continue;
        }
        out.push(
            shell(name, t)
                .children(rows.into_iter().map(|task| task_row(app, task, t, cx)))
                .into_any_element(),
        );
    }
    if !app.batches.is_empty() {
        out.push(
            shell("Batches", t)
                .children(app.batches.iter().map(|batch| card(app, batch, t, cx)))
                .into_any_element(),
        );
    }
    if !completed.is_empty() {
        out.push(
            shell("Recently completed", t)
                .children(completed.into_iter().map(|task| task_row(app, task, t, cx)))
                .into_any_element(),
        );
    }
    out
}

/// A section shell that scopes the element ids under it. A failed task is
/// both stalled and finished, and its two rows must not share one id.
pub(crate) fn shell(name: &str, t: &Tokens) -> Stateful<Div> {
    section(name, t).id(SharedString::from(name.to_string()))
}

fn empty_state(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(SPACE[2]))
        .child("No work yet")
        .child(
            div()
                .text_color(t.muted_fg)
                .child("Start a task with ⌘N, or import issues from GitHub or Linear."),
        )
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
        .tabular_nums()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .child(format!("{} ago", relative_age(created_at, now_ms())))
}

pub(crate) fn task_row(
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
        .child(
            div()
                .tabular_nums()
                .child(relative_age(task.created_at, now_ms())),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
}

pub(crate) fn pill(state: &'static str, count: usize, t: &Tokens) -> Div {
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
                runner: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: vec![],
                depends_on_condition: Default::default(),
                batch: batch.map(String::from),
                sandbox: None,
                requirements: vec![],
                goal: None,
                review_executor: None,
                model: None,
                allowed_hosts: Vec::new(),
                session: None,
                created_by: None,
            },
            status,
            runner: None,
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
            pr_review: None,
            executions: Vec::new(),
            scratchpad: String::new(),
            files: Vec::new(),
            workspace: None,
            created_by: None,
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
