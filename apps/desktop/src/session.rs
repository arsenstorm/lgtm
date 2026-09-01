//! The session page: one chat thread, a turn per message, with the composer
//! pinned under it.

mod card;

use crate::app::{LgtmApp, Page};
use crate::tasks::{now_ms, repo_slug};
use crate::theme::{
    icon, icon_button, tokens, Header, Tokens, BAR_H, ICON, RADIUS, ROW_H, SPACE, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    deferred, div, px, AnyElement, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, Stateful, StatefulInteractiveElement as _, Styled as _, Window,
};
use lgtm_protocol::{StoredEvent, Task, TaskEvent};

/// The thread's own column, so a turn never runs the width of the window.
const COLUMN: f32 = 752.;
/// How far a user bubble stops short of the full column.
const BUBBLE_MAX: f32 = 560.;

/// One thing the thread shows.
#[derive(Debug, PartialEq)]
pub enum Turn<'a> {
    /// The message that started a task.
    User(&'a str),
    Task(&'a Task),
    /// Something the orchestration model decided, in its own words.
    Assistant(&'a str),
}

/// The thread in order: each task's message, the task, then whatever the
/// orchestrator said about it. Tasks whose events have not been fetched
/// contribute no assistant turns.
pub fn turns<'a>(tasks: &'a [Task], events: &'a [(String, Vec<StoredEvent>)]) -> Vec<Turn<'a>> {
    let mut out = Vec::new();
    for task in tasks {
        out.push(Turn::User(&task.spec.prompt));
        out.push(Turn::Task(task));
        out.extend(reasons(events, &task.id).map(Turn::Assistant));
    }
    out
}

fn reasons<'a>(
    events: &'a [(String, Vec<StoredEvent>)],
    id: &str,
) -> impl Iterator<Item = &'a str> {
    events
        .iter()
        .find(|(held, _)| held == id)
        .map(|(_, events)| events.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|stored| match &stored.event {
            TaskEvent::Orchestrated { reason, .. } => Some(reason.as_str()),
            _ => None,
        })
}

pub fn page(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(thread(app, &t, cx))
        .child(crate::composer::composer(app, &t, window, cx))
        .into_any_element()
}

/// What the thread is about, seated in the window bar. The folder owns the
/// project context instead of spelling it out as a second header row.
pub(crate) fn session_header(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let Some(open) = &app.session else {
        return div().h_full().flex_1();
    };
    let title = match open.session.title.trim().is_empty() {
        true => "New session".to_string(),
        false => open.session.title.clone(),
    };
    Header::new(title)
        .leading(
            icon_button("session-project", "folder", true, t)
                .occlude()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let open = !this.ui.session_project_menu;
                    this.close_menus(cx);
                    this.ui.session_project_menu = open;
                    cx.notify();
                })),
        )
        .render()
        .relative()
        .when(app.ui.session_project_menu, |this| {
            this.child(deferred(project_dismiss(cx)))
                .child(deferred(project_popover(app, t, cx)).with_priority(1))
        })
}

const PROJECT_MENU_W: f32 = 300.;

fn project_popover(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let Some(open) = &app.session else {
        return div();
    };
    let repository = open.session.repository.clone();
    let slug = repo_slug(&repository);
    let tasks = open.tasks.len();
    div()
        .absolute()
        .top(px(BAR_H - 2.))
        .left_0()
        .w(px(PROJECT_MENU_W))
        .flex()
        .flex_col()
        .p(px(SPACE[0]))
        .rounded(px(RADIUS))
        .bg(t.popover)
        .border_1()
        .border_color(t.border)
        .text_size(px(TEXT_SECONDARY))
        .occlude()
        .child(project_row(
            icon("folder", ICON, t.muted_fg),
            slug.clone(),
            t,
        ))
        .child(project_row(
            icon("list-checks", ICON, t.muted_fg),
            format!("{tasks} task{}", if tasks == 1 { "" } else { "s" }),
            t,
        ))
        .child(project_row(
            icon("git-branch", ICON, t.muted_fg),
            open.session.base_branch.clone(),
            t,
        ))
        .child(project_row(icon("folder", ICON, t.muted_fg), repository, t))
        .child(separator(t))
        .child(
            project_row(
                icon("settings", ICON, t.muted_fg),
                "View project".to_string(),
                t,
            )
            .id("view-session-project")
            .cursor_pointer()
            .hover(|this| this.bg(t.muted))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.close_menus(cx);
                this.show_page(Page::Project(slug.clone()), cx);
            })),
        )
}

fn project_row(leading: impl IntoElement, text: String, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(SPACE[1]))
        .text_color(t.muted_fg)
        .child(leading)
        .child(div().min_w_0().truncate().child(text))
}

fn separator(t: &Tokens) -> Div {
    div().h(px(1.)).my(px(SPACE[0])).bg(t.border)
}

fn project_dismiss(cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    div()
        .id("session-project-dismiss")
        .absolute()
        .top(px(-4000.))
        .left(px(-4000.))
        .w(px(8000.))
        .h(px(8000.))
        .occlude()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_menus(cx)))
}

fn thread(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Stateful<Div> {
    div()
        .id("session-thread")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&app.ui.session_scroll)
        .flex()
        .flex_col()
        .items_center()
        .gap(px(SPACE[2]))
        .p(px(SPACE[2]))
        .children(rows(app, t, cx))
}

fn rows(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let Some(open) = &app.session else {
        return vec![note("Loading…", t)];
    };
    if open.tasks.is_empty() {
        return vec![note("Send a message to start this thread.", t)];
    }
    let now = now_ms();
    turns(&open.tasks, &app.session_events)
        .into_iter()
        .map(|turn| match turn {
            Turn::User(text) => user(text, t).into_any_element(),
            Turn::Task(task) => column()
                .child(card::card(app, task, events_of(app, &task.id), now, t, cx))
                .into_any_element(),
            Turn::Assistant(reason) => assistant(reason, t).into_any_element(),
        })
        .collect()
}

fn events_of<'a>(app: &'a LgtmApp, id: &str) -> &'a [StoredEvent] {
    app.session_events
        .iter()
        .find(|(held, _)| held == id)
        .map(|(_, events)| events.as_slice())
        .unwrap_or_default()
}

/// The width every turn is laid out in.
fn column() -> Div {
    div().w_full().max_w(px(COLUMN)).flex()
}

/// What the person asked for, in a filled bubble on the right.
fn user(text: &str, t: &Tokens) -> Div {
    column().justify_end().child(
        div()
            .max_w(px(BUBBLE_MAX))
            .px(px(SPACE[2]))
            .py(px(SPACE[1]))
            .rounded(px(RADIUS))
            .bg(t.muted)
            .child(text.to_string()),
    )
}

/// What the orchestrator decided, as plain text on the left.
fn assistant(reason: &str, t: &Tokens) -> Div {
    column().child(
        div()
            .max_w(px(BUBBLE_MAX))
            .text_color(t.muted_fg)
            .child(reason.to_string()),
    )
}

fn note(text: &'static str, t: &Tokens) -> AnyElement {
    column()
        .justify_center()
        .child(div().text_color(t.muted_fg).child(text))
        .into_any_element()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec, TaskStatus};

    pub(crate) fn task(id: &str, prompt: &str) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: "https://x/one.git".into(),
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
                session: Some("s1".into()),
                created_by: None,
            },
            status: TaskStatus::Queued,
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
    fn every_task_is_a_message_then_a_card_in_creation_order() {
        let tasks = vec![task("a", "first"), task("b", "second")];
        let turns = turns(&tasks, &[]);
        assert_eq!(turns[0], Turn::User("first"));
        assert_eq!(turns[1], Turn::Task(&tasks[0]));
        assert_eq!(turns[2], Turn::User("second"));
        assert_eq!(turns[3], Turn::Task(&tasks[1]));
        assert_eq!(turns.len(), 4);
    }

    #[test]
    fn what_the_orchestrator_said_follows_the_card_it_said_it_about() {
        let tasks = vec![task("a", "first")];
        let events = vec![(
            "a".to_string(),
            vec![
                StoredEvent {
                    at: 0,
                    event: TaskEvent::Orchestrated {
                        action: "retry".into(),
                        reason: "the checks failed".into(),
                        applied: true,
                        note: String::new(),
                    },
                },
                StoredEvent {
                    at: 1,
                    event: TaskEvent::Cancelled,
                },
            ],
        )];
        let turns = turns(&tasks, &events);
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[2], Turn::Assistant("the checks failed"));
    }
}
