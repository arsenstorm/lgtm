//! The root view: state, key bindings, and the sidebar/content split.

use crate::net::{self, Action, Msg};
use crate::render::{self, Line};
use crate::theme::{tokens, SPACE, TEXT_BODY, TEXT_SECONDARY, UI_FONT};
use crate::{home, panes, sidebar};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    actions, div, px, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollHandle,
    Styled as _, Subscription, Task as GpuiTask, Window,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::select::SelectState;
use lgtm_client::Client;
use lgtm_protocol::{Batch, Executor, Task, TaskKind, TaskSpec, TaskStatus, WorkerStatus};
use std::time::Duration;
use tokio::task::JoinHandle;

actions!(
    lgtm,
    [
        NewTask,
        ToggleSearch,
        SelectNext,
        SelectPrev,
        ShowActivity,
        ShowChanges,
        ShowChecks,
        ShowPlan,
        Submit,
    ]
);

pub const CONTEXT: &str = "Lgtm";
/// Repository picker entry that reveals the clone-URL field.
pub const OTHER_REPOSITORY: &str = "Other…";
const AUTO_WORKER: &str = "Auto";
const ERROR_TTL: Duration = Duration::from_secs(5);
const HEADER_PREVIEW: usize = 80;

/// `!Input` keeps the single-letter keys out of the way while a text field has
/// focus; the ⌘ bindings stay live everywhere.
pub fn init(cx: &mut App) {
    let anywhere = Some(CONTEXT);
    let outside_inputs = Some("Lgtm && !Input");
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewTask, anywhere),
        KeyBinding::new("cmd-k", ToggleSearch, anywhere),
        KeyBinding::new("cmd-enter", Submit, anywhere),
        KeyBinding::new("j", SelectNext, outside_inputs),
        KeyBinding::new("k", SelectPrev, outside_inputs),
        KeyBinding::new("1", ShowActivity, outside_inputs),
        KeyBinding::new("2", ShowChanges, outside_inputs),
        KeyBinding::new("3", ShowChecks, outside_inputs),
        KeyBinding::new("4", ShowPlan, outside_inputs),
        KeyBinding::new("v", crate::review::MarkViewed, outside_inputs),
        KeyBinding::new("n", crate::review::NextFile, outside_inputs),
        KeyBinding::new("p", crate::review::PrevFile, outside_inputs),
        KeyBinding::new("s", crate::review::ToggleDiffStyle, outside_inputs),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Activity,
    Changes,
    Checks,
    Plan,
}

pub struct LgtmApp {
    pub client: Client,
    pub tx: net::Sender,
    pub tasks: Vec<Task>,
    pub workers: Vec<WorkerStatus>,
    pub batches: Vec<Batch>,
    pub selected: Option<String>,
    pub pane: Pane,
    pub review: crate::review::ReviewState,

    pub orchestrator: String,
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
    pub search: Entity<InputState>,
    pub repo_select: Entity<SelectState<Vec<String>>>,
    pub worker_select: Entity<SelectState<Vec<String>>>,
    repo_options: Vec<String>,
    worker_options: Vec<String>,
    pub plan_first: bool,
    pub show_search: bool,
    pub show_settings: bool,
    pub show_follow_up: bool,
    pub batches_only: bool,
    pub task_scroll: ScrollHandle,
    pub content_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
    _pump: GpuiTask<()>,
}

impl LgtmApp {
    pub fn new(
        client: Client,
        orchestrator: String,
        token_source: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        net::poll(client.clone(), tx.clone());

        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .auto_grow(3, 8)
                .placeholder("Describe the change…")
        });
        let repo_url = field("https://github.com/you/repo.git", window, cx);
        let base_branch = cx.new(|cx| InputState::new(window, cx).default_value("main"));
        let follow_up = field("Ask for a change…", window, cx);
        let search = field("Filter tasks", window, cx);
        // Both pickers start with only their fallback entry selected, so an
        // orchestrator that never answers still leaves a usable composer.
        let repo_options = vec![OTHER_REPOSITORY.to_string()];
        let repo_select = cx.new(|cx| {
            SelectState::new(
                repo_options.clone(),
                Some(gpui_component::IndexPath::default()),
                window,
                cx,
            )
        });
        let worker_options = vec![AUTO_WORKER.to_string()];
        let worker_select = cx.new(|cx| {
            SelectState::new(
                worker_options.clone(),
                Some(gpui_component::IndexPath::default()),
                window,
                cx,
            )
        });

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
            pane: Pane::Activity,
            review: crate::review::ReviewState::new(),
            orchestrator,
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
            search,
            repo_select,
            worker_select,
            repo_options,
            worker_options,
            plan_first: false,
            show_search: false,
            show_settings: false,
            show_follow_up: false,
            batches_only: false,
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
                self.refresh_pickers(window, cx);
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
        }
        cx.notify();
    }

    /// Keeps the repository and worker dropdowns in step with what exists,
    /// without disturbing a choice the user already made.
    fn refresh_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut repos: Vec<String> = Vec::new();
        for task in &self.tasks {
            let slug = sidebar::repo_slug(&task.spec.repository);
            if !repos.contains(&slug) {
                repos.push(slug);
            }
        }
        repos.push(OTHER_REPOSITORY.to_string());
        if repos != self.repo_options {
            let first = repos.first().cloned();
            self.repo_options = repos.clone();
            self.repo_select.update(cx, |state, cx| {
                let keep = state.selected_value().cloned();
                state.set_items(repos, window, cx);
                let value = keep.filter(|v| self.repo_options.contains(v)).or(first);
                match value {
                    Some(value) => state.set_selected_value(&value, window, cx),
                    None => state.set_selected_index(None, window, cx),
                }
            });
        }

        let mut names = vec![AUTO_WORKER.to_string()];
        names.extend(self.workers.iter().map(|w| w.info.name.clone()));
        if names != self.worker_options {
            self.worker_options = names.clone();
            self.worker_select.update(cx, |state, cx| {
                let keep = state
                    .selected_value()
                    .cloned()
                    .filter(|v| names.contains(v))
                    .unwrap_or_else(|| AUTO_WORKER.to_string());
                state.set_items(names, window, cx);
                state.set_selected_value(&keep, window, cx);
            });
        }
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

    pub fn select(&mut self, id: String, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(id.as_str()) {
            return;
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
        if let Some(stream) = self.stream.take() {
            stream.abort();
        }
        self.generation += 1;
        self.selected = None;
        self.lines.clear();
        self.prompt.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_search = !self.show_search;
        if self.show_search {
            self.search.update(cx, |state, cx| state.focus(window, cx));
        } else {
            self.search
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }

    pub fn search_query(&self, cx: &Context<Self>) -> String {
        if self.show_search {
            self.search.read(cx).value().to_string()
        } else {
            String::new()
        }
    }

    pub fn repository_is_other(&self, cx: &Context<Self>) -> bool {
        match self.repo_select.read(cx).selected_value() {
            Some(value) => value == OTHER_REPOSITORY,
            None => true,
        }
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
        let repository = home::chosen_repository(self, cx);
        if prompt.trim().is_empty() || repository.trim().is_empty() {
            self.set_error("A prompt and a repository are required.".into(), cx);
            return;
        }
        let base_branch = self.base_branch.read(cx).value().to_string();
        let worker = self
            .worker_select
            .read(cx)
            .selected_value()
            .cloned()
            .filter(|name| name != AUTO_WORKER);
        let spec = TaskSpec {
            repository,
            base_branch: if base_branch.trim().is_empty() {
                "main".into()
            } else {
                base_branch
            },
            prompt,
            executor: Executor::Claude,
            worker,
            issue: None,
            linear: None,
            kind: if self.plan_first {
                TaskKind::Plan
            } else {
                TaskKind::Run
            },
            parent: None,
            depends_on: vec![],
            batch: None,
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
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewTask, window, cx| this.go_home(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleSearch, window, cx| this.toggle_search(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_action(window, cx)))
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
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.text)
            .font_family(UI_FONT)
            .text_size(px(TEXT_BODY))
            .when(unreachable, |this| {
                this.child(
                    div()
                        .w_full()
                        .px(px(SPACE[2]))
                        .py(px(SPACE[0]))
                        .bg(t.danger)
                        .text_color(t.accent_fg)
                        .text_size(px(TEXT_SECONDARY))
                        .child(format!("Orchestrator unreachable at {}", self.orchestrator)),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar::render_sidebar(self, window, cx))
                    .child(if self.selected.is_some() {
                        panes::task_view(self, window, cx)
                    } else {
                        home::home(self, window, cx)
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lgtm_protocol::TaskSpec;

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
