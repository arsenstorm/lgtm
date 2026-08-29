//! The left pane: new-task form, task list, workers.

use crate::app::{prompt_preview, status_label, LgtmApp};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::{ActiveTheme as _, Sizable as _};
use lgtm_protocol::{Batch, BatchSource, Task};
use std::collections::HashSet;

pub fn render_sidebar(app: &mut LgtmApp, cx: &mut Context<LgtmApp>) -> Div {
    let selected = app.selected.clone();
    div()
        .w(px(320.))
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().sidebar)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(div().font_weight(FontWeight::BOLD).child("New task"))
                .child(Input::new(&app.prompt))
                .child(Input::new(&app.repository))
                .child(Input::new(&app.base_branch))
                .child(Input::new(&app.worker))
                .child(
                    Button::new("submit")
                        .label("Submit")
                        .primary()
                        .small()
                        .on_click(cx.listener(LgtmApp::submit)),
                ),
        )
        .child(
            div()
                .id("tasks")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.task_scroll)
                .children(task_rows(app, selected.as_deref(), cx)),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .p_2()
                .gap_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_sm()
                .children(app.workers.iter().map(|worker| {
                    div().flex().gap_2().child(worker.info.name.clone()).child(
                        div().text_color(cx.theme().muted_foreground).child(format!(
                            "{}/{}",
                            worker.running.len(),
                            worker.info.slots
                        )),
                    )
                })),
        )
}

/// Task rows in existing (newest-first) order, with one header row inserted
/// before the first task of each batch. Tasks without a batch get no header.
fn task_rows(app: &LgtmApp, selected: Option<&str>, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut seen = HashSet::new();
    let mut rows = Vec::with_capacity(app.tasks.len());
    for task in &app.tasks {
        if let Some(batch_id) = &task.spec.batch {
            if seen.insert(batch_id.clone()) {
                if let Some(batch) = app.batches.iter().find(|b| &b.id == batch_id) {
                    let count = app
                        .tasks
                        .iter()
                        .filter(|t| t.spec.batch.as_deref() == Some(batch_id.as_str()))
                        .count();
                    rows.push(batch_header(batch, count, cx).into_any_element());
                }
            }
        }
        let id = task.id.clone();
        let active = selected == Some(id.as_str());
        rows.push(
            task_row(task, &app.tasks, active, cx)
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)),
                )
                .into_any_element(),
        );
    }
    rows
}

fn batch_header(batch: &Batch, count: usize, cx: &App) -> Div {
    div()
        .px_2()
        .py_1()
        .text_sm()
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().muted_foreground)
        .child(format!("▣ {} · {count} tasks", batch_label(&batch.source)))
}

/// `o/r label:L` for a GitHub label batch, `TEAM/STATE` for a Linear batch.
pub fn batch_label(source: &BatchSource) -> String {
    match source {
        BatchSource::GithubLabel { owner, repo, label } => {
            format!("{owner}/{repo} label:{label}")
        }
        BatchSource::Linear { team, state } => format!("{team}/{state}"),
    }
}

fn task_row(task: &Task, tasks: &[Task], active: bool, cx: &App) -> Stateful<Div> {
    let mut status = status_label(task, tasks).to_string();
    let failing = task.result.as_ref().is_some_and(|r| {
        r.validation_failed()
            || r.review
                .as_ref()
                .is_some_and(|review| review.has_blocking())
    });
    if failing {
        status.push('!');
    }
    if let Some(pr) = &task.pull_request {
        status.push_str(&format!(" #{}", pr.number));
    }
    let is_child = task.spec.parent.is_some();
    let prompt = if is_child {
        format!("↳ {}", prompt_preview(&task.spec.prompt))
    } else {
        prompt_preview(&task.spec.prompt)
    };
    div()
        .id(SharedString::from(format!("task-{}", task.id)))
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .text_sm()
        .when(active, |this| this.bg(cx.theme().list_active))
        .hover(|this| this.bg(cx.theme().list_hover))
        .child(
            div()
                .flex()
                .gap_2()
                .child(task.id.clone())
                .child(div().text_color(cx.theme().muted_foreground).child(status)),
        )
        .child(
            div()
                .text_color(cx.theme().muted_foreground)
                .when(is_child, |this| this.pl_2())
                .child(prompt),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_label_formats_github_label() {
        let source = BatchSource::GithubLabel {
            owner: "o".into(),
            repo: "r".into(),
            label: "L".into(),
        };
        assert_eq!(batch_label(&source), "o/r label:L");
    }

    #[test]
    fn batch_label_formats_linear() {
        let source = BatchSource::Linear {
            team: "TEAM".into(),
            state: "STATE".into(),
        };
        assert_eq!(batch_label(&source), "TEAM/STATE");
    }
}
