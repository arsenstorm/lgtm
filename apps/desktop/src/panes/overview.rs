//! The Overview tab: what the task is, every attempt at it, and what the
//! orchestrator and the other live tasks have to say about it.

use super::{badge, muted};
use crate::app::LgtmApp;
use crate::labels::status_label;
use crate::render;
use crate::tasks::{duration, now_ms, relative_age, repo_slug};
use crate::theme::{section_label, Tokens, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use lgtm_protocol::{Execution, ExecutionStatus, Overlap, StoredEvent, Task, TaskEvent};

/// The key column, wide enough for `Requirements`.
const KEY_W: f32 = 116.;

pub(super) fn overview(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[3]))
        .child(div().flex().flex_col().gap(px(SPACE[0])).children(
            pairs(app, task, t, cx).into_iter().map(|(key, value)| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(SPACE[1]))
                    .child(
                        div()
                            .w(px(KEY_W))
                            .flex_none()
                            .text_color(t.muted_fg)
                            .child(key),
                    )
                    .child(div().flex_1().min_w_0().child(value))
            }),
        ))
        .child(section(
            "Attempts",
            attempts(task, t),
            "No attempts yet.",
            t,
        ))
        .child(section(
            "Policy",
            decision_rows(&app.events, t),
            "No decisions yet.",
            t,
        ))
        .child(section(
            "Overlaps",
            overlap_rows(&app.overlaps, t, cx),
            "No overlapping tasks.",
            t,
        ))
        .into_any_element()
}

fn section(label: &'static str, rows: Vec<AnyElement>, empty: &'static str, t: &Tokens) -> Div {
    let filled = !rows.is_empty();
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .child(section_label(label, t))
        .children(rows)
        .when(!filled, |this| this.child(muted(empty, t)))
}

/// The key/value block, in the order a person reads a task in.
fn pairs(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<(&'static str, AnyElement)> {
    let spec = &task.spec;
    let sandbox = match spec.sandbox {
        Some(profile) => profile.as_str().to_string(),
        None => "repository default".to_string(),
    };
    let worker = task.worker.clone().unwrap_or_else(|| "unassigned".into());
    let mut out = vec![
        ("Status", text(status_label(task, &app.tasks), t)),
        ("Repository", text(repo_slug(&spec.repository), t)),
        ("Base branch", text(spec.base_branch.clone(), t)),
        ("Executor", text(spec.executor.binary(), t)),
        ("Worker", text(worker, t)),
        ("Sandbox", text(sandbox, t)),
    ];
    if !spec.requirements.is_empty() {
        out.push(("Requirements", chips(&spec.requirements, t)));
    }
    if let Some(goal) = spec.goal.clone() {
        out.push(("Goal", goal_link(app, task, goal, t, cx)));
    }
    out.push(("Created", text(relative_age(task.created_at, now_ms()), t)));
    if let Some(result) = task.result.as_ref() {
        out.push(("Cost", text(format!("${:.2}", result.cost_usd), t)));
    }
    out
}

fn text(value: impl Into<SharedString>, t: &Tokens) -> AnyElement {
    div()
        .text_color(t.fg)
        .child(value.into())
        .into_any_element()
}

fn chips(requirements: &[String], t: &Tokens) -> AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(SPACE[0]))
        .children(requirements.iter().map(|name| badge(name.clone(), t.fg, t)))
        .into_any_element()
}

/// The goal's objective, opening the project page's Goals tab at it.
fn goal_link(
    app: &LgtmApp,
    task: &Task,
    goal: String,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    let objective = app
        .goals
        .iter()
        .find(|summary| summary.goal.id == goal)
        .map(|summary| summary.goal.objective.clone())
        .unwrap_or_else(|| goal.clone());
    let slug = repo_slug(&task.spec.repository);
    div()
        .id("goal-link")
        .cursor_pointer()
        .text_color(t.info)
        .child(objective)
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.open_project(slug.clone(), Some(goal.clone()), cx)
        }))
        .into_any_element()
}

fn attempts(task: &Task, t: &Tokens) -> Vec<AnyElement> {
    let now = now_ms();
    task.executions
        .iter()
        .map(|execution| {
            div()
                .flex()
                .flex_col()
                .child(attempt_line(execution, now))
                .when_some(execution.error.clone(), |this, error| {
                    this.child(div().text_color(t.danger).child(error))
                })
                .into_any_element()
        })
        .collect()
}

/// `#n · status · worker · executor[ · model] · duration`.
fn attempt_line(execution: &Execution, now: u64) -> String {
    let ended = execution.finished_at.unwrap_or(now);
    let model = execution
        .model
        .as_deref()
        .map_or(String::new(), |model| format!(" · {model}"));
    format!(
        "#{} · {} · {} · {}{model} · {}",
        execution.attempt,
        execution_status(execution.status),
        execution.worker,
        execution.executor.binary(),
        duration(ended.saturating_sub(execution.started_at))
    )
}

fn execution_status(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Running => "running",
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::Cancelled => "cancelled",
    }
}

fn decision_rows(events: &[StoredEvent], t: &Tokens) -> Vec<AnyElement> {
    decisions(events)
        .into_iter()
        .map(|line| div().text_color(t.fg).child(line).into_any_element())
        .collect()
}

/// The latest policy line and the latest orchestrator line, worded as the
/// Activity tab words them.
fn decisions(events: &[StoredEvent]) -> Vec<String> {
    let latest = |wanted: fn(&TaskEvent) -> bool| {
        events
            .iter()
            .rev()
            .find(|stored| wanted(&stored.event))
            .map(|stored| render::render(&stored.event))
            .unwrap_or_default()
    };
    let mut lines = latest(|event| matches!(event, TaskEvent::PolicyDecision { .. }));
    lines.extend(latest(|event| {
        matches!(event, TaskEvent::Orchestrated { .. })
    }));
    lines.into_iter().map(|line| line.text).collect()
}

fn overlap_rows(overlaps: &[Overlap], t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    overlaps
        .iter()
        .map(|overlap| {
            let id = overlap.task.clone();
            div()
                .flex()
                .gap(px(SPACE[0]))
                .child(div().text_color(t.muted_fg).child("overlaps with"))
                .child(
                    div()
                        .id(SharedString::from(format!("overlap:{id}")))
                        .cursor_pointer()
                        .text_color(t.info)
                        .child(id.clone())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.select(id.clone(), cx)
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.muted_fg)
                        .child(overlap.files.join(", ")),
                )
                .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::Executor;

    fn execution(finished_at: Option<u64>) -> Execution {
        Execution {
            attempt: 2,
            worker: "compute".into(),
            executor: Executor::Codex,
            started_at: 1_000,
            finished_at,
            status: ExecutionStatus::Completed,
            error: None,
            cost_usd: 0.0,
            validation: Vec::new(),
            model: None,
        }
    }

    #[test]
    fn an_attempt_reads_as_one_line() {
        assert_eq!(
            attempt_line(&execution(Some(91_000)), 200_000),
            "#2 · completed · compute · codex · 1m"
        );
    }

    /// A running attempt has no end, so the row ages against the clock.
    #[test]
    fn an_unfinished_attempt_is_measured_to_now() {
        assert_eq!(
            attempt_line(&execution(None), 13_000),
            "#2 · completed · compute · codex · 12s"
        );
    }

    fn stored(event: TaskEvent) -> StoredEvent {
        StoredEvent { at: 0, event }
    }

    #[test]
    fn only_the_last_policy_and_orchestrator_lines_show() {
        let events = vec![
            stored(TaskEvent::PolicyDecision {
                action: "approve".into(),
                allowed: false,
                reasons: vec!["too big".into()],
            }),
            stored(TaskEvent::Started { model: None }),
            stored(TaskEvent::PolicyDecision {
                action: "merge".into(),
                allowed: true,
                reasons: vec!["ci green".into()],
            }),
            stored(TaskEvent::Orchestrated {
                action: "retry".into(),
                reason: "flaky check".into(),
                applied: true,
                note: String::new(),
            }),
        ];
        assert_eq!(
            decisions(&events),
            vec![
                "policy: auto-merge (ci green)".to_string(),
                "orchestrator: retry — flaky check".to_string(),
            ]
        );
    }

    #[test]
    fn no_decisions_means_no_lines() {
        assert!(decisions(&[stored(TaskEvent::Started { model: None })]).is_empty());
    }
}
