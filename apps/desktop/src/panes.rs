//! The task view: header with status and actions, then the four tabs.

use crate::app::{header_preview, status_label, LgtmApp, Pane};
use crate::net::Action;
use crate::render::Kind;
use crate::sidebar::repo_slug;
use crate::theme::{
    field, icon, tokens, Tokens, HEADER_H, LINE_MONO, MONO_FONT, RADIUS_PILL, SPACE, TEXT_MONO,
    TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, App, ClickEvent, Context, Div, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_component::tab::{Tab, TabBar};
use gpui_component::Sizable as _;
use lgtm_protocol::{CiState, CiStatus, Severity, Task, TaskStatus};

/// `h-5`, the reference badge height.
const BADGE_H: f32 = 20.;

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
                    .px(px(SPACE[1]))
                    .pt(px(SPACE[1]))
                    .child(field(&app.follow_up, &t)),
            )
        })
        .when_some(app.error.clone(), |this, error| {
            this.child(
                div()
                    .px(px(SPACE[2]))
                    .pt(px(SPACE[1]))
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.danger)
                    .child(error),
            )
        })
        .child(
            div().p(px(SPACE[1])).child(
                TabBar::new("panes")
                    .segmented()
                    .text_size(px(TEXT_SECONDARY))
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
                .px(px(SPACE[2]))
                .pb(px(SPACE[2]))
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .line_height(px(LINE_MONO))
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
    let mut meta = format!(
        "{} · {} · {worker}",
        repo_slug(&task.spec.repository),
        task.spec.base_branch
    );
    if cost > 0.0 {
        meta.push_str(&format!(" · ${cost:.2}"));
    }

    div()
        .h(px(HEADER_H))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .px(px(SPACE[1]))
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(
                    div()
                        .flex_shrink()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(header_preview(&task.spec.prompt)),
                )
                .child(badge(status, status_tone(status, t), t))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.muted_fg)
                        .child(meta),
                )
                .when_some(task.pull_request.clone(), |this, pr| {
                    let (mark, tone) = ci_mark(task.ci.as_ref(), t);
                    this.child(
                        badge(format!("#{}", pr.number), tone, t)
                            .when_some(mark, |this, name| this.child(icon(name, MARK, tone)))
                            .id("pr-chip")
                            .cursor_pointer()
                            .on_click(
                                cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&pr.url)),
                            ),
                    )
                })
                .when_some(task.spec.linear.clone(), |this, linear| {
                    this.child(
                        badge(linear.identifier.clone(), t.fg, t)
                            .id("linear-chip")
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.open_url(&linear.url)
                            })),
                    )
                }),
        )
        .child(actions(task, t, cx))
}

/// A ghost button that reads as destructive: no fill, danger-coloured label.
fn danger_ghost(t: &Tokens, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(Hsla::transparent_black())
        .border(Hsla::transparent_black())
        .foreground(t.danger)
        .hover(t.muted)
        .active(t.muted)
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
        "running" => t.info,
        "approved" | "merged" => t.success,
        "failed" | "rejected" | "cancelled" => t.danger,
        _ => t.muted_fg,
    }
}

/// The shadcn badge: a `bg-muted` pill, `text-xs`, coloured by what it reports.
fn badge(label: impl Into<SharedString>, tone: Hsla, t: &Tokens) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(BADGE_H))
        .px(px(SPACE[1]))
        .rounded(px(RADIUS_PILL))
        .bg(t.muted)
        .text_size(px(TEXT_SECONDARY))
        .font_weight(FontWeight::MEDIUM)
        .text_color(tone)
        .child(label.into())
}

/// The icon a badge or a check row carries.
const MARK: f32 = 13.;

fn ci_mark(ci: Option<&CiStatus>, t: &Tokens) -> (Option<&'static str>, Hsla) {
    match ci.map(|ci| ci.state) {
        Some(CiState::Success) => (Some("check"), t.success),
        Some(CiState::Failure) => (Some("x"), t.danger),
        Some(CiState::Pending) => (Some("ellipsis"), t.muted_fg),
        None => (None, t.muted_fg),
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
                Kind::Text => t.fg,
                Kind::Tool => t.info,
                Kind::Stderr => t.danger,
                Kind::Message => t.warning,
                Kind::Status => t.muted_fg,
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
            let mark = if check.ok { "check" } else { "x" };
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(SPACE[0]))
                        .text_color(tone)
                        .child(icon(mark, MARK, tone))
                        .child(check.name.clone()),
                )
                .when(!check.ok, |this| {
                    this.children(
                        check
                            .output_tail
                            .lines()
                            .map(|line| {
                                div()
                                    .pl(px(SPACE[2]))
                                    .text_color(t.muted_fg)
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
                    .text_color(t.muted_fg)
                    .font_weight(FontWeight::BOLD)
                    .child("Review"),
            )
            .children(findings.iter().map(|finding| {
                let (mark, tone) = match finding.severity {
                    Severity::Blocking => ("x", t.danger),
                    Severity::Warning => ("circle-dot", t.warning),
                };
                let location = match finding.line {
                    Some(line) => format!("{}:{line}", finding.file),
                    None => finding.file.clone(),
                };
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[1]))
                    .child(icon(mark, MARK, tone))
                    .child(div().text_color(t.muted_fg).child(location))
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
                .child(div().text_color(t.muted_fg).child(step.prompt.clone()))
                .when(!step.depends_on.is_empty(), |this| {
                    this.child(
                        div()
                            .text_color(t.muted_fg)
                            .child(format!("after: {}", step.depends_on.join(", "))),
                    )
                })
        }))
        .into_any_element()
}

fn muted(text: &'static str, t: &Tokens) -> AnyElement {
    div().text_color(t.muted_fg).child(text).into_any_element()
}
