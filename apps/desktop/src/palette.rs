//! ⌘K: one input over everything, with tasks, repositories and actions under it.

use crate::app::{LgtmApp, Page};
use crate::labels::{prompt_preview, status_label};
use crate::sidebar::repo_slug;
use crate::theme::{
    panel, scrim, section_label, tokens, Pref, Tokens, ROW_H, SPACE, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, relative, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::input::Input;
use gpui_component::Sizable as _;
use lgtm_protocol::Task;

const WIDTH: f32 = 560.;
const MAX_LIST_H: f32 = 380.;
const PROMPT_PREVIEW: usize = 60;
const FOOTER: &str = "↑↓ navigate · ↩ open · esc close";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    NewTask,
    Batches,
    Settings,
    ToggleTheme,
    ToggleSidebar,
}

impl Act {
    const ALL: [(Act, &'static str); 5] = [
        (Act::NewTask, "New task"),
        (Act::Batches, "Batches"),
        (Act::Settings, "Settings"),
        (Act::ToggleTheme, "Toggle theme"),
        (Act::ToggleSidebar, "Toggle sidebar"),
    ];
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Task(String),
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

pub fn build_groups(query: &str, tasks: &[Task], repos: &[String]) -> Vec<Group> {
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
    let repo_items = repos
        .iter()
        .map(|url| (Kind::Repository(url.clone()), repo_slug(url), String::new()));
    let action_items = Act::ALL
        .iter()
        .map(|(act, label)| (Kind::Action(*act), (*label).to_string(), String::new()));
    [
        group("Tasks", task_items, query),
        group("Repositories", repo_items, query),
        group("Actions", action_items, query),
    ]
    .into_iter()
    .filter(|group| !group.items.is_empty())
    .collect()
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
    let query = app.query.read(cx).value().to_string();
    build_groups(&query, &app.tasks, &app.known_repositories())
        .into_iter()
        .flat_map(|group| group.items)
        .collect()
}

pub fn step(app: &mut LgtmApp, delta: isize, cx: &mut Context<LgtmApp>) {
    let count = items(app, cx).len() as isize;
    if count == 0 {
        return;
    }
    app.palette_at = (app.palette_at as isize + delta).rem_euclid(count) as usize;
    cx.notify();
}

pub fn run(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) {
    let Some(item) = items(app, cx).into_iter().nth(app.palette_at) else {
        return;
    };
    activate(app, item.kind, window, cx);
}

fn activate(app: &mut LgtmApp, kind: Kind, window: &mut Window, cx: &mut Context<LgtmApp>) {
    app.close_overlay(window, cx);
    match kind {
        Kind::Task(id) => app.select(id, cx),
        Kind::Repository(url) => {
            app.project = Some(url);
            app.show_page(Page::Home, cx);
        }
        Kind::Action(Act::NewTask) => app.go_home(window, cx),
        Kind::Action(Act::Batches) => app.show_page(Page::Batches, cx),
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
    let at = app.palette_at;
    let query = app.query.read(cx).value().to_string();
    let groups = build_groups(&query, &app.tasks, &app.known_repositories());

    let rows = rows(groups, at, &t, cx);

    scrim("palette-scrim", &t)
        .pt(relative(0.2))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            panel(&t)
                .id("palette")
                .key_context("Palette")
                .w(px(WIDTH))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(SPACE[1]))
                        .py(px(SPACE[0]))
                        .border_b_1()
                        .border_color(t.border)
                        .child(Input::new(&app.query).appearance(false).large()),
                )
                .child(
                    div()
                        .id("palette-list")
                        .flex()
                        .flex_col()
                        .max_h(px(MAX_LIST_H))
                        .overflow_y_scroll()
                        .p(px(SPACE[0]))
                        .children(rows),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(ROW_H))
                        .px(px(SPACE[2]))
                        .border_t_1()
                        .border_color(t.border)
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.muted_fg)
                        .child(FOOTER),
                ),
        )
        .into_any_element()
}

/// Every group's label and rows, or "No matches" when there are none.
fn rows(groups: Vec<Group>, at: usize, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut index = 0;
    let mut rows: Vec<AnyElement> = Vec::new();
    for group in groups {
        rows.push(
            section_label(group.title, t)
                .px(px(SPACE[2]))
                .pt(px(SPACE[1]))
                .pb(px(SPACE[0]))
                .into_any_element(),
        );
        for item in group.items {
            rows.push(row(item, (index, index == at), t, cx).into_any_element());
            index += 1;
        }
    }
    if rows.is_empty() {
        rows.push(
            div()
                .px(px(SPACE[2]))
                .py(px(SPACE[2]))
                .text_color(t.muted_fg)
                .child("No matches")
                .into_any_element(),
        );
    }
    rows
}

fn row(
    item: Item,
    (index, active): (usize, bool),
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let kind = item.kind.clone();
    div()
        .id(SharedString::from(format!("palette-{index}")))
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(8.))
        .cursor_pointer()
        .when(active, |this| this.bg(t.muted))
        .hover(|this| this.bg(t.muted))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .children(highlight(&item.label, &item.matched, t)),
        )
        .when(!item.hint.is_empty(), |this| {
            this.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.muted_fg)
                    .child(item.hint.clone()),
            )
        })
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            activate(this, kind.clone(), window, cx)
        }))
}

/// The label, with the matched characters in bold.
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
        .flex_shrink_0()
        .when(hit, |this| {
            this.font_weight(FontWeight::BOLD).text_color(t.fg)
        })
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
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: vec![],
                batch: None,
            },
            status: TaskStatus::Running,
            worker: None,
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
        }
    }

    #[test]
    fn groups_drop_what_does_not_match_and_keep_the_rest() {
        let tasks = vec![
            task("1", "fix the parser", "https://x/one.git"),
            task("2", "ship the docs", "https://x/two.git"),
        ];
        let repos = vec!["https://x/one.git".to_string()];
        let groups = build_groups("fix", &tasks, &repos);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].title, "Tasks");
        assert_eq!(groups[0].items[0].kind, Kind::Task("1".into()));
        assert_eq!(groups[0].items[0].hint, "one · running");

        let all = build_groups("", &tasks, &repos);
        let titles: Vec<&str> = all.iter().map(|group| group.title).collect();
        assert_eq!(titles, vec!["Tasks", "Repositories", "Actions"]);
        assert_eq!(all[1].items[0].label, "one");
        assert_eq!(all[2].items.len(), Act::ALL.len());
    }
}
