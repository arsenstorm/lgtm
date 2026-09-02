//! ⌘K: one input over everything, with tasks, repositories and actions under it.

use crate::app::{LgtmApp, Page};
use crate::labels::{prompt_preview, status_label};
use crate::tasks::repo_slug;
use crate::theme::{
    icon, tokens, Pref, Tokens, ICON, RADIUS, RADIUS_PILL, SPACE, TEXT_ROW, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::input::Input;
use gpui_component::Sizable as _;
use lgtm_protocol::{Session, Task};

// Wide and tall enough that a real workspace's groups read without feeling
// boxed in; the list still scrolls past this rather than growing forever.
const WIDTH: f32 = 640.;
const MAX_LIST_H: f32 = 520.;
/// One palette row. Compact, the way a menu is: a row is one line of text.
const ROW: f32 = 32.;
/// The palette is a floating surface, and rounder than the cards in the app.
const PANEL_RADIUS: f32 = 18.;
/// The row's own inset from the panel edge, so a highlighted row reads as a
/// menu selection rather than a button.
const INSET: f32 = SPACE[0];
/// Every row's text starts here: the same inset the input carries.
const GUTTER: f32 = SPACE[2];
const PROMPT_PREVIEW: usize = 60;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    NewTask,
    Work,
    Inbox,
    Settings,
    ToggleTheme,
    ToggleSidebar,
}

impl Act {
    const ALL: [(Act, &'static str); 6] = [
        (Act::NewTask, "New task"),
        (Act::Work, "Work"),
        (Act::Inbox, "Inbox"),
        (Act::Settings, "Settings"),
        (Act::ToggleTheme, "Toggle theme"),
        (Act::ToggleSidebar, "Toggle sidebar"),
    ];

    fn icon(self) -> &'static str {
        match self {
            Act::NewTask => "square-pen",
            Act::Work => "list-checks",
            Act::Inbox => "inbox",
            Act::Settings => "settings",
            Act::ToggleTheme => "palette",
            Act::ToggleSidebar => "panel-left",
        }
    }

    /// The binding that reaches it without opening the palette, if there is
    /// one. Shown as a key badge, the way a menu shows its shortcut.
    fn keys(self) -> &'static str {
        match self {
            Act::NewTask => "⌘N",
            Act::ToggleSidebar => "⌘B",
            _ => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Task(String),
    /// A chat thread, by session id.
    Session(String),
    /// A repository slug, as the project page keys them.
    Project(String),
    Repository(String),
    Action(Act),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub label: String,
    pub hint: String,
    /// Character positions in `label` the query matched.
    pub matched: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub title: &'static str,
    pub items: Vec<Item>,
}

/// Case-insensitive subsequence match, returning where each query character
/// landed. An empty query matches everything, at no position.
pub fn fuzzy(query: &str, text: &str) -> Option<Vec<usize>> {
    let mut wanted = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .peekable();
    let mut matched = Vec::new();
    for (index, ch) in text.chars().enumerate() {
        let Some(next) = wanted.peek() else { break };
        if ch.to_lowercase().eq(std::iter::once(*next)) {
            matched.push(index);
            wanted.next();
        }
    }
    wanted.next().is_none().then_some(matched)
}

fn item(kind: Kind, label: String, hint: String, query: &str) -> Option<Item> {
    let matched = fuzzy(query, &label)?;
    Some(Item {
        kind,
        label,
        hint,
        matched,
    })
}

pub fn build_groups(
    query: &str,
    tasks: &[Task],
    repos: &[String],
    sessions: &[Session],
) -> Vec<Group> {
    let session_items = sessions.iter().map(|open| {
        // The sidebar drops archived threads, so the palette is the only place
        // left that finds one — and has to say that is what it found.
        let hint = match open.archived {
            true => format!("{} · archived", repo_slug(&open.repository)),
            false => repo_slug(&open.repository),
        };
        (
            Kind::Session(open.id.clone()),
            crate::sidebar::session_title(open),
            hint,
        )
    });
    let task_items = tasks.iter().map(|task| {
        let hint = format!(
            "{} · {}",
            repo_slug(&task.spec.repository),
            status_label(task, tasks)
        );
        (
            Kind::Task(task.id.clone()),
            prompt_preview(&task.spec.prompt, PROMPT_PREVIEW),
            hint,
        )
    });
    let project_items = project_slugs(repos).into_iter().map(|slug| {
        (
            Kind::Project(slug.clone()),
            format!("Open project {slug}"),
            String::new(),
        )
    });
    let repo_items = repos
        .iter()
        .map(|url| (Kind::Repository(url.clone()), repo_slug(url), String::new()));
    let action_items = Act::ALL
        .iter()
        .map(|(act, label)| (Kind::Action(*act), (*label).to_string(), String::new()));
    [
        group("Threads", session_items, query),
        group("Tasks", task_items, query),
        group("Projects", project_items, query),
        group("Repositories", repo_items, query),
        group("Actions", action_items, query),
    ]
    .into_iter()
    .filter(|group| !group.items.is_empty())
    .collect()
}

/// One slug per project, first seen first — two clone URLs can share one.
fn project_slugs(repos: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for slug in repos.iter().map(|url| repo_slug(url)) {
        if !out.contains(&slug) {
            out.push(slug);
        }
    }
    out
}

/// The entries of `candidates` (kind, label, hint) that match `query`.
fn group(
    title: &'static str,
    candidates: impl Iterator<Item = (Kind, String, String)>,
    query: &str,
) -> Group {
    let items = candidates
        .filter_map(|(kind, label, hint)| item(kind, label, hint, query))
        .collect();
    Group { title, items }
}

fn items(app: &LgtmApp, cx: &Context<LgtmApp>) -> Vec<Item> {
    let query = app.inputs.query.read(cx).value().to_string();
    build_groups(&query, &app.tasks, &app.known_repositories(), &app.sessions)
        .into_iter()
        .flat_map(|group| group.items)
        .collect()
}

pub fn step(app: &mut LgtmApp, delta: isize, cx: &mut Context<LgtmApp>) {
    let count = items(app, cx).len() as isize;
    if count == 0 {
        return;
    }
    app.ui.palette_at = (app.ui.palette_at as isize + delta).rem_euclid(count) as usize;
    cx.notify();
}

pub fn run(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) {
    let Some(item) = items(app, cx).into_iter().nth(app.ui.palette_at) else {
        return;
    };
    activate(app, item.kind, window, cx);
}

fn activate(app: &mut LgtmApp, kind: Kind, window: &mut Window, cx: &mut Context<LgtmApp>) {
    app.close_overlay(window, cx);
    match kind {
        Kind::Task(id) => app.select(id, cx),
        Kind::Session(id) => app.show_page(Page::Session(id), cx),
        Kind::Project(slug) => app.open_project(slug, None, cx),
        Kind::Repository(url) => {
            app.composer.project = Some(url);
            app.show_page(Page::Home, cx);
        }
        Kind::Action(Act::NewTask) => app.go_home(window, cx),
        Kind::Action(Act::Work) => app.show_page(Page::Work, cx),
        Kind::Action(Act::Inbox) => app.show_page(Page::Inbox, cx),
        Kind::Action(Act::Settings) => app.open_settings(cx),
        Kind::Action(Act::ToggleSidebar) => app.toggle_sidebar(cx),
        Kind::Action(Act::ToggleTheme) => {
            let next = if gpui_component::ActiveTheme::theme(&**cx).mode.is_dark() {
                Pref::Light
            } else {
                Pref::Dark
            };
            crate::theme::set_pref(next, window, cx);
        }
    }
}

pub fn view(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let at = app.ui.palette_at;
    let query = app.inputs.query.read(cx).value().to_string();
    let groups = build_groups(&query, &app.tasks, &app.known_repositories(), &app.sessions);
    let rows = rows(groups, at, &t, cx);

    // No dimming layer: the palette sits over the app the way a menu does,
    // and its own surface is what separates it.
    div()
        .id("palette-scrim")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            div()
                .id("palette")
                .key_context("Palette")
                .w(px(WIDTH))
                .flex()
                .flex_col()
                .overflow_hidden()
                .p(px(INSET))
                .rounded(px(PANEL_RADIUS))
                // The prompt input's surface: the palette is the same kind of
                // floating card, so it is made of the same two colours.
                .bg(t.composer.card)
                .border_1()
                .border_color(t.composer.edge)
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(search_box(app))
                .child(
                    div()
                        .id("palette-list")
                        .flex()
                        .flex_col()
                        .max_h(px(MAX_LIST_H))
                        .overflow_y_scroll()
                        .pb(px(SPACE[1]))
                        .children(rows),
                ),
        )
        .into_any_element()
}

/// Frameless: no box, no border, no icon. The placeholder is text on the
/// panel, and the caret is the only thing that says it is a field.
fn search_box(app: &LgtmApp) -> Div {
    div()
        .py(px(SPACE[1]))
        .child(Input::new(&app.inputs.query).appearance(false).large())
}

/// Every group's label and rows, or "No matches" when there are none.
fn rows(groups: Vec<Group>, at: usize, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut index = 0;
    let mut rows: Vec<AnyElement> = Vec::new();
    for group in groups {
        rows.push(heading(group.title, t).into_any_element());
        for item in group.items {
            rows.push(row(item, (index, index == at), t, cx).into_any_element());
            index += 1;
        }
    }
    if rows.is_empty() {
        rows.push(
            div()
                .px(px(GUTTER))
                .py(px(SPACE[2]))
                .text_color(t.sidebar_muted)
                .child("No matches")
                .into_any_element(),
        );
    }
    rows
}

/// A group's name. No rule under it, no weight of its own: it is a heading by
/// position and by being dimmer than the rows it heads.
fn heading(title: &'static str, t: &Tokens) -> Div {
    div()
        .flex_shrink_0()
        .px(px(GUTTER))
        .pt(px(SPACE[2]))
        .pb(px(SPACE[0]))
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.sidebar_muted)
        .child(title)
}

fn row(
    item: Item,
    (index, active): (usize, bool),
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let kind = item.kind.clone();
    let action = match item.kind {
        Kind::Action(act) => Some(act),
        _ => None,
    };
    div()
        .id(SharedString::from(format!("palette-{index}")))
        .flex()
        .items_center()
        .flex_shrink_0()
        .gap(px(SPACE[1]))
        .h(px(ROW))
        .px(px(GUTTER))
        .rounded(px(RADIUS))
        .cursor_pointer()
        .text_size(px(TEXT_ROW))
        // A wash rather than an opaque grey: one step up from the panel, the
        // only thing that marks the current row.
        .when(active, |this| this.bg(t.wash))
        .hover(|this| this.bg(t.wash))
        // An empty slot where a row has no icon, so every label starts on one
        // column whether or not the group carries icons.
        .child(match action {
            Some(act) => div()
                .flex_none()
                .child(icon(act.icon(), ICON, t.sidebar_fg)),
            None => div().flex_none().size(px(ICON)),
        })
        .child(label(&item, t))
        .when_some(action.map(Act::keys).filter(|keys| !keys.is_empty()), {
            |this, keys| this.child(badge(keys, t))
        })
        .when(!item.hint.is_empty(), |this| {
            this.child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.sidebar_muted)
                    .child(item.hint.clone()),
            )
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            activate(this, kind.clone(), window, cx)
        }))
}

/// The row's own words. With nothing typed there is one run of text, which is
/// what lets it end in an ellipsis rather than a hard clip.
fn label(item: &Item, t: &Tokens) -> Div {
    let base = div().flex_1().min_w_0().text_color(t.sidebar_fg);
    match item.matched.is_empty() {
        true => base.truncate().child(item.label.clone()),
        false => base
            .flex()
            .truncate()
            .children(highlight(&item.label, &item.matched, t)),
    }
}

/// A keyboard shortcut, as a key badge: one step off the panel, muted text.
fn badge(keys: &'static str, t: &Tokens) -> Div {
    div()
        .flex_none()
        .px(px(SPACE[1]))
        .py(px(1.))
        .rounded(px(RADIUS_PILL))
        .bg(t.wash)
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.sidebar_muted)
        .child(keys)
}

/// The label, with the matched characters at full strength. Colour rather
/// than weight: a bolder run would reflow the row it sits in.
fn highlight(label: &str, matched: &[usize], t: &Tokens) -> Vec<Div> {
    let mut out = Vec::new();
    let mut run = String::new();
    let mut run_hit = false;
    for (index, ch) in label.chars().enumerate() {
        let hit = matched.contains(&index);
        if hit != run_hit && !run.is_empty() {
            out.push(span(std::mem::take(&mut run), run_hit, t));
        }
        run_hit = hit;
        run.push(ch);
    }
    if !run.is_empty() {
        out.push(span(run, run_hit, t));
    }
    out
}

fn span(text: String, hit: bool, t: &Tokens) -> Div {
    div()
        .flex_none()
        .when(hit, |this| this.text_color(t.fg))
        .child(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec, TaskStatus};

    #[test]
    fn fuzzy_matches_a_case_insensitive_subsequence() {
        assert_eq!(fuzzy("fb", "FooBar"), Some(vec![0, 3]));
        assert_eq!(fuzzy("", "anything"), Some(vec![]));
        assert_eq!(fuzzy("zz", "FooBar"), None);
        assert_eq!(fuzzy("rab", "FooBar"), None);
    }

    #[test]
    fn fuzzy_ignores_spaces_in_the_query() {
        assert_eq!(fuzzy("f b", "FooBar"), Some(vec![0, 3]));
    }

    fn task(id: &str, prompt: &str, repo: &str) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: repo.into(),
                base_branch: "main".into(),
                prompt: prompt.into(),
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
                goal: None,
                review_executor: None,
                model: None,
                allowed_hosts: Vec::new(),
                session: None,
                created_by: None,
            },
            status: TaskStatus::Running,
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

    #[test]
    fn groups_drop_what_does_not_match_and_keep_the_rest() {
        let tasks = vec![
            task("1", "fix the parser", "https://x/one.git"),
            task("2", "ship the docs", "https://x/two.git"),
        ];
        let repos = vec!["https://x/one.git".to_string()];
        let groups = build_groups("fix", &tasks, &repos, &[]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].title, "Tasks");
        assert_eq!(groups[0].items[0].kind, Kind::Task("1".into()));
        // The wording is `status_label`'s to choose; the palette only has to
        // read the same words as the task cards.
        assert_eq!(
            groups[0].items[0].hint,
            format!("one · {}", status_label(&tasks[0], &tasks))
        );

        let all = build_groups("", &tasks, &repos, &[]);
        let titles: Vec<&str> = all.iter().map(|group| group.title).collect();
        assert_eq!(titles, vec!["Tasks", "Projects", "Repositories", "Actions"]);
        assert_eq!(all[1].items[0].kind, Kind::Project("one".into()));
        assert_eq!(all[1].items[0].label, "Open project one");
        assert_eq!(all[2].items[0].label, "one");
        assert_eq!(all[3].items.len(), Act::ALL.len());
    }

    #[test]
    fn a_session_row_is_titled_by_the_session_alone() {
        let session = Session {
            id: "s1".into(),
            repository: "https://x/one.git".into(),
            base_branch: "main".into(),
            title: "fix the parser".into(),
            created_at: 0,
            workspace: None,
            created_by: None,
            archived: false,
        };
        let groups = build_groups("", &[], &[], std::slice::from_ref(&session));
        assert_eq!(groups[0].title, "Threads");
        assert_eq!(groups[0].items[0].label, "fix the parser");
        assert_eq!(groups[0].items[0].hint, "one");

        // Still findable once archived, and saying so.
        let archived = Session {
            archived: true,
            ..session
        };
        let groups = build_groups("", &[], &[], std::slice::from_ref(&archived));
        assert_eq!(groups[0].items[0].hint, "one · archived");
    }
}
