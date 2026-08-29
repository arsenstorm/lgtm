//! The Activity, Checks, and Plan tabs.

use super::MARK;
use crate::app::LgtmApp;
use crate::render::Kind;
use crate::theme::{icon, Tokens, SPACE};
use gpui::prelude::FluentBuilder as _;
use gpui::{div, px, AnyElement, Div, FontWeight, IntoElement, ParentElement as _, Styled as _};
use lgtm_protocol::{Finding, Severity, Task, ValidationResult};

pub(super) fn activity(app: &LgtmApp, t: &Tokens) -> AnyElement {
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

pub(super) fn checks(task: &Task, t: &Tokens) -> AnyElement {
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
        .children(checks.iter().map(|check| check_row(check, t)))
        .when(!findings.is_empty(), |this| {
            this.child(
                div()
                    .pt(px(SPACE[1]))
                    .text_color(t.muted_fg)
                    .font_weight(FontWeight::BOLD)
                    .child("Review"),
            )
            .children(findings.iter().map(|finding| finding_row(finding, t)))
        })
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

fn finding_row(finding: &Finding, t: &Tokens) -> Div {
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
}

pub(super) fn plan_pane(task: &Task, t: &Tokens) -> AnyElement {
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
