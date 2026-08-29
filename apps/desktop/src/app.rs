//! The root view: state, key bindings, and the window bar / sidebar / main split.

use crate::home::Chip;
use crate::import::ImportForm;
use crate::keys::{
    CloseOverlay, NewTask, OpenPalette, PaletteNext, PalettePrev, PaletteRun, SelectNext,
    SelectPrev, ShowActivity, ShowChanges, ShowChecks, ShowPlan, Submit, ToggleSidebar, CONTEXT,
};
use crate::net::{self, Action, Msg};
use crate::render::{self, Line};
use crate::theme::{tokens, SPACE, STATUS_H, TEXT_BODY, TEXT_SECONDARY, UI_FONT};
use crate::{batches, home, import, palette, panes, settings, sidebar, titlebar};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, Styled as _,
    Subscription, Task as GpuiTask, Window,
};
use gpui_component::input::{InputEvent, InputState};
use lgtm_client::Client;
use lgtm_protocol::{Batch, Task, TaskStatus, WorkerStatus};
use std::collections::HashSet;
use std::time::Duration;
use tokio::task::JoinHandle;

const ERROR_TTL: Duration = Duration::from_secs(5);
const HEADER_PREVIEW: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Activity,
    Changes,
    Checks,
    Plan,
}

/// What the main area shows when no task is selected.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Batches,
}

/// The one modal that can be up at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Palette,
    Settings,
    Import,
}

pub struct LgtmApp {
    pub client: Client,
    pub tx: net::Sender,
    pub tasks: Vec<Task>,
    pub workers: Vec<WorkerStatus>,
    pub batches: Vec<Batch>,
    pub selected: Option<String>,
    pub page: Page,
    pub pane: Pane,
    pub review: crate::review::ReviewState,

    pub orchestrator: String,
    pub token: String,
    pub token_source: &'static str,
    pub reachable: bool,
    pub error: Option<String>,
    pub focus: FocusHandle,
    /// Bumped on every selection so events from the previous stream are dropped.
    pub generation: u64,
    stream: Option<JoinHandle<()>>,
    pub lines: Vec<Line>,

    pub prompt: Entity<InputState>,
    pub repo_url: Entity<InputState>,
    pub base_branch: Entity<InputState>,
    pub follow_up: Entity<InputState>,
    pub query: Entity<InputState>,
    pub import: ImportForm,

    /// The repository the composer will use, as a clone URL.
    pub project: Option<String>,
    pub chips: Vec<Chip>,
    pub project_menu: bool,
    pub add_repo: bool,
    pub plus_menu: bool,
    pub plus_view: home::PlusView,

    pub overlay: Overlay,
    pub palette_at: usize,
    /// Batches whose task rows are unfolded.
    pub expanded: HashSet<String>,
    pub settings_scroll: ScrollHandle,
    pub sidebar_open: bool,
    /// Tasks selected so far, and where in that list we are.
    pub visited: Vec<String>,
    pub visited_at: usize,
    pub show_follow_up: bool,
    pub dragging: bool,
    pub task_scroll: ScrollHandle,
    pub content_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
    _pump: GpuiTask<()>,
}

impl LgtmApp {
    pub fn new(
        client: Client,
        orchestrator: String,
        token: String,
        token_source: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        net::poll(client.clone(), tx.clone());

        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(2, 8)
                .placeholder("Describe your task…")
        });
        let repo_url = field("https://github.com/you/repo.git", window, cx);
        let base_branch = cx.new(|cx| InputState::new(window, cx).default_value("main"));
        let follow_up = field("Ask for a change…", window, cx);
        let query = field("Search tasks, repositories, actions…", window, cx);
        let import = ImportForm::new(window, cx);

        let subscriptions = vec![
            cx.subscribe_in(
                &follow_up,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.send_follow_up(window, cx);
                    }
                },
            ),
            // ⌘↩ inside the prompt is the input's own binding, so the composer
            // hears about it as an event rather than as the Submit action.
            cx.subscribe_in(
                &prompt,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { secondary: true }) {
                        this.submit(window, cx);
                    }
                },
            ),
            cx.subscribe_in(&query, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.palette_at = 0;
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::theme::apply(Some(window), cx);
                cx.notify();
            }),
        ];

        let pump = cx.spawn_in(window, async move |this, cx| {
            while let Some(msg) = rx.recv().await {
                if this
                    .update_in(cx, |this, window, cx| this.apply(msg, window, cx))
                    .is_err()
                {
                    return;
                }
            }
        });

        let focus = cx.focus_handle();
        window.focus(&focus);
        Self {
            client,
            tx,
            tasks: Vec::new(),
            workers: Vec::new(),
            batches: Vec::new(),
            selected: None,
            page: Page::Home,
            pane: Pane::Activity,
            review: crate::review::ReviewState::new(),
            orchestrator,
            token,
            token_source,
            reachable: false,
            error: None,
            focus,
            generation: 0,
            stream: None,
            lines: Vec::new(),
            prompt,
            repo_url,
            base_branch,
            follow_up,
            query,
            import,
            project: None,
            chips: Vec::new(),
            project_menu: false,
            add_repo: false,
            plus_menu: false,
            plus_view: home::PlusView::Root,
            overlay: Overlay::None,
            palette_at: 0,
            expanded: HashSet::new(),
            settings_scroll: ScrollHandle::new(),
            sidebar_open: true,
            visited: Vec::new(),
            visited_at: 0,
            show_follow_up: false,
            dragging: false,
            task_scroll: ScrollHandle::new(),
            content_scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
            _pump: pump,
        }
    }

    fn apply(&mut self, msg: Msg, window: &mut Window, cx: &mut Context<Self>) {
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
            Msg::Action(Ok(created)) => {
                if let Some(task) = created {
                    self.prompt
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    self.tasks.insert(0, task.clone());
                    self.select(task.id, cx);
                }
                net::refresh(self.client.clone(), self.tx.clone());
            }
            Msg::Action(Err(err)) => self.set_error(err, cx),
            Msg::Batch(Ok(response)) => {
                self.import.issues = response.issues;
                if response.batch.is_some() {
                    self.overlay = Overlay::None;
                    net::refresh(self.client.clone(), self.tx.clone());
                }
            }
            Msg::Batch(Err(err)) => self.set_error(err, cx),
        }
        cx.notify();
    }

    fn set_error(&mut self, err: String, cx: &mut Context<Self>) {
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

    /// Opens Settings, scrolled to the Workers section when asked.
    pub fn open_settings(&mut self, workers: bool, cx: &mut Context<Self>) {
        self.overlay = Overlay::Settings;
        if workers {
            self.settings_scroll
                .scroll_to_top_of_item(settings::WORKERS_SECTION);
        }
        cx.notify();
    }

    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
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
    fn submit_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_some() && self.show_follow_up {
            self.send_follow_up(window, cx);
        } else if self.selected.is_none() {
            self.submit(window, cx);
        }
    }

    fn move_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
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

    fn show(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.pane = pane;
        cx.notify();
    }
}

fn field(
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<LgtmApp>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

/// First line of the prompt, truncated to `limit` characters.
pub fn prompt_preview(prompt: &str, limit: usize) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    if line.chars().count() > limit {
        format!("{}…", line.chars().take(limit).collect::<String>())
    } else {
        line.to_string()
    }
}

pub fn header_preview(prompt: &str) -> String {
    prompt_preview(prompt, HEADER_PREVIEW)
}

/// Display status for a task. Queued tasks waiting on unmet dependencies show
/// as `blocked` instead of `queued` (display only, doesn't affect `status`).
pub fn status_label(task: &Task, tasks: &[Task]) -> &'static str {
    if task.status == TaskStatus::Queued && task.worker.is_none() && is_blocked(task, tasks) {
        return "blocked";
    }
    match task.status {
        TaskStatus::Queued => "queued",
        TaskStatus::Running => "running",
        TaskStatus::AwaitingReview => "awaiting_review",
        TaskStatus::Approved => "approved",
        TaskStatus::Merged => "merged",
        TaskStatus::Rejected => "rejected",
        TaskStatus::Failed => "failed",
        TaskStatus::Cancelled => "cancelled",
    }
}

/// True when `task` depends on another task that isn't yet approved/merged.
/// Dependencies absent from `tasks` don't block (nothing known to wait on).
fn is_blocked(task: &Task, tasks: &[Task]) -> bool {
    !task.spec.depends_on.is_empty()
        && !task.spec.depends_on.iter().all(|dep_id| {
            tasks
                .iter()
                .find(|t| &t.id == dep_id)
                .is_none_or(|t| matches!(t.status, TaskStatus::Approved | TaskStatus::Merged))
        })
}

impl Focusable for LgtmApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LgtmApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let unreachable = !self.reachable;
        let overlay = self.overlay;
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewTask, window, cx| this.go_home(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_action(window, cx)))
            .on_action(
                cx.listener(|this, _: &CloseOverlay, window, cx| this.close_overlay(window, cx)),
            )
            .on_action(cx.listener(|this, _: &PaletteNext, _, cx| palette::step(this, 1, cx)))
            .on_action(cx.listener(|this, _: &PalettePrev, _, cx| palette::step(this, -1, cx)))
            .on_action(
                cx.listener(|this, _: &PaletteRun, window, cx| palette::run(this, window, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &ShowActivity, _, cx| this.show(Pane::Activity, cx)))
            .on_action(cx.listener(|this, _: &ShowChanges, _, cx| this.show(Pane::Changes, cx)))
            .on_action(cx.listener(|this, _: &ShowChecks, _, cx| this.show(Pane::Checks, cx)))
            .on_action(cx.listener(|this, _: &ShowPlan, _, cx| this.show(Pane::Plan, cx)))
            .on_action(cx.listener(|this, _: &crate::review::MarkViewed, _, cx| {
                this.review.mark_current_viewed();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::review::NextFile, _, cx| {
                this.review.step_file(1);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &crate::review::PrevFile, _, cx| {
                this.review.step_file(-1);
                cx.notify();
            }))
            .on_action(
                cx.listener(|this, _: &crate::review::ToggleDiffStyle, _, cx| {
                    this.review.flip_style();
                    cx.notify();
                }),
            )
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.fg)
            .font_family(UI_FONT)
            .text_size(px(TEXT_BODY))
            .child(titlebar::bar(self, cx))
            .when(unreachable, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .h(px(STATUS_H))
                        .px(px(SPACE[2]))
                        // `bg-destructive/10 text-destructive`, the reference's
                        // destructive fill — a loud strip would own the window.
                        .bg(gpui::Hsla {
                            a: 0.12,
                            ..t.danger
                        })
                        .text_color(t.danger)
                        .text_size(px(TEXT_SECONDARY))
                        .child(format!("Orchestrator unreachable at {}", self.orchestrator)),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.sidebar_open, |this| {
                        this.child(sidebar::render_sidebar(self, window, cx))
                    })
                    .child(if self.selected.is_some() {
                        panes::task_view(self, window, cx)
                    } else if self.page == Page::Batches {
                        batches::page(self, cx)
                    } else {
                        home::home(self, window, cx)
                    }),
            )
            .when(overlay == Overlay::Palette, |this| {
                this.child(palette::view(self, cx))
            })
            .when(overlay == Overlay::Settings, |this| {
                this.child(settings::view(self, cx))
            })
            .when(overlay == Overlay::Import, |this| {
                this.child(import::modal(self, cx))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::{Executor, TaskKind, TaskSpec};

    fn task(id: &str, status: TaskStatus, depends_on: Vec<&str>) -> Task {
        Task {
            id: id.into(),
            spec: TaskSpec {
                repository: "r".into(),
                base_branch: "main".into(),
                prompt: "p".into(),
                executor: Executor::Claude,
                worker: None,
                issue: None,
                linear: None,
                kind: TaskKind::Run,
                parent: None,
                depends_on: depends_on.into_iter().map(String::from).collect(),
                batch: None,
            },
            status,
            worker: None,
            created_at: 0,
            result: None,
            error: None,
            pull_request: None,
            ci: None,
        }
    }

    #[test]
    fn queued_task_with_unmet_dependency_is_blocked() {
        let dep = task("dep", TaskStatus::Running, vec![]);
        let queued = task("q", TaskStatus::Queued, vec!["dep"]);
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "blocked");
    }

    #[test]
    fn queued_task_with_approved_dependency_is_queued() {
        let dep = task("dep", TaskStatus::Approved, vec![]);
        let queued = task("q", TaskStatus::Queued, vec!["dep"]);
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "queued");
    }

    #[test]
    fn assigned_worker_is_not_blocked_even_with_unmet_dependency() {
        let dep = task("dep", TaskStatus::Running, vec![]);
        let mut queued = task("q", TaskStatus::Queued, vec!["dep"]);
        queued.worker = Some("compute".into());
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "queued");
    }

    #[test]
    fn prompt_preview_truncates_to_the_first_line() {
        assert_eq!(prompt_preview("one\ntwo", 32), "one");
        assert_eq!(prompt_preview("abcdef", 3), "abc…");
    }
}
