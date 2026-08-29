use crate::net::{self, Action, Msg};
use crate::panes;
use crate::render::{self, Line};
use crate::sidebar;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    actions, div, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollHandle,
    Styled as _, Subscription, Task as GpuiTask, Window,
};
use gpui_component::input::{InputEvent, InputState};
use gpui_component::ActiveTheme as _;
use lgtm_client::Client;
use lgtm_protocol::{Batch, Executor, Task, TaskKind, TaskSpec, TaskStatus, WorkerStatus};
use std::time::Duration;
use tokio::task::JoinHandle;

actions!(
    lgtm,
    [SelectNext, SelectPrev, ShowActivity, ShowDiff, ShowChecks]
);

pub const CONTEXT: &str = "Lgtm";
const ERROR_TTL: Duration = Duration::from_secs(5);
const PROMPT_PREVIEW: usize = 44;

/// `!Input` keeps j/k/1/2/3 out of the way while a text field has focus.
pub fn init(cx: &mut App) {
    let context = Some("Lgtm && !Input");
    cx.bind_keys([
        KeyBinding::new("j", SelectNext, context),
        KeyBinding::new("k", SelectPrev, context),
        KeyBinding::new("1", ShowActivity, context),
        KeyBinding::new("2", ShowDiff, context),
        KeyBinding::new("3", ShowChecks, context),
    ]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Activity,
    Diff,
    Checks,
    Plan,
}

pub struct LgtmApp {
    pub client: Client,
    pub tx: net::Sender,
    pub focus: FocusHandle,
    pub tasks: Vec<Task>,
    pub workers: Vec<WorkerStatus>,
    pub batches: Vec<Batch>,
    pub banner: Option<String>,
    pub error: Option<String>,
    pub selected: Option<String>,
    /// Bumped on every selection so events from the previous stream are dropped.
    pub generation: u64,
    stream: Option<JoinHandle<()>>,
    pub lines: Vec<Line>,
    pub pane: Pane,
    pub prompt: Entity<InputState>,
    pub repository: Entity<InputState>,
    pub base_branch: Entity<InputState>,
    pub worker: Entity<InputState>,
    pub follow_up: Entity<InputState>,
    pub task_scroll: ScrollHandle,
    pub content_scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
    _pump: GpuiTask<()>,
}

impl LgtmApp {
    pub fn new(client: Client, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        net::poll(client.clone(), tx.clone());

        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(3)
                .placeholder("what should the agent do?")
        });
        let repository = field("https://github.com/you/repo.git", window, cx);
        let base_branch = cx.new(|cx| InputState::new(window, cx).default_value("main"));
        let worker = field("worker (optional)", window, cx);
        let follow_up = field("follow-up", window, cx);

        let subscriptions = vec![cx.subscribe_in(
            &follow_up,
            window,
            |this, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.send_follow_up(window, cx);
                }
            },
        )];

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
            focus,
            tasks: Vec::new(),
            workers: Vec::new(),
            batches: Vec::new(),
            banner: None,
            error: None,
            selected: None,
            generation: 0,
            stream: None,
            lines: Vec::new(),
            pane: Pane::Activity,
            prompt,
            repository,
            base_branch,
            worker,
            follow_up,
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
                self.banner = None;
                if self.selected.is_none() {
                    if let Some(first) = self.tasks.first().map(|t| t.id.clone()) {
                        self.select(first, cx);
                    }
                }
            }
            Msg::Lists(Err(err)) => self.banner = Some(format!("orchestrator unreachable: {err}")),
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

    fn send_follow_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.follow_up.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.follow_up
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.act(Action::Tell(text), cx);
    }

    pub fn submit(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let prompt = self.prompt.read(cx).value().to_string();
        let repository = self.repository.read(cx).value().to_string();
        if prompt.trim().is_empty() || repository.trim().is_empty() {
            self.set_error("prompt and repository are required".into(), cx);
            cx.notify();
            return;
        }
        let base_branch = self.base_branch.read(cx).value().to_string();
        let worker = self.worker.read(cx).value().to_string();
        let spec = TaskSpec {
            repository,
            base_branch: if base_branch.trim().is_empty() {
                "main".into()
            } else {
                base_branch
            },
            prompt,
            executor: Executor::Claude,
            worker: Some(worker).filter(|w| !w.trim().is_empty()),
            issue: None,
            linear: None,
            kind: TaskKind::Run,
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
}

fn field(
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<LgtmApp>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

pub fn prompt_preview(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or("").trim();
    if line.chars().count() > PROMPT_PREVIEW {
        format!("{}…", line.chars().take(PROMPT_PREVIEW).collect::<String>())
    } else {
        line.to_string()
    }
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
        let banner = self.banner.clone();
        div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
            .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_selection(-1, cx)))
            .on_action(cx.listener(|this, _: &ShowActivity, _, cx| {
                this.pane = Pane::Activity;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowDiff, _, cx| {
                this.pane = Pane::Diff;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ShowChecks, _, cx| {
                this.pane = Pane::Checks;
                cx.notify();
            }))
            .size_full()
            .flex()
            .flex_col()
            .when_some(banner, |this, text| {
                this.child(
                    div()
                        .w_full()
                        .px_2()
                        .py_1()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .text_sm()
                        .child(text),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar::render_sidebar(self, cx))
                    .child(panes::render_main(self, window, cx)),
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
    fn queued_task_with_no_dependencies_is_queued() {
        let queued = task("q", TaskStatus::Queued, vec![]);
        assert_eq!(
            status_label(&queued, std::slice::from_ref(&queued)),
            "queued"
        );
    }

    #[test]
    fn assigned_worker_is_not_blocked_even_with_unmet_dependency() {
        let dep = task("dep", TaskStatus::Running, vec![]);
        let mut queued = task("q", TaskStatus::Queued, vec!["dep"]);
        queued.worker = Some("compute".into());
        assert_eq!(status_label(&queued, &[dep, queued.clone()]), "queued");
    }
}
