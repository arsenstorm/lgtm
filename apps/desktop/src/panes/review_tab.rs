//! The Review tab: the checks, the reviewer's findings, and the decision the
//! task is waiting on.

mod artefacts;

use super::{danger_ghost, review_actions, MARK};
use crate::app::LgtmApp;
use crate::net::Action;
use crate::theme::{
    field, icon, section, Tokens, LINE_MONO, MONO_FONT, SPACE, TEXT_MONO, TEXT_ROW, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use lgtm_protocol::{
    pending_requests, Finding, Severity, StoredEvent, Task, TaskEvent, TaskStatus, ValidationResult,
};

pub(super) fn review(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    let result = task.result.as_ref();
    let checks = result.map(|r| r.validation.as_slice()).unwrap_or_default();
    let review = result.and_then(|r| r.review.as_ref());
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[4]))
        .text_size(px(TEXT_ROW))
        .children(
            (!checks.is_empty()).then(|| {
                section("Checks", t).children(checks.iter().map(|check| check_row(check, t)))
            }),
        )
        .children(
            review
                .filter(|review| !review.findings.is_empty())
                .map(|review| {
                    section("Findings", t)
                        .children(
                            review
                                .findings
                                .iter()
                                .map(|finding| finding_row(finding, t, cx)),
                        )
                        .when_some(review.executor, |this, executor| {
                            this.child(
                                div()
                                    .text_size(px(TEXT_SECONDARY))
                                    .text_color(t.muted_fg)
                                    .child(format!("reviewed by {}", executor.binary())),
                            )
                        })
                }),
        )
        .children(artefacts::render(app, t, cx))
        .children(requests(app, task, t, cx))
        .children(actions(app, task, t, cx))
        .into_any_element()
}

/// Hosts an agent asked for that a person hasn't granted yet, each with a
/// button to grant it for the task's next run.
fn requests(app: &LgtmApp, task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Option<Div> {
    let pending: Vec<(String, String)> = pending_requests(&app.events, &task.spec)
        .into_iter()
        .filter(|(target, _)| !app.ui.denied.contains(&denial(&task.id, target)))
        .collect();
    if pending.is_empty() {
        return None;
    }
    Some(
        section("Requests", t).children(
            pending
                .into_iter()
                .map(|(target, reason)| request_row(&task.id, target, reason, t, cx)),
        ),
    )
}

/// What a denied request is remembered by, for this window only.
fn denial(task: &str, host: &str) -> String {
    format!("{task}:{host}")
}

fn request_row(
    task: &str,
    target: String,
    reason: String,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Div {
    let host = target.clone();
    let dismiss = denial(task, &target);
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().text_color(t.fg).child(target.clone()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(reason),
        )
        .child(
            Button::new(SharedString::from(format!("allow:{target}")))
                .label("Allow")
                .outline()
                .small()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.act(Action::AllowHost(host.clone()), cx)
                })),
        )
        // Denying is local to this window: the orchestrator has no
        // `POST /api/tasks/:id/deny`, so nothing can be told about it and the
        // request comes back with the next window.
        .child(
            Button::new(SharedString::from(format!("deny:{target}")))
                .label("Deny")
                .custom(danger_ghost(t, cx))
                .small()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.ui.denied.insert(dismiss.clone());
                    cx.notify();
                })),
        )
}

/// The mark carries whether the check passed, so the name itself can stay
/// plain: a green wall of check names is what made the tab shout.
fn check_row(check: &ValidationResult, t: &Tokens) -> Div {
    let tone = if check.ok { t.success } else { t.danger };
    let mark = if check.ok { "check" } else { "x" };
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(icon(mark, MARK, tone))
                .child(check.name.clone()),
        )
        .when(!check.ok, |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .pl(px(SPACE[3]))
                    .font_family(MONO_FONT)
                    .text_size(px(TEXT_MONO))
                    .line_height(px(LINE_MONO))
                    .text_color(t.muted_fg)
                    .children(
                        check
                            .output_tail
                            .lines()
                            .map(|line| div().child(line.to_string())),
                    ),
            )
        })
}

/// A finding opens the file it names in Changes.
fn finding_row(finding: &Finding, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    let (mark, tone) = match finding.severity {
        Severity::Blocking => ("x", t.danger),
        Severity::Warning => ("circle-dot", t.warning),
    };
    let location = match finding.line {
        Some(line) => format!("{}:{line}", finding.file),
        None => finding.file.clone(),
    };
    let file = finding.file.clone();
    div()
        .id(SharedString::from(format!("finding:{location}")))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .cursor_pointer()
        .hover(|this| this.bg(t.muted))
        .child(icon(mark, MARK, tone))
        .child(
            div()
                .flex_none()
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .child(location),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(t.muted_fg)
                .child(finding.message.clone()),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_changes_at(&file, cx)))
}

/// What the task is waiting for a person to do, if anything.
fn actions(app: &LgtmApp, task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Option<Div> {
    let row = div().flex().items_center().gap(px(SPACE[1]));
    match task.status {
        TaskStatus::AwaitingReview => Some(
            section("Decision", t)
                .child(
                    div()
                        .text_color(t.muted_fg)
                        .child("Approve this result to finish the review. A pull request can merge only after approval and passing CI."),
                )
                .child(row.children(review_actions(t, cx))),
        ),
        TaskStatus::Conflicted => Some(conflict(app, t, cx)),
        TaskStatus::Failed
        | TaskStatus::TimedOut
        | TaskStatus::RunnerLost
        | TaskStatus::Cancelled => Some(
            row.child(
                Button::new("review-retry")
                    .label("Retry")
                    .outline()
                    .small()
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Retry, cx)),
                    ),
            ),
        ),
        _ => None,
    }
}

/// A rebase conflict is only resolved by telling the agent what to do, so the
/// composer is open from the start, over the files that clash.
fn conflict(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let files = conflict_files(&app.events);
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .when(!files.is_empty(), |this| {
            this.child(
                div()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.warning)
                    .child(format!("Rebase conflict on: {}", files.join(", "))),
            )
        })
        .child(field(&app.inputs.follow_up, t))
        .child(
            div().child(
                Button::new("conflict-tell")
                    .label("Send")
                    .custom(danger_ghost(t, cx))
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.send_follow_up(window, cx)
                    })),
            ),
        )
}

/// Files the last `Conflicted` event reported.
fn conflict_files(events: &[StoredEvent]) -> Vec<String> {
    events
        .iter()
        .rev()
        .find_map(|stored| match &stored.event {
            TaskEvent::Conflicted { files, .. } => Some(files.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(event: TaskEvent) -> StoredEvent {
        StoredEvent { at: 0, event }
    }

    #[test]
    fn the_hint_reads_the_last_conflict() {
        let events = vec![
            stored(TaskEvent::Conflicted {
                base: "main".into(),
                files: vec!["old.rs".into()],
            }),
            stored(TaskEvent::Conflicted {
                base: "main".into(),
                files: vec!["a.rs".into(), "b.rs".into()],
            }),
        ];
        assert_eq!(conflict_files(&events), vec!["a.rs", "b.rs"]);
        assert!(conflict_files(&[stored(TaskEvent::Started { model: None })]).is_empty());
    }
}
