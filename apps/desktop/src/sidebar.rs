//! The left rail: quick actions, tasks grouped by repository, and one status
//! row that opens Settings.

use crate::app::{LgtmApp, Overlay, Page};
use crate::labels::prompt_preview;
use crate::tasks::group_by_repo;
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
use lgtm_protocol::{Task, TaskStatus};

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
                .track_scroll(&app.task_scroll)
                .px(px(SPACE[1]))
                .pb(px(SPACE[1]))
                .children(repository_groups(app, &t, cx)),
        )
        .child(footer(app, &t, cx))
}

/// The product name, and the way into the palette.
fn brand(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let searching = app.overlay == Overlay::Palette;
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
        let key = format!("repo:{slug}");
        let expanded = app.expanded.contains(&key);
        let hidden = rows.len().saturating_sub(PER_PROJECT);
        out.push(repo_header(slug, t));
        let shown = if expanded { rows.len() } else { PER_PROJECT };
        for task in rows.into_iter().take(shown) {
            out.push(task_row(app, task, t, cx).into_any_element());
        }
        if hidden > 0 && !expanded {
            out.push(show_more(key, t, cx));
        }
    }
    out
}

fn repo_header(slug: String, t: &Tokens) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .text_size(px(TEXT_ROW))
        .text_color(t.fg)
        .child(icon("folder", ICON, t.muted_fg))
        .child(div().min_w_0().truncate().child(slug))
        .into_any_element()
}

fn show_more(key: String, t: &Tokens, cx: &mut Context<LgtmApp>) -> AnyElement {
    row_shell(SharedString::from(format!("more-{key}")), false, t)
        .pl(px(NEST))
        .child("Show more")
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.expanded.insert(key.clone());
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

/// One row: whether the orchestrator answered, and the way into Settings.
fn footer(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> gpui::Stateful<Div> {
    let status = if app.reachable {
        let n = app.workers.len();
        format!("Connected · {n} worker{}", if n == 1 { "" } else { "s" })
    } else {
        "Not connected".to_string()
    };
    let tone = if app.reachable { t.success } else { t.danger };
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
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .w(px(STATUS_DOT))
                .h(px(STATUS_DOT))
                .rounded_full()
                .bg(gpui::Hsla { a: 0.15, ..tone })
                .child(dot(6., tone)),
        )
        .child(div().flex_1().min_w_0().truncate().child(status))
        .child(
            icon_button("open-settings", "settings", true, t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_worker_settings(cx))),
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_worker_settings(cx)))
}
