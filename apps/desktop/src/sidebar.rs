//! The left rail: quick actions, tasks grouped by repository, and one status
//! row that opens Settings.

use crate::app::{LgtmApp, Overlay, Page};
use crate::labels::prompt_preview;
use crate::project::goals_of;
use crate::tasks::{goal_color, group_by_repo};
use crate::theme::{
    icon, icon_button, tokens, Tokens, FOOTER_H, ICON, ROW_H, SPACE, TEXT_BODY, TEXT_ROW,
    TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use lgtm_protocol::{GoalSummary, Task, TaskStatus};

pub const WIDTH: f32 = 240.;
const PROMPT_PREVIEW: usize = 34;
/// Sidebar rows are the one place that keeps `rounded-md`; a pill this short
/// would read as a lozenge, not a list.
const ROW_RADIUS: f32 = 8.;
/// The row carrying the product name.
const BRAND_H: f32 = 36.;
/// How far a task sits under its project.
const NEST: f32 = 24.;
/// Tasks shown per project before `Show more`.
const PER_PROJECT: usize = 6;
/// The circle around the connection dot.
const STATUS_DOT: f32 = 20.;

pub fn render_sidebar(app: &mut LgtmApp, _window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
    let t = tokens(cx);
    div()
        .w(px(WIDTH))
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(t.sidebar)
        .child(brand(app, &t, cx))
        .child(nav(app, &t, cx))
        .child(
            div()
                .id("tasks")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.ui.task_scroll)
                .px(px(SPACE[1]))
                .pb(px(SPACE[1]))
                .children(repository_groups(app, &t, cx)),
        )
        .child(footer(app, &t, cx))
}

/// The product name, and the way into the palette.
fn brand(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let searching = app.ui.overlay == Overlay::Palette;
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(BRAND_H))
        .pl(px(SPACE[2]))
        .pr(px(SPACE[1]))
        .child(
            div()
                .flex_1()
                .text_size(px(TEXT_BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .child("LGTM"),
        )
        .child(icon_button("search", "search", !searching, t).on_click(
            cx.listener(|this, _: &ClickEvent, window, cx| this.open_palette(window, cx)),
        ))
}

fn nav(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let home = app.selected.is_none() && app.page == Page::Home;
    let batches = app.selected.is_none() && app.page == Page::Batches;
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .px(px(SPACE[1]))
        .gap(px(2.))
        .child(
            nav_row(&NEW_TASK, home, t)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.go_home(window, cx))),
        )
        .child(
            nav_row(&BATCHES, batches, t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.show_page(Page::Batches, cx)),
            ),
        )
}

/// The shared sidebar row: 28px tall, `accent` on hover, `accent` plus
/// full-strength text when it is the current one.
fn row_shell(id: impl Into<SharedString>, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .text_size(px(TEXT_ROW))
        .text_color(if active { t.fg } else { t.muted_fg })
        .when(active, |this| this.bg(t.muted))
        .hover(|this| this.bg(t.muted))
}

struct NavItem {
    id: &'static str,
    icon: &'static str,
    label: &'static str,
}

const NEW_TASK: NavItem = NavItem {
    id: "new-task",
    icon: "square-pen",
    label: "New task",
};
const BATCHES: NavItem = NavItem {
    id: "batches",
    icon: "list-checks",
    label: "Batches",
};

fn nav_row(item: &NavItem, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    row_shell(item.id, active, t)
        .child(icon(item.icon, ICON, t.muted_fg))
        .child(div().flex_1().min_w_0().truncate().child(item.label))
}

fn repository_groups(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut out = vec![div()
        .h(px(ROW_H))
        .flex()
        .items_center()
        .px(px(SPACE[1]))
        .pt(px(SPACE[1]))
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .child("Projects")
        .into_any_element()];

    if app.tasks.is_empty() {
        out.push(
            div()
                .px(px(SPACE[1]))
                .py(px(SPACE[0]))
                .text_size(px(TEXT_ROW))
                .text_color(t.muted_fg)
                .child("No tasks yet")
                .into_any_element(),
        );
        return out;
    }

    for (slug, rows) in group_by_repo(&app.tasks) {
        out.extend(project_group(app, &slug, rows, t, cx));
    }
    out
}

/// A project: its header, then its goals, then the tasks no goal claims.
fn project_group(
    app: &LgtmApp,
    slug: &str,
    rows: Vec<&Task>,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    let loose: Vec<&Task> = rows
        .into_iter()
        .filter(|task| task.spec.goal.is_none())
        .collect();
    let key = format!("repo:{slug}");
    let shown = match app.ui.expanded.contains(&key) {
        true => loose.len(),
        false => PER_PROJECT,
    };
    let mut out = vec![repo_header(app, slug, t, cx).into_any_element()];
    out.extend(
        goals_of(app, slug)
            .into_iter()
            .map(|summary| goal_row(slug, summary, t, cx).into_any_element()),
    );
    out.extend(
        loose
            .iter()
            .take(shown)
            .map(|task| task_row(app, task, t, cx).into_any_element()),
    );
    if loose.len() > shown {
        out.push(show_more(key, t, cx));
    }
    out
}

fn repo_header(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let active = app.page == Page::Project(slug.to_string()) && app.selected.is_none();
    let open = slug.to_string();
    row_shell(SharedString::from(format!("repo-{slug}")), active, t)
        .text_color(t.fg)
        .child(icon("folder", ICON, t.muted_fg))
        .child(div().min_w_0().truncate().child(slug.to_string()))
        .on_click(
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.open_project(open.clone(), None, cx)
            }),
        )
}

/// A goal under its project: the objective, dotted with how it is going.
fn goal_row(
    slug: &str,
    summary: &GoalSummary,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = summary.goal.id.clone();
    let open = slug.to_string();
    row_shell(SharedString::from(format!("goal-{id}")), false, t)
        .pl(px(NEST))
        .child(dot(6., goal_color(summary.status, t)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(prompt_preview(&summary.goal.objective, PROMPT_PREVIEW)),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.open_project(open.clone(), Some(id.clone()), cx)
        }))
}

fn show_more(key: String, t: &Tokens, cx: &mut Context<LgtmApp>) -> AnyElement {
    row_shell(SharedString::from(format!("more-{key}")), false, t)
        .pl(px(NEST))
        .child("Show more")
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.ui.expanded.insert(key.clone());
            cx.notify();
        }))
        .into_any_element()
}

fn task_row(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = task.id.clone();
    let active = app.selected.as_deref() == Some(id.as_str());
    let running = task.status == TaskStatus::Running;

    row_shell(SharedString::from(format!("task-{id}")), active, t)
        .pl(px(NEST))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(prompt_preview(&task.spec.prompt, PROMPT_PREVIEW)),
        )
        .when(running, |this| this.child(dot(6., t.info)))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
}

fn dot(size: f32, color: gpui::Hsla) -> Div {
    div()
        .flex_shrink_0()
        .w(px(size))
        .h(px(size))
        .rounded_full()
        .bg(color)
}

/// A dot on a halo of its own colour.
fn status_dot(tone: gpui::Hsla) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(STATUS_DOT))
        .h(px(STATUS_DOT))
        .rounded_full()
        .bg(gpui::Hsla { a: 0.15, ..tone })
        .child(dot(6., tone))
}

/// One row: whether the orchestrator answered, and the way into Settings.
fn footer(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> gpui::Stateful<Div> {
    let status = if app.link.reachable {
        let n = app.workers.len();
        format!("Connected · {n} worker{}", if n == 1 { "" } else { "s" })
    } else {
        "Not connected".to_string()
    };
    let tone = if app.link.reachable {
        t.success
    } else {
        t.danger
    };
    div()
        .id("status")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(FOOTER_H))
        .pl(px(SPACE[1]))
        .pr(px(SPACE[1]))
        .text_size(px(TEXT_ROW))
        .text_color(t.muted_fg)
        .cursor_pointer()
        .hover(|this| this.text_color(t.fg))
        .child(status_dot(tone))
        .child(div().flex_1().min_w_0().truncate().child(status))
        .child(
            icon_button("open-settings", "settings", true, t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_worker_settings(cx))),
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_worker_settings(cx)))
}
