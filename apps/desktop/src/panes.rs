//! The task view: header with status and actions, then the four tabs.

use crate::app::{header_preview, status_label, LgtmApp, Pane};
use crate::net::Action;
use crate::render::Kind;
use crate::sidebar::repo_slug;
use crate::theme::{tokens, Tokens, MONO_FONT, SPACE, TEXT_MONO, TEXT_SECONDARY, TEXT_TITLE};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::tab::{Tab, TabBar};
use gpui_component::Sizable as _;
use lgtm_protocol::{CiState, CiStatus, Severity, Task, TaskStatus};

const HEADER_HEIGHT: f32 = 64.;

pub fn task_view(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let Some(task) = app.selected_task().cloned() else {
        return div().flex_1().into_any_element();
    };

    let has_plan = task.result.as_ref().is_some_and(|r| r.plan.is_some());
    // A task selected while on the Plan tab may not have a plan; fall back to
    // Activity for that render without losing the user's tab choice.
    let pane = if app.pane == Pane::Plan && !has_plan {
        Pane::Activity
    } else {
        app.pane
    };
    let mut tabs = vec![
        Tab::new().label("Activity"),
        Tab::new().label("Changes"),
        Tab::new().label("Checks"),
    ];
    if has_plan {
        tabs.push(Tab::new().label("Plan"));
    }

    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(header(app, &task, &t, cx))
        .when(app.show_follow_up, |this| {
            this.child(
                div()
                    .px(px(SPACE[3]))
                    .pb(px(SPACE[1]))
                    .child(Input::new(&app.follow_up)),
            )
        })
        .when_some(app.error.clone(), |this, error| {
            this.child(
                div()
                    .px(px(SPACE[3]))
                    .pb(px(SPACE[1]))
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.danger)
                    .child(error),
            )
        })
        .child(
            div().px(px(SPACE[2])).child(
                TabBar::new("panes")
                    .selected_index(match pane {
                        Pane::Activity => 0,
                        Pane::Changes => 1,
                        Pane::Checks => 2,
                        Pane::Plan => 3,
                    })
                    .children(tabs)
                    .on_click(cx.listener(|this, index: &usize, _, cx| {
                        this.pane = match index {
                            0 => Pane::Activity,
                            1 => Pane::Changes,
                            2 => Pane::Checks,
                            _ => Pane::Plan,
                        };
                        cx.notify();
                    })),
            ),
        )
        .child(match pane {
            Pane::Changes => div()
                .flex_1()
                .min_h_0()
                .child(crate::changes::changes_pane(app, window, cx))
                .into_any_element(),
            _ => div()
                .id("pane-content")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.content_scroll)
                .p(px(SPACE[3]))
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .child(match pane {
                    Pane::Checks => checks(&task, &t),
                    Pane::Plan => plan_pane(&task, &t),
                    _ => activity(app, &t),
                })
                .into_any_element(),
        })
        .into_any_element()
}

fn header(app: &mut LgtmApp, task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let status = status_label(task, &app.tasks);
    let worker = task.worker.clone().unwrap_or_else(|| "unassigned".into());
    let cost = task.result.as_ref().map(|r| r.cost_usd).unwrap_or(0.0);

    div()
        .h(px(HEADER_HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(SPACE[2]))
        .px(px(SPACE[3]))
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .child(header_preview(&task.spec.prompt)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(SPACE[1]))
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.text_muted)
                        .child(pill(status, status_tone(status, t), t))
                        .child(format!(
                            "{} · {} · {worker}",
                            repo_slug(&task.spec.repository),
                            task.spec.base_branch
                        ))
                        .when(cost > 0.0, |this| this.child(format!("${cost:.2}")))
                        .when_some(task.pull_request.clone(), |this, pr| {
                            let (mark, tone) = ci_mark(task.ci.as_ref(), t);
                            this.child(
                                div()
                                    .id("pr-chip")
                                    .cursor_pointer()
                                    .px(px(SPACE[0]))
                                    .rounded(px(4.))
                                    .bg(t.surface)
                                    .text_color(tone)
                                    .child(format!("#{} {mark}", pr.number))
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        cx.open_url(&pr.url)
                                    })),
                            )
                        })
                        .when_some(task.spec.linear.clone(), |this, linear| {
                            this.child(
                                div()
                                    .id("linear-chip")
                                    .cursor_pointer()
                                    .px(px(SPACE[0]))
                                    .rounded(px(4.))
                                    .bg(t.surface)
                                    .text_color(t.text)
                                    .child(linear.identifier.clone())
                                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                        cx.open_url(&linear.url)
                                    })),
                            )
                        }),
                ),
        )
        .child(actions(task, t, cx))
}

/// A ghost button that reads as destructive: no fill, danger-coloured label.
fn danger_ghost(t: &Tokens, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(Hsla::transparent_black())
        .border(Hsla::transparent_black())
        .foreground(t.danger)
        .hover(t.surface)
        .active(t.surface)
        .shadow(false)
}

fn actions(task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let row = div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(SPACE[1]));
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
                Button::new("request-changes")
                    .label("Request changes")
                    .outline()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        if this.review.comment_count() > 0 {
                            crate::changes::request_changes(this, cx);
                        } else {
                            this.open_follow_up(window, cx);
                        }
                    })),
            )
            .child(
                Button::new("reject")
                    .label("Reject")
                    .custom(danger_ghost(t, cx))
                    .small()
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Reject, cx)),
                    ),
            ),
        TaskStatus::Queued | TaskStatus::Running => row.child(
            Button::new("cancel")
                .label("Cancel")
                .custom(danger_ghost(t, cx))
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

fn status_tone(status: &str, t: &Tokens) -> Hsla {
    match status {
        "awaiting_review" => t.warning,
        "running" => t.accent,
        "approved" | "merged" => t.success,
        "failed" | "rejected" | "cancelled" => t.danger,
        _ => t.text_muted,
    }
}

fn pill(label: impl Into<SharedString>, tone: Hsla, t: &Tokens) -> Div {
    div()
        .px(px(SPACE[0]))
        .rounded(px(4.))
        .bg(t.surface)
        .text_color(tone)
        .child(label.into())
}

fn ci_mark(ci: Option<&CiStatus>, t: &Tokens) -> (&'static str, Hsla) {
    match ci.map(|ci| ci.state) {
        Some(CiState::Success) => ("✓", t.success),
        Some(CiState::Failure) => ("✗", t.danger),
        Some(CiState::Pending) => ("…", t.text_muted),
        None => ("", t.text_muted),
    }
}

fn activity(app: &LgtmApp, t: &Tokens) -> AnyElement {
    if app.lines.is_empty() {
        return muted("Nothing yet.", t);
    }
    div()
        .flex()
        .flex_col()
        .children(app.lines.iter().map(|line| {
            let color = match line.kind {
                Kind::Text => t.text,
                Kind::Tool => t.accent,
                Kind::Stderr => t.danger,
                Kind::Message => t.warning,
                Kind::Status => t.text_muted,
            };
            div().text_color(color).child(line.text.clone())
        }))
        .into_any_element()
}

fn checks(task: &Task, t: &Tokens) -> AnyElement {
    let result = task.result.as_ref();
    let checks = result.map(|r| r.validation.clone()).unwrap_or_default();
    let findings = result
        .and_then(|r| r.review.as_ref())
        .map(|r| r.findings.as_slice())
        .unwrap_or_default();

    if checks.is_empty() && findings.is_empty() {
        return muted("No checks configured.", t);
    }

    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .children(checks.into_iter().map(|check| {
            let tone = if check.ok { t.success } else { t.danger };
            let mark = if check.ok { "✓" } else { "✗" };
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .text_color(tone)
                        .child(format!("{mark} {}", check.name)),
                )
                .when(!check.ok, |this| {
                    this.children(
                        check
                            .output_tail
                            .lines()
                            .map(|line| {
                                div()
                                    .pl(px(SPACE[3]))
                                    .text_color(t.text_muted)
                                    .child(line.to_string())
                            })
                            .collect::<Vec<_>>(),
                    )
                })
        }))
        .when(!findings.is_empty(), |this| {
            this.child(
                div()
                    .pt(px(SPACE[1]))
                    .text_color(t.text_muted)
                    .font_weight(FontWeight::BOLD)
                    .child("Review"),
            )
            .children(findings.iter().map(|finding| {
                let (mark, tone) = match finding.severity {
                    Severity::Blocking => ("✖", t.danger),
                    Severity::Warning => ("⚠", t.warning),
                };
                let location = match finding.line {
                    Some(line) => format!("{}:{line}", finding.file),
                    None => finding.file.clone(),
                };
                div()
                    .flex()
                    .gap(px(SPACE[1]))
                    .child(div().text_color(tone).child(mark))
                    .child(div().text_color(t.text_muted).child(location))
                    .child(div().child(finding.message.clone()))
            }))
        })
        .into_any_element()
}

fn plan_pane(task: &Task, t: &Tokens) -> AnyElement {
    let Some(plan) = task.result.as_ref().and_then(|r| r.plan.as_ref()) else {
        return muted("No plan.", t);
    };
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[2]))
        .children(plan.steps.iter().enumerate().map(|(i, step)| {
            div()
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::BOLD).child(format!(
                    "{}. {}  {}",
                    i + 1,
                    step.key,
                    step.title
                )))
                .child(div().text_color(t.text_muted).child(step.prompt.clone()))
                .when(!step.depends_on.is_empty(), |this| {
                    this.child(
                        div()
                            .text_color(t.text_muted)
                            .child(format!("after: {}", step.depends_on.join(", "))),
                    )
                })
        }))
        .into_any_element()
}

fn muted(text: &'static str, t: &Tokens) -> AnyElement {
    div()
        .text_color(t.text_muted)
        .child(text)
        .into_any_element()
}
