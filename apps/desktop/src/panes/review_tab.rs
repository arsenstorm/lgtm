//! The Review tab: the checks, the reviewer's findings, and the decision the
//! task is waiting on.

use super::{danger_ghost, muted, review_actions, MARK};
use crate::app::LgtmApp;
use crate::net::Action;
use crate::theme::{field, icon, section_label, Tokens, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use lgtm_protocol::{
    Finding, Severity, StoredEvent, Task, TaskEvent, TaskStatus, ValidationResult,
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
        .gap(px(SPACE[2]))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE[0]))
                .child(section_label("Checks", t))
                .children(checks.iter().map(|check| check_row(check, t)))
                .when(checks.is_empty(), |this| {
                    this.child(muted("No checks configured.", t))
                }),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(SPACE[0]))
                .child(section_label("Findings", t))
                .children(
                    review
                        .map(|review| review.findings.as_slice())
                        .unwrap_or_default()
                        .iter()
                        .map(|finding| finding_row(finding, t, cx)),
                )
                .when(review.is_none_or(|r| r.findings.is_empty()), |this| {
                    this.child(muted("No findings.", t))
                })
                .when_some(review.and_then(|r| r.executor), |this, executor| {
                    this.child(
                        div()
                            .text_color(t.muted_fg)
                            .child(format!("reviewed by {}", executor.binary())),
                    )
                }),
        )
        .children(actions(app, task, t, cx))
        .into_any_element()
}

fn check_row(check: &ValidationResult, t: &Tokens) -> Div {
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
            this.children(check.output_tail.lines().map(|line| {
                div()
                    .pl(px(SPACE[2]))
                    .text_color(t.muted_fg)
                    .child(line.to_string())
            }))
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
        .child(div().text_color(t.info).child(location))
        .child(div().child(finding.message.clone()))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_changes_at(&file, cx)))
}

/// What the task is waiting for a person to do, if anything.
fn actions(app: &LgtmApp, task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Option<Div> {
    let row = div().flex().items_center().gap(px(SPACE[1]));
    match task.status {
        TaskStatus::AwaitingReview => Some(row.children(review_actions(t, cx))),
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
