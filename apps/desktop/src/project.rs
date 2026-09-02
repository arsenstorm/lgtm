//! The project page: one repository at a glance, the work under way in it,
//! and what every run there is told.

mod goals;
mod lists;
mod overview;
mod work;

use crate::app::LgtmApp;
use crate::tasks::repo_slug;
use crate::theme::{icon, tokens, Header, Tokens, ICON, SPACE, TEXT_SECONDARY};
use gpui::{
    div, px, AnyElement, Context, Div, InteractiveElement as _, IntoElement, ParentElement as _,
    Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::tab::{Tab, TabBar};
use lgtm_protocol::{GoalSummary, Task};

/// Which section of the project page is showing.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectTab {
    #[default]
    Overview,
    Work,
    /// What every run in this project is told: its memories.
    Context,
}

impl ProjectTab {
    const ALL: [(ProjectTab, &'static str); 3] = [
        (ProjectTab::Overview, "Overview"),
        (ProjectTab::Work, "Work"),
        (ProjectTab::Context, "Context"),
    ];
}

/// The clone URL the sidebar's slug stands for. Memories and todos are keyed
/// by URL, and only the slug reaches the page.
pub fn repository_of(app: &LgtmApp, slug: &str) -> Option<String> {
    app.tasks
        .iter()
        .map(|task| &task.spec.repository)
        .chain(app.sessions.iter().map(|session| &session.repository))
        .chain(app.goals.iter().map(|summary| &summary.goal.repository))
        .find(|url| repo_slug(url) == slug)
        .cloned()
}

/// Tasks in this project, in the order the app holds them (newest first).
pub fn tasks_of<'a>(app: &'a LgtmApp, slug: &str) -> Vec<&'a Task> {
    app.tasks
        .iter()
        .filter(|task| repo_slug(&task.spec.repository) == slug)
        .collect()
}

pub fn goals_of<'a>(app: &'a LgtmApp, slug: &str) -> Vec<&'a GoalSummary> {
    app.goals
        .iter()
        .filter(|summary| repo_slug(&summary.goal.repository) == slug)
        .collect()
}

pub fn page(app: &LgtmApp, slug: &str, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let tab = app.ui.project_tab;
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().px(px(SPACE[1])).child(tab_bar(tab, cx)))
        .child(body(app, slug, tab, &t, cx))
        .into_any_element()
}

pub(crate) fn project_header(slug: &str, t: &Tokens) -> Div {
    Header::new(slug.to_string())
        .leading(icon("folder", ICON, t.muted_fg))
        .render()
}

fn tab_bar(tab: ProjectTab, cx: &mut Context<LgtmApp>) -> TabBar {
    let at = ProjectTab::ALL
        .iter()
        .position(|(one, _)| *one == tab)
        .unwrap_or(0);
    TabBar::new("project-tabs")
        .underline()
        .text_size(px(TEXT_SECONDARY))
        .selected_index(at)
        .children(ProjectTab::ALL.map(|(_, label)| Tab::new().label(label)))
        .on_click(cx.listener(|this, index: &usize, _, cx| {
            if let Some((tab, _)) = ProjectTab::ALL.get(*index) {
                this.show_project_tab(*tab, cx);
            }
        }))
}

/// The scrolled section. The goal cards are its first children on the Work
/// tab, so opening a goal from the sidebar can scroll straight to one by index.
fn body(
    app: &LgtmApp,
    slug: &str,
    tab: ProjectTab,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Stateful<Div> {
    div()
        .id("project-body")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&app.ui.project_scroll)
        .flex()
        .flex_col()
        .gap(px(SPACE[2]))
        .px(px(SPACE[1]))
        .py(px(SPACE[2]))
        .children(match tab {
            ProjectTab::Overview => overview::rows(app, slug, t, cx),
            ProjectTab::Work => work::rows(app, slug, t, cx),
            ProjectTab::Context => lists::memories(app, t, cx),
        })
}

fn muted(text: &'static str, t: &Tokens) -> AnyElement {
    div().text_color(t.muted_fg).child(text).into_any_element()
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
