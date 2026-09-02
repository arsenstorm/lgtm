//! The left rail: quick actions, sessions grouped by repository, and one
//! status row that opens Settings.

use crate::app::{LgtmApp, Overlay, Page};
use crate::labels::prompt_preview;
use crate::project::goals_of;
use crate::tasks::{goal_color, repo_slug};
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
use lgtm_protocol::{GoalSummary, Session};

pub const WIDTH: f32 = 240.;
const PROMPT_PREVIEW: usize = 34;
/// Sidebar rows are the one place that keeps `rounded-md`; a pill this short
/// would read as a lozenge, not a list.
const ROW_RADIUS: f32 = 8.;
/// The row carrying the product name.
const BRAND_H: f32 = 36.;
/// How far a session sits under its project.
const NEST: f32 = 24.;
/// Sessions shown per project before `Show more`.
const PER_PROJECT: usize = 5;
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
    let activity = app.selected.is_none() && app.page == Page::Activity;
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .px(px(SPACE[1]))
        .gap(px(2.))
        .child(
            nav_row(&NEW_SESSION, home, t)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.go_home(window, cx))),
        )
        .child(
            nav_row(&BATCHES, batches, t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.show_page(Page::Batches, cx)),
            ),
        )
        .child(nav_row(&ACTIVITY, activity, t).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| this.show_page(Page::Activity, cx)),
        ))
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

const NEW_SESSION: NavItem = NavItem {
    id: "new-session",
    icon: "square-pen",
    label: "New session",
};
const BATCHES: NavItem = NavItem {
    id: "batches",
    icon: "list-checks",
    label: "Batches",
};
const ACTIVITY: NavItem = NavItem {
    id: "activity",
    icon: "activity",
    label: "Activity",
};

fn nav_row(item: &NavItem, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    row_shell(item.id, active, t)
        .child(icon(item.icon, ICON, t.muted_fg))
        .child(div().flex_1().min_w_0().truncate().child(item.label))
}

/// Every project the orchestrator knows, most recently touched first: the
/// sessions name most of them, and a repository with only tasks still counts.
fn project_slugs(app: &LgtmApp) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for slug in app
        .sessions
        .iter()
        .map(|open| repo_slug(&open.repository))
        .chain(
            app.tasks
                .iter()
                .map(|task| repo_slug(&task.spec.repository)),
        )
    {
        if !out.contains(&slug) {
            out.push(slug);
        }
    }
    out
}

fn sessions_of<'a>(app: &'a LgtmApp, slug: &str) -> Vec<&'a Session> {
    app.sessions
        .iter()
        .filter(|open| repo_slug(&open.repository) == slug)
        .collect()
}

/// A session's row label: its first message, or what an unused thread is
/// called until one is sent.
pub fn session_title(session: &Session) -> String {
    match session.title.trim().is_empty() {
        true => "New session".to_string(),
        false => prompt_preview(&session.title, PROMPT_PREVIEW),
    }
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

    let slugs = project_slugs(app);
    if slugs.is_empty() {
        out.push(
            div()
                .px(px(SPACE[1]))
                .py(px(SPACE[0]))
                .text_size(px(TEXT_ROW))
                .text_color(t.muted_fg)
                .child("No projects yet")
                .into_any_element(),
        );
        return out;
    }
    for slug in slugs {
        out.extend(project_group(app, &slug, t, cx));
    }
    out
}

/// A project: its header, then — unless it is folded away — its goals and its
/// sessions, newest first.
fn project_group(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    let mut out = vec![repo_header(app, slug, t, cx).into_any_element()];
    if app.ui.collapsed.contains(slug) {
        return out;
    }
    out.extend(
        goals_of(app, slug)
            .into_iter()
            .map(|summary| goal_row(slug, summary, t, cx).into_any_element()),
    );
    let rows = sessions_of(app, slug);
    let key = format!("repo:{slug}");
    let shown = match app.ui.expanded.contains(&key) {
        true => rows.len(),
        false => PER_PROJECT,
    };
    out.extend(
        rows.iter()
            .take(shown)
            .map(|open| session_row(app, open, t, cx).into_any_element()),
    );
    if rows.len() > shown {
        out.push(show_more(key, t, cx));
    }
    out
}

/// The project's own row. Clicking it folds its sessions away; the two buttons
/// that appear under the cursor open the project page and start a thread.
fn repo_header(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let group = SharedString::from(format!("repo-{slug}"));
    let key = slug.to_string();
    row_shell(group.clone(), false, t)
        .group(group.clone())
        .text_color(t.fg)
        .child(icon("folder", ICON, t.muted_fg))
        .child(div().flex_1().min_w_0().truncate().child(slug.to_string()))
        .child(open_project_button(slug, group.clone(), t, cx))
        .child(new_session_button(app, slug, group, t, cx))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.ui.collapsed.remove(&key) {
                this.ui.collapsed.insert(key.clone());
            }
            cx.notify();
        }))
}

/// An icon button that is only there while the cursor is on `group`.
fn hover_button(id: String, name: &str, group: SharedString, t: &Tokens) -> gpui::Stateful<Div> {
    icon_button(SharedString::from(id), name, true, t)
        .opacity(0.)
        .group_hover(group, |this| this.opacity(1.))
}

/// The `…`: the project's own page, the one place its tasks and goals are.
fn open_project_button(
    slug: &str,
    group: SharedString,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let open = slug.to_string();
    hover_button(format!("open-project-{slug}"), "ellipsis", group, t).on_click(cx.listener(
        move |this, _: &ClickEvent, _, cx| {
            // The row underneath would fold the project away otherwise.
            cx.stop_propagation();
            this.open_project(open.clone(), None, cx);
        },
    ))
}

/// The pencil: a new thread in this project, off the row's first clone URL.
fn new_session_button(
    app: &LgtmApp,
    slug: &str,
    group: SharedString,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let repository = app
        .known_repositories()
        .into_iter()
        .find(|url| repo_slug(url) == slug);
    hover_button(format!("new-session-{slug}"), "square-pen", group, t).on_click(cx.listener(
        move |this, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
            if let Some(repository) = repository.clone() {
                this.start_session(repository, cx);
            }
        },
    ))
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

fn session_row(
    app: &LgtmApp,
    session: &Session,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = session.id.clone();
    let open = id.clone();
    let active = app.selected.is_none() && app.page == Page::Session(id.clone());
    row_shell(SharedString::from(format!("session-{id}")), active, t)
        .pl(px(NEST))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(session_title(session)),
        )
        .when_some(
            crate::app::owner_label(app.owner_name(session.created_by.as_deref()), t),
            |this, owner| this.child(owner),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.show_page(Page::Session(open.clone()), cx)
        }))
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
        let n = app.runners.len();
        format!("Connected · {n} runner{}", if n == 1 { "" } else { "s" })
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
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_runner_settings(cx))),
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_runner_settings(cx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(title: &str) -> Session {
        Session {
            id: "s1".into(),
            repository: "https://x/one.git".into(),
            base_branch: "main".into(),
            title: title.into(),
            created_at: 0,
            workspace: None,
            created_by: None,
        }
    }

    #[test]
    fn a_row_names_an_unsent_thread_and_cuts_a_long_one() {
        assert_eq!(session_title(&session("")), "New session");
        assert_eq!(session_title(&session("   ")), "New session");
        assert_eq!(session_title(&session("fix the parser")), "fix the parser");
        let long = "x".repeat(PROMPT_PREVIEW + 1);
        // The cut adds an ellipsis in place of what it dropped.
        assert_eq!(
            session_title(&session(&long)).chars().count(),
            PROMPT_PREVIEW + 1
        );
    }
}
