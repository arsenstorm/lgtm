//! The left rail: quick actions, sessions grouped by repository, and one
//! status row that opens Settings.

use crate::app::{LgtmApp, Overlay, Page};
use crate::labels::prompt_preview;
use crate::menu::Target;
use crate::project::goals_of;
use crate::tasks::{goal_color, repo_slug};
use crate::theme::{
    icon, tokens, Tokens, GLYPH, ICON, RADIUS, ROW_H, ROW_RADIUS, SPACE, TEXT_BODY, TEXT_ROW,
    TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    deferred, div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use lgtm_protocol::{GoalSummary, Session};

pub const WIDTH: f32 = 240.;
const PROMPT_PREVIEW: usize = 34;
/// The row carrying the product name.
const BRAND_H: f32 = 36.;
/// How far a session sits under its project: flush with the label after the
/// project's folder icon, the way Codex nests its task rows.
const NEST: f32 = 32.;
/// Sessions shown per project before `Show more`.
const PER_PROJECT: usize = 5;
/// The circle around the connection dot: the size of an icon, so the dot
/// holds the same column as the folder icons above it.
const STATUS_DOT: f32 = 16.;
/// The runner popover spans the footer row.
const POPOVER_W: f32 = WIDTH - 2. * SPACE[1];

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
        .child(projects_header(&t, cx))
        .child(
            div()
                .id("tasks")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.ui.task_scroll)
                .flex()
                .flex_col()
                .gap(px(2.))
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
        .gap(px(SPACE[1]))
        // Flush with the nav icons below: their container and row padding. The
        // mark takes that column, so the name lands on the nav labels.
        .pl(px(SPACE[3]))
        .pr(px(SPACE[1]))
        .child(icon("lgtm", ICON, t.sidebar_fg))
        .child(
            div()
                .flex_1()
                .text_size(px(TEXT_BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.sidebar_fg)
                .child("LGTM"),
        )
        .child(search_button(!searching, t).on_click(
            cx.listener(|this, _: &ClickEvent, window, cx| this.open_palette(window, cx)),
        ))
}

/// The search icon a hair inside the standard glyph.
const SEARCH_ICON: f32 = 14.;

/// The palette trigger: quieter and smaller than a standard icon button, so
/// the full-strength icon pops on hover instead of merely brightening.
fn search_button(enabled: bool, t: &Tokens) -> gpui::Stateful<Div> {
    let group = SharedString::from("search");
    div()
        .id(group.clone())
        .group(group.clone())
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(GLYPH))
        .h(px(GLYPH))
        .rounded(px(ROW_RADIUS))
        .when(enabled, |this| {
            this.cursor_pointer().hover(|this| this.bg(t.wash))
        })
        .child(
            gpui::svg()
                .path("icons/search.svg")
                .flex_none()
                .size(px(SEARCH_ICON))
                .text_color(if enabled { t.sidebar_muted } else { t.border })
                .when(enabled, |this| {
                    this.group_hover(group, |this| this.text_color(t.fg))
                }),
        )
}

fn nav(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let work = app.selected.is_none() && app.page == Page::Work;
    let inbox = app.selected.is_none() && app.page == Page::Inbox;
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .px(px(SPACE[1]))
        .gap(px(2.))
        .child(
            // Never highlighted: it is a door into the composer, not a place
            // the window can be.
            nav_row(&NEW_TASK, false, t)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.go_home(window, cx))),
        )
        .child(
            nav_row(&WORK, work, t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.show_page(Page::Work, cx)),
            ),
        )
        .child(
            nav_row(&INBOX, inbox, t).on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.show_page(Page::Inbox, cx)),
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
        // One grey for every row, active or not: the rail reads as one list,
        // and the pill alone marks the current row.
        .text_color(t.sidebar_fg)
        // The wash, not the opaque grey: these rows sit on the alpha sidebar,
        // and a solid fill would break the blur behind it.
        .when(active, |this| this.bg(t.wash))
        .hover(|this| this.bg(t.wash))
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
const WORK: NavItem = NavItem {
    id: "work",
    icon: "list-checks",
    label: "Work",
};
const INBOX: NavItem = NavItem {
    id: "inbox",
    icon: "inbox",
    label: "Inbox",
};

fn nav_row(item: &NavItem, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    row_shell(item.id, active, t)
        .child(icon(item.icon, ICON, t.sidebar_fg))
        .child(div().flex_1().min_w_0().truncate().child(item.label))
}

/// Every project the orchestrator knows, most recently touched first: the
/// sessions name most of them, and a repository with only tasks still counts.
pub(crate) fn project_slugs(app: &LgtmApp) -> Vec<String> {
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

/// One project's threads. An archived thread is gone from the rail; the
/// palette is where it stays findable.
fn sessions_of<'a>(sessions: &'a [Session], slug: &str) -> Vec<&'a Session> {
    sessions
        .iter()
        .filter(|open| !open.archived && repo_slug(&open.repository) == slug)
        .collect()
}

/// A session's row label: its first message, or what an unused thread is
/// called until one is sent.
pub fn session_title(session: &Session) -> String {
    match session.title.trim().is_empty() {
        true => "New task".to_string(),
        false => prompt_preview(&session.title, PROMPT_PREVIEW),
    }
}

/// The Projects heading: its label, and — under the cursor — the list's own
/// controls: fold options behind the ellipsis, a new project behind the plus.
fn projects_header(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let group = SharedString::from("projects-header");
    div()
        .flex_shrink_0()
        .px(px(SPACE[1]))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[0]))
                .px(px(SPACE[1]))
                // Codex's rhythm: clear air above a section, a little below.
                .pt(px(SPACE[3]))
                .pb(px(SPACE[0]))
                .group(group.clone())
                .child(
                    div()
                        .flex_1()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.sidebar_muted)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Projects"),
                )
                .child(
                    hover_button("projects-options".into(), "ellipsis", group.clone(), t).on_click(
                        cx.listener(|this, event: &ClickEvent, window, cx| {
                            this.open_menu(Target::Projects, event.position(), window, cx)
                        }),
                    ),
                )
                .child(
                    hover_button("projects-add".into(), "plus", group, t).on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| this.open_add_project(window, cx),
                    )),
                ),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                this.open_menu(Target::Projects, event.position, window, cx)
            }),
        )
}

/// A click anywhere else closes the menu above it. It reaches out past its
/// own bounds, so the nearest positioned ancestor is all it needs.
fn dismiss(id: impl Into<SharedString>, cx: &mut Context<LgtmApp>) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .absolute()
        .top(px(-4000.))
        .left(px(-4000.))
        .w(px(8000.))
        .h(px(8000.))
        .occlude()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_menus(cx)))
}

fn repository_groups(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let slugs = project_slugs(app);
    if slugs.is_empty() {
        out.push(
            div()
                .px(px(SPACE[1]))
                .py(px(SPACE[0]))
                .text_size(px(TEXT_ROW))
                .text_color(t.sidebar_muted)
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
    let rows = sessions_of(&app.sessions, slug);
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

/// The project's own row. Clicking it folds its sessions away; the `…` and a
/// right-click open everything else it can do.
fn repo_header(
    app: &LgtmApp,
    slug: &str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let group = SharedString::from(format!("repo-{slug}"));
    let (key, target) = (slug.to_string(), slug.to_string());
    row_shell(group.clone(), false, t)
        .group(group.clone())
        // Already normal weight, so the project rows step down a pixel
        // instead to sit under the section label.
        .text_size(px(TEXT_ROW - 1.))
        .child(icon("folder", ICON, t.sidebar_fg))
        .child(div().flex_1().min_w_0().truncate().child(slug.to_string()))
        .child(project_menu_button(slug, group.clone(), t, cx))
        .child(new_session_button(app, slug, group, t, cx))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.open_menu(Target::Project(target.clone()), event.position, window, cx)
            }),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if !this.ui.collapsed.remove(&key) {
                this.ui.collapsed.insert(key.clone());
            }
            cx.notify();
        }))
}

/// An icon button that is only there while the cursor is on `group`.
fn hover_button(id: String, name: &str, group: SharedString, t: &Tokens) -> gpui::Stateful<Div> {
    let own = SharedString::from(id);
    div()
        .id(own.clone())
        .group(own.clone())
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(GLYPH))
        .h(px(GLYPH))
        .cursor_pointer()
        .opacity(0.)
        .group_hover(group, |this| this.opacity(1.))
        // No fill of its own: the icon alone goes white under the cursor.
        .child(
            gpui::svg()
                .path(format!("icons/{name}.svg"))
                .flex_none()
                .size(px(ICON))
                .text_color(t.sidebar_muted)
                .group_hover(own, |this| this.text_color(t.fg)),
        )
}

/// The `…`: the same menu a right-click opens, for a pointer that has none.
fn project_menu_button(
    slug: &str,
    group: SharedString,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let target = slug.to_string();
    hover_button(format!("open-project-{slug}"), "ellipsis", group, t).on_click(cx.listener(
        move |this, event: &ClickEvent, window, cx| {
            // The row underneath would fold the project away otherwise.
            cx.stop_propagation();
            this.open_menu(
                Target::Project(target.clone()),
                event.position(),
                window,
                cx,
            );
        },
    ))
}

/// The pencil: the composer, with this project already chosen. Starting an
/// empty thread here would list a "New task" row nobody asked for.
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
        move |this, _: &ClickEvent, window, cx| {
            cx.stop_propagation();
            this.composer.project = repository.clone();
            this.go_home(window, cx);
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
        .text_color(t.sidebar_muted)
        .child("Show more")
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.ui.expanded.insert(key.clone());
            cx.notify();
        }))
        .into_any_element()
}

/// A thread under its project. Everything it can do is a right-click away.
fn session_row(
    app: &LgtmApp,
    session: &Session,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = session.id.clone();
    let (open, target) = (id.clone(), id.clone());
    let group = SharedString::from(format!("session-{id}"));
    let active = app.selected.is_none() && app.page == Page::Session(id.clone());
    row_shell(group.clone(), active, t)
        .group(group)
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
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.open_menu(Target::Thread(target.clone()), event.position, window, cx)
            }),
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

/// One row: whether the orchestrator answered and its runners, and Settings.
fn footer(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
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
        .relative()
        .flex_shrink_0()
        .border_t_1()
        .border_color(t.sidebar_border)
        .px(px(SPACE[1]))
        .py(px(SPACE[1]))
        .when(app.ui.runner_menu, |this| {
            this.child(deferred(dismiss("runner-dismiss", cx)))
                .child(deferred(popover(app, t, cx)).with_priority(1))
        })
        .child(
            // Two controls, not one: the runner status opens its popover, the
            // gear opens Settings, and neither sits inside the other's row.
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[0]))
                .child(
                    row_shell("status", false, t)
                        .flex_1()
                        .min_w_0()
                        .child(status_dot(tone))
                        .child(div().flex_1().min_w_0().truncate().child(status))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            let open = !this.ui.runner_menu;
                            this.close_menus(cx);
                            this.ui.runner_menu = open;
                            cx.notify();
                        })),
                )
                .child(
                    footer_settings(t).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| this.open_settings(cx)),
                    ),
                ),
        )
}

/// The footer's gear: the standard icon button squared up to the status row
/// beside it — same height, same corner radius.
fn footer_settings(t: &Tokens) -> gpui::Stateful<Div> {
    let group = SharedString::from("open-settings");
    div()
        .id(group.clone())
        .group(group.clone())
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(ROW_H))
        .h(px(ROW_H))
        .rounded(px(ROW_RADIUS))
        .cursor_pointer()
        .hover(|this| this.bg(t.wash))
        .child(
            gpui::svg()
                .path("icons/settings.svg")
                .flex_none()
                .size(px(ICON))
                .text_color(t.sidebar_muted)
                .group_hover(group, |this| this.text_color(t.fg)),
        )
}

/// One row per runner: what it is called, how loaded it is, what it runs on.
fn popover(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .absolute()
        .bottom(px(ROW_H + 2. * SPACE[0]))
        .left(px(SPACE[1]))
        .w(px(POPOVER_W))
        .flex()
        .flex_col()
        .p(px(SPACE[0]))
        .rounded(px(RADIUS))
        .bg(t.popover)
        .border_1()
        .border_color(t.border)
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .occlude()
        .when(app.runners.is_empty(), |this| {
            this.child(div().px(px(SPACE[1])).py(px(SPACE[0])).child("No runners"))
        })
        .children(app.runners.iter().map(|runner| {
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .h(px(ROW_H))
                .px(px(SPACE[1]))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(t.fg)
                        .child(runner.info.name.clone()),
                )
                .child(format!("{}/{}", runner.running.len(), runner.info.slots))
                .child(runner.info.os.clone())
        }))
        .child(div().my(px(SPACE[0])).h(px(1.)).bg(t.border))
        .child(
            div()
                .id("add-runner")
                .flex()
                .items_center()
                .h(px(ROW_H))
                .px(px(SPACE[1]))
                .rounded(px(ROW_RADIUS))
                .cursor_pointer()
                .text_color(t.sidebar_fg)
                .hover(|this| this.bg(t.wash))
                .child("Add a machine")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.open_runner_settings(cx))),
        )
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
            archived: false,
        }
    }

    #[test]
    fn a_row_names_an_unsent_thread_and_cuts_a_long_one() {
        assert_eq!(session_title(&session("")), "New task");
        assert_eq!(session_title(&session("   ")), "New task");
        assert_eq!(session_title(&session("fix the parser")), "fix the parser");
        let long = "x".repeat(PROMPT_PREVIEW + 1);
        // The cut adds an ellipsis in place of what it dropped.
        assert_eq!(
            session_title(&session(&long)).chars().count(),
            PROMPT_PREVIEW + 1
        );
    }

    #[test]
    fn the_rail_lists_a_projects_live_threads_only() {
        let mut archived = session("old");
        archived.id = "s2".into();
        archived.archived = true;
        let other = Session {
            id: "s3".into(),
            repository: "https://x/two.git".into(),
            ..session("elsewhere")
        };
        let all = vec![session("live"), archived, other];
        let rows = sessions_of(&all, "one");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "s1");
    }
}
