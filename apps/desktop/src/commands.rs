//! What the root view does in response to the network and the user.

use crate::app::{LgtmApp, Overlay, Page, Pane, ERROR_TTL, STARTING};
use crate::net::{self, Action, Msg};
use crate::project::ProjectTab;
use crate::{home, render, settings};
use gpui::{Context, Window};
use lgtm_protocol::{Task, TaskStatus};

impl LgtmApp {
    pub(crate) fn apply(&mut self, msg: Msg, window: &mut Window, cx: &mut Context<Self>) {
        match msg {
            Msg::Lists(Ok(mut lists)) => {
                lists
                    .tasks
                    .sort_by_key(|task| std::cmp::Reverse(task.created_at));
                self.announce(&lists.tasks, cx);
                self.tasks = lists.tasks;
                self.runners = lists.runners;
                self.batches = lists.batches;
                self.goals = lists.goals;
                self.stats = lists.stats.or_else(|| self.stats.take());
                self.link.reachable = true;
            }
            Msg::Lists(Err(_)) => self.link.reachable = false,
            Msg::Detail { generation, detail } => {
                if generation != self.generation {
                    return;
                }
                self.lines = detail
                    .events
                    .iter()
                    .flat_map(|stored| render::render(&stored.event))
                    .collect();
                self.events = detail.events;
                self.overlaps = detail.overlaps;
                self.ui.content_scroll.scroll_to_bottom();
            }
            Msg::Live { generation, event } => {
                if generation != self.generation {
                    return;
                }
                self.lines.extend(render::render(&event.event));
                self.events.push(event);
                self.ui.content_scroll.scroll_to_bottom();
            }
            Msg::Action(Ok(created)) => self.created(created, window, cx),
            Msg::Batch(Ok(response)) => {
                self.import.issues = response.issues;
                if response.batch.is_some() {
                    self.ui.overlay = Overlay::None;
                    net::refresh(self.client.clone(), self.tx.clone());
                }
            }
            Msg::Action(Err(err)) | Msg::Batch(Err(err)) => self.set_error(err, cx),
        }
        cx.notify();
    }

    /// Notifies for every task this poll moved into a state a person cares
    /// about. A task the last poll didn't have is the baseline, not news, so
    /// the first poll after launch says nothing.
    fn announce(&self, polled: &[Task], cx: &gpui::App) {
        if !crate::theme::notify(cx) {
            return;
        }
        for task in polled {
            let Some(before) = self.tasks.iter().find(|known| known.id == task.id) else {
                continue;
            };
            if let Some(line) = lgtm_protocol::attention_for_status(task, before.status) {
                crate::notify::send("LGTM", &line);
            }
        }
    }

    /// An action went through; a new task also clears the prompt and opens.
    fn created(&mut self, created: Option<Task>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(task) = created {
            self.inputs
                .prompt
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.tasks.insert(0, task.clone());
            self.select(task.id, cx);
        }
        net::refresh(self.client.clone(), self.tx.clone());
    }

    /// What the strip says while the orchestrator is not answering.
    pub(crate) fn banner(&self) -> String {
        if self.link.hosted && self.link.started.elapsed() < STARTING {
            "Starting the orchestrator…".to_string()
        } else {
            format!("Orchestrator unreachable at {}", self.link.orchestrator)
        }
    }

    pub fn set_error(&mut self, err: String, cx: &mut Context<Self>) {
        self.error = Some(err);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ERROR_TTL).await;
            this.update(cx, |this, cx| {
                this.error = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Selects a task and remembers it, dropping any forward history.
    pub fn select(&mut self, id: String, cx: &mut Context<Self>) {
        if self.open(id.clone(), cx) {
            self.ui.visited.truncate(self.ui.visited_at);
            self.ui.visited.push(id);
            self.ui.visited_at = self.ui.visited.len();
        }
    }

    /// The selection itself. Returns false when nothing changed, so back and
    /// forward don't rewrite the history they are walking.
    fn open(&mut self, id: String, cx: &mut Context<Self>) -> bool {
        if self.selected.as_deref() == Some(id.as_str()) {
            return false;
        }
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }
        self.generation += 1;
        self.clear_detail();
        self.ui.show_follow_up = false;
        self.pane = default_pane(
            self.tasks
                .iter()
                .find(|task| task.id == id)
                .map(|task| task.status),
        );
        self.selected = Some(id.clone());
        self.stream = Some(net::watch(
            self.client.clone(),
            id,
            self.generation,
            self.tx.clone(),
        ));
        cx.notify();
        true
    }

    pub fn can_go_back(&self) -> bool {
        self.ui.visited_at > 1
    }

    pub fn can_go_forward(&self) -> bool {
        self.ui.visited_at < self.ui.visited.len()
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_back() {
            return;
        }
        self.ui.visited_at -= 1;
        let id = self.ui.visited[self.ui.visited_at - 1].clone();
        self.open(id, cx);
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_forward() {
            return;
        }
        let id = self.ui.visited[self.ui.visited_at].clone();
        self.ui.visited_at += 1;
        self.open(id, cx);
    }

    pub fn selected_task(&self) -> Option<&Task> {
        let id = self.selected.as_deref()?;
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn act(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        net::act(self.client.clone(), id, action, self.tx.clone());
        cx.notify();
    }

    pub fn go_home(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_page(Page::Home, cx);
        self.inputs
            .prompt
            .update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }
        self.generation += 1;
        self.selected = None;
        self.clear_detail();
        self.page = page;
        self.ui.overlay = Overlay::None;
        cx.notify();
    }

    /// Opens a project page. A goal id lands on the Goals tab, scrolled to it.
    pub fn open_project(&mut self, slug: String, goal: Option<String>, cx: &mut Context<Self>) {
        self.ui.project_tab = match &goal {
            Some(_) => ProjectTab::Goals,
            None => ProjectTab::Overview,
        };
        if let Some(id) = goal {
            let at = crate::project::goals_of(self, &slug)
                .iter()
                .position(|summary| summary.goal.id == id)
                .unwrap_or(0);
            self.ui.project_scroll.scroll_to_top_of_item(at);
        }
        self.show_page(Page::Project(slug), cx);
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.overlay = Overlay::Palette;
        self.ui.palette_at = 0;
        self.inputs
            .query
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.inputs
            .query
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.ui.overlay = Overlay::Settings;
        cx.notify();
    }

    /// Opens Settings scrolled to the Runners section.
    pub fn open_runner_settings(&mut self, cx: &mut Context<Self>) {
        self.ui
            .settings_scroll
            .scroll_to_top_of_item(settings::RUNNERS_SECTION);
        self.open_settings(cx);
    }

    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.overlay = Overlay::None;
        self.close_menus(cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.ui.sidebar_open = !self.ui.sidebar_open;
        cx.notify();
    }

    /// Repositories seen on existing tasks, plus the one the composer holds.
    pub fn known_repositories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for url in self
            .tasks
            .iter()
            .map(|task| task.spec.repository.clone())
            .chain(self.composer.project.clone())
        {
            if !out.contains(&url) {
                out.push(url);
            }
        }
        out
    }

    /// Opens the follow-up field under the task header and focuses it.
    pub fn open_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ui.show_follow_up = true;
        self.inputs
            .follow_up
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn send_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.inputs.follow_up.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.inputs
            .follow_up
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.ui.show_follow_up = false;
        self.act(Action::Tell(text), cx);
    }

    pub fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.inputs.prompt.read(cx).value().to_string();
        let Some(spec) = home::compose(
            &prompt,
            self.composer.project.as_deref(),
            &self.composer.chips,
        ) else {
            self.set_error("A prompt and a project are required.".into(), cx);
            return;
        };
        net::act(
            self.client.clone(),
            String::new(),
            Action::Create(Box::new(spec)),
            self.tx.clone(),
        );
        cx.notify();
    }

    /// ⌘↩ sends the follow-up when one is open, otherwise starts a task.
    pub(crate) fn submit_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_some() && self.ui.show_follow_up {
            self.send_follow_up(window, cx);
        } else if self.selected.is_none() {
            self.submit(window, cx);
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tasks.is_empty() {
            return;
        }
        let current = self
            .selected
            .as_deref()
            .and_then(|id| self.tasks.iter().position(|task| task.id == id))
            .unwrap_or(0) as isize;
        let next = (current + delta).clamp(0, self.tasks.len() as isize - 1) as usize;
        let id = self.tasks[next].id.clone();
        self.select(id, cx);
    }

    pub(crate) fn show(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.pane = pane;
        cx.notify();
    }

    /// Drops everything that belonged to the task being left.
    fn clear_detail(&mut self) {
        self.lines.clear();
        self.events.clear();
        self.overlaps.clear();
        self.ui.editing_notes = false;
    }

    /// Opens the Changes tab at `file`. The diff is parsed here too, so a
    /// finding can jump to a file before the tab has ever rendered.
    pub fn open_changes_at(&mut self, file: &str, cx: &mut Context<Self>) {
        let diff = self
            .selected_task()
            .and_then(|task| Some((task.id.clone(), task.result.as_ref()?.diff.clone())));
        if let Some((id, diff)) = diff {
            self.review.load(&id, &diff);
        }
        if let Some(at) = self.review.files.iter().position(|f| f.name == file) {
            self.review.focus_file(at);
        }
        self.show(Pane::Changes, cx);
    }

    /// Puts the task's notes in the editor and focuses it.
    pub fn edit_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let notes = self
            .selected_task()
            .map(|task| task.scratchpad.clone())
            .unwrap_or_default();
        self.inputs.notes.update(cx, |state, cx| {
            state.set_value(notes, window, cx);
            state.focus(window, cx);
        });
        self.ui.editing_notes = true;
        cx.notify();
    }

    pub fn save_notes(&mut self, cx: &mut Context<Self>) {
        let notes = self.inputs.notes.read(cx).value().to_string();
        self.ui.editing_notes = false;
        self.act(Action::SetScratchpad(notes), cx);
    }
}

/// The tab a task opens on: the one its status makes it about.
pub(crate) fn default_pane(status: Option<TaskStatus>) -> Pane {
    match status {
        Some(TaskStatus::AwaitingReview | TaskStatus::Conflicted) => Pane::Review,
        Some(TaskStatus::Running) => Pane::Activity,
        _ => Pane::Overview,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_tab_follows_what_the_task_needs() {
        assert!(default_pane(Some(TaskStatus::AwaitingReview)) == Pane::Review);
        assert!(default_pane(Some(TaskStatus::Conflicted)) == Pane::Review);
        assert!(default_pane(Some(TaskStatus::Running)) == Pane::Activity);
        assert!(default_pane(Some(TaskStatus::Merged)) == Pane::Overview);
        assert!(default_pane(None) == Pane::Overview);
    }
}
