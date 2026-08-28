//! The left pane: new-task form, task list, workers.

use crate::app::{prompt_preview, status_label, LgtmApp};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::{ActiveTheme as _, Sizable as _};
use lgtm_protocol::Task;

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
                .children(app.tasks.iter().map(|task| {
                    let id = task.id.clone();
                    let active = selected.as_deref() == Some(id.as_str());
                    task_row(task, &app.tasks, active, cx).on_click(
                        cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)),
                    )
                })),
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

fn task_row(task: &Task, tasks: &[Task], active: bool, cx: &App) -> Stateful<Div> {
    let mut status = status_label(task, tasks).to_string();
    if task.result.as_ref().is_some_and(|r| r.validation_failed()) {
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
