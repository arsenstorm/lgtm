//! What the root view does in response to the network and the user.

use crate::app::{LgtmApp, Overlay, Page, Pane, ERROR_TTL, STARTING};
use crate::net::{self, Action, Msg};
use crate::{home, render, settings};
use gpui::{Context, Window};
use lgtm_protocol::Task;

impl LgtmApp {
    pub(crate) fn apply(&mut self, msg: Msg, window: &mut Window, cx: &mut Context<Self>) {
        match msg {
            Msg::Lists(Ok((mut tasks, workers, batches))) => {
                tasks.sort_by_key(|task| std::cmp::Reverse(task.created_at));
                self.tasks = tasks;
                self.workers = workers;
                self.batches = batches;
                self.reachable = true;
            }
            Msg::Lists(Err(_)) => self.reachable = false,
            Msg::Detail { generation, detail } => {
                if generation != self.generation {
                    return;
                }
                self.lines = detail
                    .events
                    .iter()
                    .flat_map(|stored| render::render(&stored.event))
                    .collect();
                self.content_scroll.scroll_to_bottom();
            }
            Msg::Live { generation, event } => {
                if generation != self.generation {
                    return;
                }
                self.lines.extend(render::render(&event.event));
                self.content_scroll.scroll_to_bottom();
            }
            Msg::Action(Ok(created)) => self.created(created, window, cx),
            Msg::Batch(Ok(response)) => {
                self.import.issues = response.issues;
                if response.batch.is_some() {
                    self.overlay = Overlay::None;
                    net::refresh(self.client.clone(), self.tx.clone());
                }
            }
            Msg::Action(Err(err)) | Msg::Batch(Err(err)) => self.set_error(err, cx),
        }
        cx.notify();
    }

    /// An action went through; a new task also clears the prompt and opens.
    fn created(&mut self, created: Option<Task>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(task) = created {
            self.prompt
                .update(cx, |state, cx| state.set_value("", window, cx));
            self.tasks.insert(0, task.clone());
            self.select(task.id, cx);
        }
        net::refresh(self.client.clone(), self.tx.clone());
    }

    /// What the strip says while the orchestrator is not answering.
    pub(crate) fn banner(&self) -> String {
        if self.hosted && self.started.elapsed() < STARTING {
            "Starting the orchestrator…".to_string()
        } else {
            format!("Orchestrator unreachable at {}", self.orchestrator)
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
            self.visited.truncate(self.visited_at);
            self.visited.push(id);
            self.visited_at = self.visited.len();
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
        self.lines.clear();
        self.show_follow_up = false;
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
        self.visited_at > 1
    }

    pub fn can_go_forward(&self) -> bool {
        self.visited_at < self.visited.len()
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_back() {
            return;
        }
        self.visited_at -= 1;
        let id = self.visited[self.visited_at - 1].clone();
        self.open(id, cx);
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_forward() {
            return;
        }
        let id = self.visited[self.visited_at].clone();
        self.visited_at += 1;
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
        self.prompt.update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn show_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }
        self.generation += 1;
        self.selected = None;
        self.lines.clear();
        self.page = page;
        self.overlay = Overlay::None;
        cx.notify();
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::Palette;
        self.palette_at = 0;
        self.query
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.query.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::Settings;
        cx.notify();
    }

    /// Opens Settings scrolled to the Workers section.
    pub fn open_worker_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_scroll
            .scroll_to_top_of_item(settings::WORKERS_SECTION);
        self.open_settings(cx);
    }

    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        self.close_menus(cx);
        window.focus(&self.focus);
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_open = !self.sidebar_open;
        cx.notify();
    }

    /// Repositories seen on existing tasks, plus the one the composer holds.
    pub fn known_repositories(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for url in self
            .tasks
            .iter()
            .map(|task| task.spec.repository.clone())
            .chain(self.project.clone())
        {
            if !out.contains(&url) {
                out.push(url);
            }
        }
        out
    }

    /// Opens the follow-up field under the task header and focuses it.
    pub fn open_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_follow_up = true;
        self.follow_up
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn send_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.follow_up.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.follow_up
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.show_follow_up = false;
        self.act(Action::Tell(text), cx);
    }

    pub fn submit(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.prompt.read(cx).value().to_string();
        let Some(spec) = home::compose(&prompt, self.project.as_deref(), &self.chips) else {
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
        if self.selected.is_some() && self.show_follow_up {
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
}
