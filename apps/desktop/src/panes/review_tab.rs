//! The Review tab: the checks, the reviewer's findings, and the decision the
//! task is waiting on.

mod artefacts;

use super::{danger_ghost, review_actions, MARK};
use crate::app::LgtmApp;
use crate::net::Action;
use crate::tasks::{now_ms, relative_age};
use crate::theme::{
    field, icon, section, Tokens, LINE_MONO, MONO_FONT, SPACE, TEXT_MONO, TEXT_ROW, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use lgtm_protocol::{
    pending_requests, CiState, Finding, Severity, StoredEvent, Task, TaskEvent, TaskStatus,
    ValidationResult,
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
        .child(decision(app, task, t, cx))
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

/// Where the task stands: the decision, or what it is waiting on. First in the
/// tab, and there for every status.
fn decision(app: &LgtmApp, task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let shell = section("Decision", t).children(
        decision_lines(task, &app.events, now_ms())
            .iter()
            .map(|line| line_row(line, t)),
    );
    match task.status {
        TaskStatus::AwaitingReview => shell.child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .children(review_actions(t, cx)),
        ),
        TaskStatus::Conflicted => shell.child(conflict(app, t, cx)),
        _ => shell,
    }
}

/// The tone a decision line carries, resolved to a colour only at render.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Tone {
    Success,
    Danger,
    Warning,
    Info,
    Muted,
}

#[derive(Debug, PartialEq)]
struct Line {
    mark: Option<&'static str>,
    tone: Tone,
    text: String,
}

fn mark_line(mark: &'static str, tone: Tone, text: impl Into<String>) -> Line {
    Line {
        mark: Some(mark),
        tone,
        text: text.into(),
    }
}

fn note(text: impl Into<String>) -> Line {
    Line {
        mark: None,
        tone: Tone::Muted,
        text: text.into(),
    }
}

fn tone_color(tone: Tone, t: &Tokens) -> Hsla {
    match tone {
        Tone::Success => t.success,
        Tone::Danger => t.danger,
        Tone::Warning => t.warning,
        Tone::Info => t.info,
        Tone::Muted => t.muted_fg,
    }
}

/// Like a check row: the mark carries the tone, the words stay plain.
fn line_row(line: &Line, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .when_some(line.mark, |this, mark| {
            this.child(icon(mark, MARK, tone_color(line.tone, t)))
        })
        .when(line.tone == Tone::Muted, |this| this.text_color(t.muted_fg))
        .child(line.text.clone())
}

/// What Review says about a task, one fact per line. `Conflicted` has none:
/// the conflict composer speaks for itself.
fn decision_lines(task: &Task, events: &[StoredEvent], now: u64) -> Vec<Line> {
    match task.status {
        TaskStatus::AwaitingReview => {
            let mut lines = summary(task);
            lines.push(note(
                "Approve this result to finish the review. A pull request can merge only after approval and passing CI.",
            ));
            lines
        }
        TaskStatus::Approved => vec![
            decided("Approved", &TaskEvent::AutoApproved, events, now),
            merge_readiness(task),
        ],
        TaskStatus::Merged => vec![decided("Merged", &TaskEvent::AutoMerged, events, now)],
        TaskStatus::Rejected => vec![mark_line("x", Tone::Danger, "Rejected")],
        TaskStatus::ChangesRequested => vec![mark_line(
            "circle-dot",
            Tone::Info,
            "Changes requested — the agent is revising.",
        )],
        TaskStatus::Conflicted => Vec::new(),
        TaskStatus::Queued | TaskStatus::Running => {
            vec![note("The task has not reached review yet.")]
        }
        TaskStatus::Failed
        | TaskStatus::TimedOut
        | TaskStatus::RunnerLost
        | TaskStatus::Cancelled => ended(task),
    }
}

/// The facts a reviewer decides on: how the checks went, what the review
/// found, where CI stands.
fn summary(task: &Task) -> Vec<Line> {
    let result = task.result.as_ref();
    let checks = result.map(|r| r.validation.as_slice()).unwrap_or_default();
    let mut lines = Vec::new();
    if !checks.is_empty() {
        let passed = checks.iter().filter(|check| check.ok).count();
        let all = passed == checks.len();
        lines.push(mark_line(
            if all { "check" } else { "x" },
            if all { Tone::Success } else { Tone::Danger },
            format!("{passed} of {} checks passed", checks.len()),
        ));
    }
    if let Some(review) = result.and_then(|r| r.review.as_ref()) {
        let blocking = review
            .findings
            .iter()
            .filter(|finding| finding.severity == Severity::Blocking)
            .count();
        lines.push(if blocking > 0 {
            mark_line(
                "circle-dot",
                Tone::Warning,
                format!("{} findings ({blocking} blocking)", review.findings.len()),
            )
        } else {
            mark_line("check", Tone::Success, "No blocking findings")
        });
    }
    if task.pull_request.is_some() {
        lines.extend(task.ci.as_ref().map(|ci| match ci.state {
            CiState::Success => mark_line("check", Tone::Success, "CI passing"),
            CiState::Failure => mark_line("x", Tone::Danger, "CI failing"),
            CiState::Pending => mark_line("ellipsis", Tone::Muted, "CI pending"),
        }));
    }
    lines
}

/// A decision headline, saying so when policy made it — and only then how long
/// ago, since the event is the one thing that carries a time.
fn decided(word: &str, auto: &TaskEvent, events: &[StoredEvent], now: u64) -> Line {
    let text = match events.iter().rev().find(|stored| &stored.event == auto) {
        Some(stored) => format!("{word} by policy {} ago", relative_age(stored.at, now)),
        None => word.to_string(),
    };
    mark_line("check", Tone::Success, text)
}

/// Merging is the header's button; this only says whether it would work.
fn merge_readiness(task: &Task) -> Line {
    if task.pull_request.is_none() {
        return note("No pull request yet.");
    }
    match task.ci.as_ref().map(|ci| ci.state) {
        Some(CiState::Success) => note("Ready to merge — use Merge in the header."),
        Some(CiState::Failure) => note("CI is failing — not ready to merge."),
        _ => note("CI has not finished yet."),
    }
}

fn ended(task: &Task) -> Vec<Line> {
    let what = match task.status {
        TaskStatus::TimedOut => "The run timed out",
        TaskStatus::RunnerLost => "The runner went away",
        TaskStatus::Cancelled => "The task was cancelled",
        _ => "The run failed",
    };
    let mut lines = vec![note(format!(
        "{what} — the error is in Activity, and Retry is in the header."
    ))];
    lines.extend(
        task.error
            .as_deref()
            .and_then(|error| error.lines().next())
            .map(note),
    );
    lines
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
    use lgtm_protocol::{CiStatus, Executor, PullRequest, Review, TaskKind, TaskResult, TaskSpec};

    fn stored(event: TaskEvent) -> StoredEvent {
        StoredEvent { at: 0, event }
    }

    fn task(status: TaskStatus) -> Task {
        Task {
            id: "t".into(),
            spec: TaskSpec {
                repository: "r".into(),
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
                batch: None,
                sandbox: None,
                requirements: vec![],
                review_executor: None,
                model: None,
                goal: None,
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

    fn result(validation: Vec<ValidationResult>, review: Option<Review>) -> TaskResult {
        TaskResult {
            branch: "b".into(),
            diff: String::new(),
            changed_files: Vec::new(),
            validation,
            plan: None,
            review,
            policy: None,
            cost_usd: 0.,
        }
    }

    fn check(name: &str, ok: bool) -> ValidationResult {
        ValidationResult {
            name: name.into(),
            command: "c".into(),
            ok,
            output_tail: String::new(),
        }
    }

    fn finding(severity: Severity) -> Finding {
        Finding {
            severity,
            file: "a.rs".into(),
            line: None,
            message: "m".into(),
        }
    }

    fn texts(lines: &[Line]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn the_summary_counts_checks_findings_and_ci() {
        let mut task = task(TaskStatus::AwaitingReview);
        assert!(summary(&task).is_empty());

        task.result = Some(result(
            vec![check("fmt", true), check("clippy", false)],
            Some(Review {
                findings: vec![finding(Severity::Blocking), finding(Severity::Warning)],
                executor: None,
            }),
        ));
        task.pull_request = Some(PullRequest {
            number: 1,
            url: "u".into(),
        });
        task.ci = Some(CiStatus {
            state: CiState::Failure,
            url: "u".into(),
        });
        assert_eq!(
            texts(&summary(&task)),
            vec![
                "1 of 2 checks passed",
                "2 findings (1 blocking)",
                "CI failing"
            ]
        );
        assert_eq!(
            summary(&task)
                .iter()
                .map(|line| line.tone)
                .collect::<Vec<_>>(),
            vec![Tone::Danger, Tone::Warning, Tone::Danger]
        );
    }

    #[test]
    fn a_clean_review_without_a_pull_request_says_so_and_stays_quiet_about_ci() {
        let mut task = task(TaskStatus::AwaitingReview);
        task.result = Some(result(vec![check("fmt", true)], Some(Review::default())));
        // CI belongs to a pull request; without one there is nothing to report.
        task.ci = Some(CiStatus {
            state: CiState::Pending,
            url: "u".into(),
        });
        assert_eq!(
            texts(&summary(&task)),
            vec!["1 of 1 checks passed", "No blocking findings"]
        );
    }

    #[test]
    fn an_approval_says_who_made_it_when_and_whether_it_can_merge() {
        let mut task = task(TaskStatus::Approved);
        assert_eq!(
            texts(&decision_lines(&task, &[], 0)),
            vec!["Approved", "No pull request yet."]
        );

        task.pull_request = Some(PullRequest {
            number: 1,
            url: "u".into(),
        });
        task.ci = Some(CiStatus {
            state: CiState::Success,
            url: "u".into(),
        });
        let events = vec![StoredEvent {
            at: 0,
            event: TaskEvent::AutoApproved,
        }];
        assert_eq!(
            texts(&decision_lines(&task, &events, 300_000)),
            vec![
                "Approved by policy 5m ago",
                "Ready to merge — use Merge in the header."
            ]
        );
    }

    #[test]
    fn a_failed_run_points_at_activity_and_carries_the_first_error_line() {
        assert!(decision_lines(&task(TaskStatus::Conflicted), &[], 0).is_empty());
        let mut task = task(TaskStatus::Failed);
        task.error = Some("boom\nstack".into());
        assert_eq!(
            texts(&decision_lines(&task, &[], 0)),
            vec![
                "The run failed — the error is in Activity, and Retry is in the header.",
                "boom"
            ]
        );
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
