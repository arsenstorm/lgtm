//! The task view: header with status and actions, its tabs, and the inspector
//! and terminal panels that sit beside and under whichever tab is open.

mod inspector;
mod review_tab;
mod tabs;
mod terminal;

use crate::app::{LgtmApp, Pane};
use crate::labels::{header_preview, status_label};
use crate::net::Action;
use crate::tasks::repo_slug;
use crate::theme::{
    field, icon, icon_button, tokens, Header, TabularNums as _, Tokens, RADIUS_PILL, SPACE,
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
use lgtm_protocol::{CiState, CiStatus, Task, TaskStatus};

use tabs::{activity, plan_pane};

/// `h-5`, the reference badge height.
const BADGE_H: f32 = 20.;
/// The inspector column: wide enough for the facts block's key and value.
const INSPECTOR_W: f32 = 320.;
/// The terminal drawer: a dozen mono lines, enough to read a command's answer
/// without giving up the tab above it.
const DRAWER_H: f32 = 240.;

pub fn task_view(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let Some(task) = app.selected_task().cloned() else {
        return div().flex_1().into_any_element();
    };

    let has_plan = task.result.as_ref().is_some_and(|r| r.plan.is_some());
    // A task selected while on the Plan tab may not have a plan; fall back for
    // that render without losing the user's tab choice.
    let pane = if app.pane == Pane::Plan && !has_plan {
        Pane::Activity
    } else {
        app.pane
    };
    let (inspector_open, terminal_open) = (app.ui.inspector_open, app.ui.terminal_open);
    let finished = task.status.is_terminal();
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .children(notices(app, &t))
        .child(div().px(px(SPACE[1])).child(tab_bar(pane, has_plan, cx)))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(body(app, pane, &task, &t, window, cx)),
                )
                .when(inspector_open, |this| {
                    this.child(inspector_panel(app, &task, &t, cx))
                }),
        )
        .when(terminal_open, |this| {
            this.child(
                div()
                    .flex_shrink_0()
                    .h(px(DRAWER_H))
                    .border_t_1()
                    .border_color(t.border)
                    .child(terminal::terminal(app, finished, &t)),
            )
        })
        .into_any_element()
}

/// The facts-and-notes column, beside whichever tab is open. It scrolls on its
/// own so reading the notes does not move the pane.
fn inspector_panel(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> impl IntoElement {
    div()
        .id("inspector")
        .flex_none()
        .w(px(INSPECTOR_W))
        .h_full()
        .overflow_y_scroll()
        .track_scroll(&app.ui.inspector_scroll)
        .border_l_1()
        .border_color(t.border)
        .px(px(SPACE[2]))
        .py(px(SPACE[2]))
        .child(inspector::inspector(app, task, t, cx))
}

fn body(
    app: &mut LgtmApp,
    pane: Pane,
    task: &Task,
    t: &Tokens,
    window: &mut Window,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    match pane {
        Pane::Changes => div()
            .flex_1()
            .min_h_0()
            .child(crate::changes::changes_pane(app, window, cx))
            .into_any_element(),
        Pane::Review => scrolling(app, review_tab::review(app, task, t, cx)),
        Pane::Plan => scrolling(app, plan_pane(task, t)),
        Pane::Activity => scrolling(app, activity(app, t, cx)),
    }
}

/// The follow-up field and the error line, when either is showing.
fn notices(app: &LgtmApp, t: &Tokens) -> Vec<Div> {
    let mut out = Vec::new();
    if app.ui.show_follow_up {
        out.push(
            div()
                .px(px(SPACE[1]))
                .pt(px(SPACE[1]))
                .child(field(&app.inputs.follow_up, t)),
        );
    }
    if let Some(error) = app.error.clone() {
        out.push(
            div()
                .px(px(SPACE[2]))
                .pt(px(SPACE[1]))
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.danger)
                .child(error),
        );
    }
    out
}

/// The tabs, in order. Plan is only there for a task that produced one.
fn tabs_for(has_plan: bool) -> Vec<(Pane, &'static str)> {
    let mut tabs = vec![
        (Pane::Activity, "Activity"),
        (Pane::Changes, "Changes"),
        (Pane::Review, "Review"),
    ];
    if has_plan {
        tabs.push((Pane::Plan, "Plan"));
    }
    tabs
}

fn tab_bar(pane: Pane, has_plan: bool, cx: &mut Context<LgtmApp>) -> TabBar {
    let tabs = tabs_for(has_plan);
    let at = tabs.iter().position(|(one, _)| *one == pane).unwrap_or(0);
    TabBar::new("panes")
        .underline()
        .text_size(px(TEXT_SECONDARY))
        .selected_index(at)
        .children(tabs.iter().map(|(_, label)| Tab::new().label(*label)))
        .on_click(cx.listener(move |this, index: &usize, _, cx| {
            if let Some((pane, _)) = tabs_for(has_plan).get(*index) {
                this.show(*pane, cx);
            }
        }))
}

/// The scrolling body shared by every pane but Changes. It sets
/// no type of its own: a pane that is prose picks the UI font, a pane that is
/// data picks mono, and neither inherits the other's.
fn scrolling(app: &LgtmApp, body: impl IntoElement) -> AnyElement {
    div()
        .id("pane-content")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&app.ui.content_scroll)
        .px(px(SPACE[2]))
        .pt(px(SPACE[2]))
        .pb(px(SPACE[4]))
        .child(body)
        .into_any_element()
}

pub(crate) fn task_header(
    app: &mut LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Div {
    let status = status_label(task, &app.tasks);
    let (inspector_open, terminal_open) = (app.ui.inspector_open, app.ui.terminal_open);
    Header::new(header_preview(&task.spec.prompt))
        .detail(badge(status, status_tone(status, t), t))
        .detail(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .tabular_nums()
                .child(meta_line(task)),
        )
        .details(chips(task, t, cx))
        .action(actions(task, t, cx))
        // The header is drawn on the window bar, so a toggle has to occlude
        // it or a second quick click would reach the bar and zoom the window.
        .action(
            icon_button("toggle-terminal", "terminal", true, t)
                .when(terminal_open, |this| this.bg(t.wash))
                .occlude()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_terminal(cx))),
        )
        .action(
            icon_button("toggle-inspector", "info", true, t)
                .when(inspector_open, |this| this.bg(t.wash))
                .occlude()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.toggle_inspector(cx))),
        )
        .render()
        .px(px(SPACE[1]))
        .border_b_1()
        .border_color(t.border)
}

/// The pull request and Linear chips, for the task that has them.
fn chips(task: &Task, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    if let Some(pr) = task.pull_request.clone() {
        let (mark, tone) = ci_mark(task.ci.as_ref(), t);
        let chip = badge(format!("#{}", pr.number), tone, t)
            .when_some(mark, |this, name| this.child(icon(name, MARK, tone)));
        out.push(link_chip(chip, "pr-chip", pr.url, cx).into_any_element());
    }
    if let Some(linear) = task.spec.linear.clone() {
        let chip = badge(linear.identifier, t.fg, t);
        out.push(link_chip(chip, "linear-chip", linear.url, cx).into_any_element());
    }
    out
}

/// `repo · base · runner`, and the cost once there is one.
fn meta_line(task: &Task) -> String {
    let runner = task.runner.as_deref().unwrap_or("unassigned");
    let cost = task.result.as_ref().map(|r| r.cost_usd).unwrap_or(0.0);
    let mut meta = format!(
        "{} · {} · {runner}",
        repo_slug(&task.spec.repository),
        task.spec.base_branch
    );
    if cost > 0.0 {
        meta.push_str(&format!(" · ${cost:.2}"));
    }
    meta
}

/// A badge that opens `url` in the browser.
fn link_chip(
    chip: Div,
    id: &'static str,
    url: String,
    cx: &mut Context<LgtmApp>,
) -> impl IntoElement {
    chip.id(id)
        .cursor_pointer()
        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
}

/// A ghost button that reads as destructive: no fill, danger-coloured label.
pub(super) fn danger_ghost(t: &Tokens, cx: &App) -> ButtonCustomVariant {
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
        TaskStatus::AwaitingReview => row,
        TaskStatus::Queued | TaskStatus::Running => row.child(
            Button::new("cancel")
                .label("Cancel")
                .custom(danger_ghost(t, cx))
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Cancel, cx))),
        ),
        TaskStatus::Failed
        | TaskStatus::TimedOut
        | TaskStatus::RunnerLost
        | TaskStatus::Cancelled => row.child(
            Button::new("retry")
                .label("Retry")
                .outline()
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Retry, cx))),
        ),
        TaskStatus::Approved if ci_passed(task) => row.child(
            Button::new("merge")
                .label("Merge")
                .primary()
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Merge, cx))),
        ),
        _ => row,
    }
}

fn ci_passed(task: &Task) -> bool {
    matches!(
        task.ci,
        Some(CiStatus {
            state: CiState::Success,
            ..
        })
    )
}

pub(super) fn review_actions(t: &Tokens, cx: &mut Context<LgtmApp>) -> [Button; 3] {
    [
        Button::new("approve")
            .label("Approve changes")
            .primary()
            .small()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Approve, cx))),
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
        Button::new("reject")
            .label("Reject task")
            .custom(danger_ghost(t, cx))
            .small()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.act(Action::Reject, cx))),
    ]
}

fn status_tone(status: &str, t: &Tokens) -> Hsla {
    match status {
        "awaiting review" | "conflicted" => t.warning,
        "running" | "changes requested" => t.info,
        "approved" | "merged" => t.success,
        "failed" | "rejected" | "cancelled" => t.danger,
        _ => t.muted_fg,
    }
}

/// The shadcn badge: a `bg-muted` pill, `text-xs`, coloured by what it reports.
pub(super) fn badge(label: impl Into<SharedString>, tone: Hsla, t: &Tokens) -> Div {
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

/// A greyed line standing in for a section with nothing in it.
pub(super) fn muted(text: &'static str, t: &Tokens) -> AnyElement {
    div().text_color(t.muted_fg).child(text).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plan_tab_is_last_and_only_for_a_task_with_a_plan() {
        let labels = |has_plan| -> Vec<&'static str> {
            tabs_for(has_plan).into_iter().map(|(_, l)| l).collect()
        };
        assert_eq!(labels(false), vec!["Activity", "Changes", "Review"]);
        assert_eq!(labels(true).last(), Some(&"Plan"));
    }
}
