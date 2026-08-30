//! What the root view does in response to the network and the user.

use crate::app::{LgtmApp, Overlay, Page, Pane, Shell, Stop, ERROR_TTL, STARTING};
use crate::net::{self, Action, Msg};
use crate::project::ProjectTab;
use crate::{home, render, settings};
use gpui::{Context, Window};
use lgtm_protocol::{StoredEvent, Task, TaskStatus, Todo};

impl LgtmApp {
    pub(crate) fn apply(&mut self, msg: Msg, cx: &mut Context<Self>) {
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
                self.sessions = lists.sessions;
                self.stats = lists.stats.or_else(|| self.stats.take());
                self.link.reachable = true;
                // The project page needs a clone URL, which the first poll to
                // arrive is the only thing that can supply.
                if self.project_stream.is_none() {
                    self.watch_project(cx);
                }
            }
            Msg::Lists(Err(_)) => self.link.reachable = false,
            Msg::Detail { generation, detail } => {
                if generation != self.generation {
                    return;
                }
                let detail = *detail;
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
            Msg::Session {
                generation,
                detail,
                task,
            } => {
                if generation != self.generation {
                    return;
                }
                self.session = Some(*detail);
                if let Some((id, events)) = task {
                    self.remember_events(id, events);
                }
            }
            Msg::Notes {
                generation,
                memories,
                todos,
            } => {
                if generation != self.generation {
                    return;
                }
                self.memories = memories;
                self.todos = todos;
            }
            Msg::Plans { generation, plans } => {
                if generation != self.generation {
                    return;
                }
                self.plans = plans;
            }
            Msg::Terminal { generation, data } => {
                if generation != self.generation {
                    return;
                }
                if let Some(shell) = self.shell.as_mut() {
                    shell.push(&data);
                    self.ui.terminal_scroll.scroll_to_bottom();
                }
            }
            Msg::Artefact { task, name, bytes } => {
                let image = crate::app::artefact_format(&name)
                    .filter(|_| !bytes.is_empty())
                    .map(|format| std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)));
                self.artefacts.insert((task, name), image);
            }
            Msg::Action(Ok(())) => {
                net::refresh(self.client.clone(), self.tx.clone());
                self.watch_project(cx);
            }
            Msg::Opened(Ok(id)) => {
                self.show_page(Page::Session(id), cx);
                net::refresh(self.client.clone(), self.tx.clone());
            }
            Msg::Opened(Err(err)) => self.set_error(err, cx),
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

    /// Keeps one event list per task of the open session, replacing the entry
    /// for the task this poll fetched.
    fn remember_events(&mut self, id: String, events: Vec<StoredEvent>) {
        match self.session_events.iter_mut().find(|(held, _)| *held == id) {
            Some(entry) => entry.1 = events,
            None => self.session_events.push((id, events)),
        }
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
        self.go(Stop::Task(id), cx);
    }

    /// Goes somewhere and remembers it, dropping any forward history.
    fn go(&mut self, stop: Stop, cx: &mut Context<Self>) {
        if self.open(stop.clone(), cx) {
            self.ui.visited.truncate(self.ui.visited_at);
            self.ui.visited.push(stop);
            self.ui.visited_at = self.ui.visited.len();
        }
    }

    /// The move itself. Returns false when nothing changed, so back and
    /// forward don't rewrite the history they are walking.
    fn open(&mut self, stop: Stop, cx: &mut Context<Self>) -> bool {
        if self.already_at(&stop) {
            return false;
        }
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }
        self.generation += 1;
        self.clear_detail();
        self.ui.overlay = Overlay::None;
        match stop {
            Stop::Task(id) => self.enter_task(id),
            Stop::Page(page) => self.enter_page(page),
        }
        self.watch_project(cx);
        cx.notify();
        true
    }

    /// Fetches an artefact's bytes the first time the Review tab asks for
    /// them. The entry is claimed before the request goes out, so a tab that
    /// renders many times over asks once.
    pub(crate) fn want_artefact(&mut self, name: String) {
        let Some(task) = self.selected.clone() else {
            return;
        };
        let key = (task.clone(), name.clone());
        if self.artefacts.contains_key(&key) {
            return;
        }
        self.artefacts.insert(key, None);
        net::fetch_artefact(self.client.clone(), task, name, self.tx.clone());
    }

    /// Keeps the open project tab fed: the list tabs poll, Plans fetches once,
    /// every other tab reads what the window already has. Called again after
    /// an action so its effect shows without waiting out the interval.
    pub(crate) fn watch_project(&mut self, cx: &mut Context<Self>) {
        if let Some(stream) = self.project_stream.take() {
            stream.abort();
        }
        let (Page::Project(slug), None) = (&self.page, &self.selected) else {
            return;
        };
        let Some(repository) = crate::project::repository_of(self, slug) else {
            return;
        };
        let (client, tx) = (self.client.clone(), self.tx.clone());
        self.project_stream = match self.ui.project_tab {
            ProjectTab::Memories | ProjectTab::Todos => {
                Some(net::watch_notes(client, repository, self.generation, tx))
            }
            ProjectTab::Plans => {
                let goals = crate::project::goals_of(self, slug)
                    .iter()
                    .map(|summary| summary.goal.id.clone())
                    .collect();
                Some(net::fetch_plans(client, goals, self.generation, tx))
            }
            _ => None,
        };
        cx.notify();
    }

    pub fn show_project_tab(&mut self, tab: ProjectTab, cx: &mut Context<Self>) {
        self.ui.project_tab = tab;
        self.watch_project(cx);
    }

    fn already_at(&self, stop: &Stop) -> bool {
        match stop {
            Stop::Task(id) => self.selected.as_deref() == Some(id.as_str()),
            Stop::Page(page) => self.selected.is_none() && &self.page == page,
        }
    }

    fn enter_task(&mut self, id: String) {
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
    }

    /// A session page polls itself; the other pages have nothing to watch.
    fn enter_page(&mut self, page: Page) {
        self.selected = None;
        if let Page::Session(id) = &page {
            self.stream = Some(net::watch_session(
                self.client.clone(),
                id.clone(),
                self.generation,
                self.tx.clone(),
            ));
        }
        self.page = page;
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
        let stop = self.ui.visited[self.ui.visited_at - 1].clone();
        self.open(stop, cx);
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if !self.can_go_forward() {
            return;
        }
        let stop = self.ui.visited[self.ui.visited_at].clone();
        self.ui.visited_at += 1;
        self.open(stop, cx);
    }

    pub fn selected_task(&self) -> Option<&Task> {
        let id = self.selected.as_deref()?;
        self.tasks.iter().find(|task| task.id == id)
    }

    pub fn act(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.act_on(id, action, cx);
    }

    /// Acts on `id`, which is a memory's or a todo's as often as a task's.
    pub fn act_on(&mut self, id: String, action: Action, cx: &mut Context<Self>) {
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
        self.go(Stop::Page(page), cx);
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
            .chain(self.sessions.iter().map(|open| open.repository.clone()))
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

    /// Starts an empty thread in `repository` and opens it once it exists.
    pub fn start_session(&mut self, repository: String, cx: &mut Context<Self>) {
        net::new_session(
            self.client.clone(),
            repository,
            "main".to_string(),
            self.tx.clone(),
        );
        cx.notify();
    }

    /// The repository the composer works in: a thread's own, or the one
    /// picked in the project panel.
    pub fn composer_project(&self) -> Option<String> {
        match (&self.page, &self.session) {
            (Page::Session(_), Some(open)) => Some(open.session.repository.clone()),
            _ => self.composer.project.clone(),
        }
    }

    /// Sends the composer's text: one more message in the open thread, or a
    /// new thread when there is none.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(todo) = self.promoting.take() {
            self.submit_promotion(todo, window, cx);
            return;
        }
        let prompt = self.inputs.prompt.read(cx).value().to_string();
        let project = self.composer_project();
        let models = crate::theme::models(cx);
        let Some(spec) = home::compose(&prompt, project.as_deref(), &self.composer.chips, &models)
        else {
            self.set_error("A prompt and a project are required.".into(), cx);
            return;
        };
        let spec = Box::new(spec);
        let (id, action) = match &self.page {
            Page::Session(id) if self.selected.is_none() => (id.clone(), Action::SendMessage(spec)),
            _ => (String::new(), Action::StartSession(spec)),
        };
        self.inputs
            .prompt
            .update(cx, |state, cx| state.set_value("", window, cx));
        net::act(self.client.clone(), id, action, self.tx.clone());
        cx.notify();
    }

    /// Promotion is the endpoint's own job: it turns the todo's stored title
    /// and description into the task, so the composer's text is only ever a
    /// preview of what will run.
    fn submit_promotion(&mut self, todo: String, window: &mut Window, cx: &mut Context<Self>) {
        let models = crate::theme::models(cx);
        let into = lgtm_client::PromoteTodo {
            base_branch: home::branch_of(&self.composer.chips),
            executor: models.executor,
            runner: home::runner_of(&self.composer.chips),
        };
        self.inputs
            .prompt
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.act_on(todo, Action::Promote(Box::new(into)), cx);
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

    /// Switching tabs attaches or detaches the shell: an unwatched terminal
    /// has no reason to hold a socket open.
    pub(crate) fn show(&mut self, pane: Pane, cx: &mut Context<Self>) {
        if self.pane == pane {
            return;
        }
        self.pane = pane;
        match pane {
            Pane::Terminal => self.attach_shell(),
            _ => self.detach_shell(),
        }
        cx.notify();
    }

    fn attach_shell(&mut self) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        self.detach_shell();
        let (stream, input) =
            net::attach_terminal(self.client.clone(), id, self.generation, self.tx.clone());
        self.shell = Some(Shell {
            output: String::new(),
            input,
            stream,
        });
    }

    /// Detaching leaves the shell running on the runner; `Close` kills it.
    fn detach_shell(&mut self) {
        if let Some(shell) = self.shell.take() {
            shell.stream.abort();
        }
    }

    pub fn send_to_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let line = self.inputs.shell.read(cx).value().to_string();
        self.inputs
            .shell
            .update(cx, |state, cx| state.set_value("", window, cx));
        if let Some(shell) = self.shell.as_ref() {
            let _ = shell.input.send(format!("{line}\n"));
        }
        cx.notify();
    }

    /// Kills the shell and detaches; the tab starts a new one on reattach.
    pub fn close_shell(&mut self, cx: &mut Context<Self>) {
        self.detach_shell();
        self.act(Action::CloseTerminal, cx);
    }

    pub fn add_memory(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.inputs.memory.read(cx).value().trim().to_string();
        let Some(repository) = self.open_repository() else {
            return;
        };
        if content.is_empty() {
            return;
        }
        self.inputs
            .memory
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.act_on(
            String::new(),
            Action::AddMemory {
                repository,
                content,
            },
            cx,
        );
    }

    pub fn add_todo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let title = self.inputs.todo.read(cx).value().trim().to_string();
        let Some(repository) = self.open_repository() else {
            return;
        };
        if title.is_empty() {
            return;
        }
        self.inputs
            .todo
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.act_on(String::new(), Action::AddTodo { repository, title }, cx);
    }

    /// Puts the todo in the composer, in its own project, and remembers it so
    /// submitting promotes it rather than starting a loose task.
    pub fn promote_todo(&mut self, todo: &Todo, window: &mut Window, cx: &mut Context<Self>) {
        let text = match todo.description.trim() {
            "" => todo.title.clone(),
            description => format!("{}\n\n{description}", todo.title),
        };
        let project = todo.repository.clone().or_else(|| self.open_repository());
        // Home first: leaving a page clears the pending promotion.
        self.go_home(window, cx);
        self.promoting = Some(todo.id.clone());
        self.composer.project = project;
        self.inputs
            .prompt
            .update(cx, |state, cx| state.set_value(text, window, cx));
        cx.notify();
    }

    /// The clone URL of whatever the window is showing.
    fn open_repository(&self) -> Option<String> {
        match &self.page {
            Page::Project(slug) => crate::project::repository_of(self, slug),
            _ => self.composer_project(),
        }
    }

    pub fn save_model(&mut self, cx: &mut Context<Self>) {
        let model = self.inputs.model.read(cx).value().trim().to_string();
        let mut models = crate::theme::models(cx);
        if models.model == model {
            return;
        }
        models.model = model;
        crate::theme::set_models(models, cx);
        cx.notify();
    }

    /// Drops everything that belonged to the task or session being left.
    fn clear_detail(&mut self) {
        self.lines.clear();
        self.events.clear();
        self.overlaps.clear();
        self.artefacts.clear();
        self.session = None;
        self.session_events.clear();
        self.memories.clear();
        self.todos.clear();
        self.plans.clear();
        self.promoting = None;
        self.detach_shell();
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
