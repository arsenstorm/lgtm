//! The right-hand pane: task header, actions, and the three tabs.

use crate::app::{prompt_preview, status_label, LgtmApp, Pane};
use crate::diff;
use crate::net::Action;
use crate::render::Kind;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, rgb, AnyElement, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::{ActiveTheme as _, Sizable as _};
use lgtm_protocol::{CiState, CiStatus, Task, TaskStatus};

const ADD: u32 = 0x1a7f37;
const DEL: u32 = 0xcf222e;

pub fn render_main(app: &mut LgtmApp, _window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
    let Some(task) = app.selected_task().cloned() else {
        return div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child("no task selected");
    };

    let pane_index = match app.pane {
        Pane::Activity => 0,
        Pane::Diff => 1,
        Pane::Checks => 2,
    };

    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(header(app, &task, cx))
        .child(
            TabBar::new("panes")
                .selected_index(pane_index)
                .children([
                    Tab::new().label("Activity"),
                    Tab::new().label("Diff"),
                    Tab::new().label("Checks"),
                ])
                .on_click(cx.listener(|this, index: &usize, _, cx| {
                    this.pane = match index {
                        0 => Pane::Activity,
                        1 => Pane::Diff,
                        _ => Pane::Checks,
                    };
                    cx.notify();
                })),
        )
        .child(
            div()
                .id("pane-content")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.content_scroll)
                .p_2()
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(cx.theme().mono_font_size)
                .child(match app.pane {
                    Pane::Activity => activity(app, cx),
                    Pane::Diff => diff_pane(&task, cx),
                    Pane::Checks => checks(&task, cx),
                }),
        )
}

fn header(app: &mut LgtmApp, task: &Task, cx: &mut Context<LgtmApp>) -> Div {
    let worker = task.worker.clone().unwrap_or_else(|| "unassigned".into());
    let title = format!("{} · {} · {worker}", task.id, status_label(task.status));
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .flex()
                .items_center()
                .child(div().font_weight(FontWeight::BOLD).child(title))
                .when_some(task.pull_request.clone(), |this, pr| {
                    this.child(
                        div()
                            .id("pr-link")
                            .cursor_pointer()
                            .font_weight(FontWeight::BOLD)
                            .child(format!(" · PR #{}", pr.number))
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.open_url(&pr.url);
                            })),
                    )
                })
                .when_some(task.ci.clone(), |this, ci| {
                    let (mark, color) = match ci.state {
                        CiState::Success => ("✓", cx.theme().success),
                        CiState::Failure => ("✗", cx.theme().danger),
                        CiState::Pending => ("…", cx.theme().muted_foreground),
                    };
                    this.child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(color)
                            .child(format!(" ci {mark}")),
                    )
                }),
        )
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(prompt_preview(&task.spec.prompt)),
        )
        .child(actions(app, task, cx))
        .when_some(app.error.clone(), |this, error| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(format!("error: {error}")),
            )
        })
}

fn actions(app: &mut LgtmApp, task: &Task, cx: &mut Context<LgtmApp>) -> Div {
    let row = div().flex().gap_2().items_center();
    match task.status {
        TaskStatus::AwaitingReview => row
            .child(
                Button::new("approve")
                    .label("Approve")
                    .primary()
                    .small()
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Approve, cx)),
                    ),
            )
            .child(
                Button::new("reject")
                    .label("Reject")
                    .danger()
                    .small()
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Reject, cx)),
                    ),
            )
            .child(div().flex_1().child(Input::new(&app.follow_up).small()))
            .child(
                Button::new("send")
                    .label("Send")
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        let text = this.follow_up.read(cx).value().to_string();
                        if text.trim().is_empty() {
                            return;
                        }
                        this.follow_up
                            .update(cx, |state, cx| state.set_value("", window, cx));
                        this.act(Action::Tell(text), cx);
                    })),
            ),
        TaskStatus::Queued | TaskStatus::Running => row.child(
            Button::new("cancel")
                .label("Cancel")
                .danger()
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Cancel, cx))),
        ),
        TaskStatus::Approved
            if matches!(
                task.ci,
                Some(CiStatus {
                    state: CiState::Success,
                    ..
                })
            ) =>
        {
            row.child(
                Button::new("merge")
                    .label("Merge")
                    .primary()
                    .small()
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Merge, cx)),
                    ),
            )
        }
        _ => row,
    }
}

fn activity(app: &LgtmApp, cx: &Context<LgtmApp>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .children(app.lines.iter().map(|line| {
            let color = match line.kind {
                Kind::Text => cx.theme().foreground,
                Kind::Tool => cx.theme().primary,
                Kind::Stderr => cx.theme().danger,
                Kind::Message => cx.theme().info,
                Kind::Status => cx.theme().muted_foreground,
            };
            div().text_color(color).child(line.text.clone())
        }))
        .into_any_element()
}

fn diff_pane(task: &Task, cx: &Context<LgtmApp>) -> AnyElement {
    let files = task
        .result
        .as_ref()
        .map(|result| diff::parse(&result.diff))
        .unwrap_or_default();
    if files.is_empty() {
        return muted("no diff yet", cx);
    }
    div()
        .flex()
        .flex_col()
        .gap_2()
        .children(files.into_iter().map(|file| {
            div()
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::BOLD).child(file.path.clone()))
                .children(file.lines.into_iter().map(|line| {
                    let color: Option<Hsla> = match line.kind {
                        diff::Kind::Add => Some(rgb(ADD).into()),
                        diff::Kind::Del => Some(rgb(DEL).into()),
                        diff::Kind::Hunk => Some(cx.theme().muted_foreground),
                        diff::Kind::Context => None,
                    };
                    div()
                        .when_some(color, |this, color| this.text_color(color))
                        .child(line.text)
                }))
        }))
        .into_any_element()
}

fn checks(task: &Task, cx: &Context<LgtmApp>) -> AnyElement {
    let checks = task
        .result
        .as_ref()
        .map(|result| result.validation.clone())
        .unwrap_or_default();
    if checks.is_empty() {
        return muted("no checks configured", cx);
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(checks.into_iter().map(|check| {
            let color = if check.ok {
                cx.theme().success
            } else {
                cx.theme().danger
            };
            let mark = if check.ok { "✓" } else { "✗" };
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_color(color)
                        .child(format!("{mark} {}", check.name)),
                )
                .when(!check.ok, |this| {
                    this.children(
                        check
                            .output_tail
                            .lines()
                            .map(|line| {
                                div()
                                    .pl_4()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(line.to_string())
                            })
                            .collect::<Vec<_>>(),
                    )
                })
        }))
        .into_any_element()
}

fn muted(text: &'static str, cx: &Context<LgtmApp>) -> AnyElement {
    div()
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}
