//! One task of a thread, drawn the way a harness draws a tool call: a header
//! that says what it is and how it is going, and one line of body.

use crate::app::LgtmApp;
use crate::labels::{prompt_preview, status_label};
use crate::panes::badge;
use crate::tasks::{duration, status_color};
use crate::theme::{Tokens, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, ClickEvent, Context, Div, InteractiveElement as _, ParentElement as _, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _,
};
use lgtm_protocol::{StoredEvent, Task, TaskEvent, TaskResult, TaskStatus};

/// How much of the prompt the card's header carries.
const TITLE: usize = 56;
/// How much of a progress, command, or error line fits on one row.
const LINE: usize = 96;

/// The card's body: why a failed task stopped, the newest step of a running
/// one, or what a finished one produced.
pub fn body_line(task: &Task, events: &[StoredEvent]) -> String {
    if let Some(error) = &task.error {
        return prompt_preview(error, LINE);
    }
    if task.status == TaskStatus::Running {
        return latest_step(events).unwrap_or_else(|| "Working…".to_string());
    }
    match &task.result {
        Some(result) => tally(result),
        None => String::new(),
    }
}

/// The last thing the agent said it was doing.
fn latest_step(events: &[StoredEvent]) -> Option<String> {
    events.iter().rev().find_map(|stored| match &stored.event {
        TaskEvent::Progress { text } => Some(prompt_preview(text, LINE)),
        TaskEvent::Command { command } => Some(prompt_preview(command, LINE)),
        _ => None,
    })
}

/// `3 files · 2/2 checks · 1 finding`, minus what the task has none of.
fn tally(result: &TaskResult) -> String {
    let passed = result.validation.iter().filter(|check| check.ok).count();
    let findings = result.review.as_ref().map_or(0, |r| r.findings.len());
    let mut parts = vec![plural(result.changed_files.len(), "file")];
    if !result.validation.is_empty() {
        parts.push(format!("{passed}/{} checks", result.validation.len()));
    }
    if findings > 0 {
        parts.push(plural(findings, "finding"));
    }
    parts.join(" · ")
}

fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// The nudge under a card that is waiting on a person.
pub fn hint(status: TaskStatus) -> Option<&'static str> {
    match status {
        TaskStatus::Conflicted => Some("Conflicted"),
        TaskStatus::AwaitingReview => Some("awaiting review"),
        _ => None,
    }
}

/// `runner · executor · duration`.
pub fn meta(task: &Task, now: u64) -> String {
    format!(
        "{} · {} · {}",
        task.runner.as_deref().unwrap_or("unassigned"),
        task.spec.executor.binary(),
        duration(elapsed(task, now))
    )
}

/// From the first attempt's start to the last one's end. A task that has not
/// run yet is timed from when it was asked for.
fn elapsed(task: &Task, now: u64) -> u64 {
    let start = task
        .executions
        .first()
        .map_or(task.created_at, |run| run.started_at);
    let end = task
        .executions
        .last()
        .and_then(|run| run.finished_at)
        .unwrap_or(now);
    end.saturating_sub(start)
}

pub fn card(
    app: &LgtmApp,
    task: &Task,
    events: &[StoredEvent],
    now: u64,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    let id = task.id.clone();
    let body = body_line(task, events);
    let failed = task.error.is_some();
    div()
        .id(SharedString::from(format!("card-{id}")))
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .w_full()
        .p(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .cursor_pointer()
        .hover(|this| this.border_color(t.muted_fg))
        .child(header(app, task, now, t))
        .when(!body.is_empty(), |this| {
            this.child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(if failed { t.danger } else { t.muted_fg })
                    .child(body),
            )
        })
        .when_some(hint(task.status), |this, hint| {
            this.child(
                div()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.warning)
                    .child(hint),
            )
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
}

fn header(app: &LgtmApp, task: &Task, now: u64, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(badge(
            status_label(task, &app.tasks),
            status_color(task, &app.tasks, t),
            t,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(prompt_preview(&task.spec.prompt, TITLE)),
        )
        .when_some(
            crate::app::owner_label(app.owner_name(task.created_by.as_deref()), t),
            |this, owner| this.child(owner),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(meta(task, now)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::tests::task;
    use lgtm_protocol::{Finding, Review, Severity, ValidationResult};

    fn stored(event: TaskEvent) -> StoredEvent {
        StoredEvent { at: 0, event }
    }

    #[test]
    fn a_running_card_shows_the_newest_step() {
        let mut running = task("a", "do it");
        running.status = TaskStatus::Running;
        let events = vec![
            stored(TaskEvent::Progress {
                text: "reading".into(),
            }),
            stored(TaskEvent::Command {
                command: "cargo test".into(),
            }),
            stored(TaskEvent::FileChanged {
                path: "src/lib.rs".into(),
            }),
        ];
        assert_eq!(body_line(&running, &events), "cargo test");
        assert_eq!(body_line(&running, &[]), "Working…");
    }

    #[test]
    fn a_finished_card_counts_files_checks_and_findings() {
        let mut done = task("a", "do it");
        done.status = TaskStatus::AwaitingReview;
        done.result = Some(TaskResult {
            branch: "b".into(),
            diff: String::new(),
            changed_files: vec!["one.rs".into()],
            validation: vec![
                ValidationResult {
                    name: "fmt".into(),
                    command: "c".into(),
                    ok: true,
                    output_tail: String::new(),
                },
                ValidationResult {
                    name: "test".into(),
                    command: "c".into(),
                    ok: false,
                    output_tail: String::new(),
                },
            ],
            plan: None,
            review: Some(Review {
                findings: vec![Finding {
                    severity: Severity::Warning,
                    file: String::new(),
                    line: None,
                    message: "m".into(),
                }],
                executor: None,
            }),
            policy: None,
            cost_usd: 0.0,
        });
        assert_eq!(body_line(&done, &[]), "1 file · 1/2 checks · 1 finding");
        assert_eq!(hint(done.status), Some("awaiting review"));
    }

    #[test]
    fn a_failed_card_shows_the_error_before_anything_else() {
        let mut failed = task("a", "do it");
        failed.status = TaskStatus::Failed;
        failed.error = Some("exit 1\nmore".into());
        assert_eq!(body_line(&failed, &[]), "exit 1");
        assert_eq!(hint(failed.status), None);
    }

    #[test]
    fn meta_times_the_run_not_the_wait() {
        let mut queued = task("a", "do it");
        queued.created_at = 1_000;
        assert_eq!(meta(&queued, 61_000), "unassigned · claude · 1m");
    }
}
