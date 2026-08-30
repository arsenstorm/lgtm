//! The project page: one repository's overview, tasks, goals, and runners.

mod goals;
mod history;
mod lists;
mod overview;
mod plans;

use crate::app::LgtmApp;
use crate::tasks::repo_slug;
use crate::theme::{icon, tokens, Tokens, HEADER_H, ICON, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::{
    div, px, AnyElement, Context, Div, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::tab::{Tab, TabBar};
use lgtm_protocol::{GoalSummary, RunnerStatus, Task, TaskStatus};

/// Which section of the project page is showing.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum ProjectTab {
    #[default]
    Overview,
    Tasks,
    Goals,
    Plans,
    Memories,
    Todos,
    History,
    Runners,
}

impl ProjectTab {
    const ALL: [(ProjectTab, &'static str); 8] = [
        (ProjectTab::Overview, "Overview"),
        (ProjectTab::Tasks, "Tasks"),
        (ProjectTab::Goals, "Goals"),
        (ProjectTab::Plans, "Plans"),
        (ProjectTab::Memories, "Memories"),
        (ProjectTab::Todos, "TODOs"),
        (ProjectTab::History, "History"),
        (ProjectTab::Runners, "Runners"),
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

/// The three numbers the header reports: running, awaiting review, failed.
pub fn counts(tasks: &[&Task]) -> (usize, usize, usize) {
    let count =
        |wanted: fn(TaskStatus) -> bool| tasks.iter().filter(|task| wanted(task.status)).count();
    (
        count(|status| status == TaskStatus::Running),
        count(|status| status == TaskStatus::AwaitingReview),
        count(|status| {
            matches!(
                status,
                TaskStatus::Failed | TaskStatus::TimedOut | TaskStatus::RunnerLost
            )
        }),
    )
}

pub fn page(app: &LgtmApp, slug: &str, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let tab = app.ui.project_tab;
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(header(app, slug, &t))
        .child(
            div()
                .px(px(SPACE[1]))
                .pt(px(SPACE[1]))
                .child(tab_bar(tab, cx)),
        )
        .child(body(app, slug, tab, &t, cx))
        .into_any_element()
}

fn header(app: &LgtmApp, slug: &str, t: &Tokens) -> Div {
    let (running, review, failed) = counts(&tasks_of(app, slug));
    div()
        .flex()
        .flex_shrink_0()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .h(px(HEADER_H + SPACE[2]))
        .px(px(SPACE[2]))
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(icon("folder", ICON, t.muted_fg))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(slug.to_string()),
                ),
        )
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(format!(
                    "{running} running · {review} awaiting review · {failed} failed"
                )),
        )
}

fn tab_bar(tab: ProjectTab, cx: &mut Context<LgtmApp>) -> TabBar {
    let at = ProjectTab::ALL
        .iter()
        .position(|(one, _)| *one == tab)
        .unwrap_or(0);
    TabBar::new("project-tabs")
        .segmented()
        .text_size(px(TEXT_SECONDARY))
        .selected_index(at)
        .children(ProjectTab::ALL.map(|(_, label)| Tab::new().label(label)))
        .on_click(cx.listener(|this, index: &usize, _, cx| {
            if let Some((tab, _)) = ProjectTab::ALL.get(*index) {
                this.show_project_tab(*tab, cx);
            }
        }))
}

/// The scrolled section. Its children are the goal cards on the Goals tab, so
/// opening a goal from the sidebar can scroll straight to one by index.
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
        .p(px(SPACE[2]))
        .children(match tab {
            ProjectTab::Overview => overview::rows(app, slug, t, cx),
            ProjectTab::Tasks => task_rows(app, slug, t, cx),
            ProjectTab::Goals => goals::cards(app, slug, t, cx),
            ProjectTab::Plans => plans::rows(app, t, cx),
            ProjectTab::Memories => lists::memories(app, t, cx),
            ProjectTab::Todos => lists::todos(app, t, cx),
            ProjectTab::History => history::rows(app, slug, t, cx),
            ProjectTab::Runners => runner_rows(app, t),
        })
}

fn task_rows(app: &LgtmApp, slug: &str, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let rows = tasks_of(app, slug);
    if rows.is_empty() {
        return vec![muted("No tasks in this project yet.", t)];
    }
    rows.into_iter()
        .map(|task| crate::batches::task_row(app, task, t, cx).into_any_element())
        .collect()
}

fn runner_rows(app: &LgtmApp, t: &Tokens) -> Vec<AnyElement> {
    if app.runners.is_empty() {
        return vec![muted("No runners connected.", t)];
    }
    app.runners
        .iter()
        .map(|runner| runner_row(runner, t).into_any_element())
        .collect()
}

/// Name, platform, slots, and what the machine can run.
fn runner_row(runner: &RunnerStatus, t: &Tokens) -> Div {
    let info = &runner.info;
    let cell = |text: String| {
        div()
            .w(px(120.))
            .flex_shrink_0()
            .text_size(px(TEXT_SECONDARY))
            .text_color(t.muted_fg)
            .child(text)
    };
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .p(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(
            div()
                .w(px(160.))
                .flex_shrink_0()
                .truncate()
                .child(info.name.clone()),
        )
        .child(cell(format!("{}/{}", info.os, info.arch)))
        .child(cell(format!(
            "{}/{} slots",
            runner.running.len(),
            info.slots
        )))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(info.capabilities.join(" · ")),
        )
}

fn muted(text: &'static str, t: &Tokens) -> AnyElement {
    div().text_color(t.muted_fg).child(text).into_any_element()
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
