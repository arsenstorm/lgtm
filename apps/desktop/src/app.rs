//! The root view: state, key bindings, and the window bar / sidebar / main split.

use crate::home::Chip;
use crate::import::ImportForm;
use crate::keys::{
    CloseOverlay, NewSession, OpenPalette, PaletteNext, PalettePrev, PaletteRun, SelectNext,
    SelectPrev, ShowActivity, ShowChanges, ShowNotes, ShowOverview, ShowPlan, ShowReview, Submit,
    ToggleSidebar, CONTEXT,
};
use crate::net::{self, Msg};
use crate::project::ProjectTab;
use crate::render::Line;
use crate::theme::{tokens, SPACE, STATUS_H, TEXT_BODY, TEXT_SECONDARY, UI_FONT};
use crate::{batches, home, import, palette, panes, project, session, settings, sidebar, titlebar};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, Styled as _,
    Subscription, Task as GpuiTask, Window,
};
use gpui_component::input::{InputEvent, InputState};
use lgtm_client::Client;
use lgtm_protocol::{
    Batch, GoalSummary, Memory, Overlap, PlanVersion, RunnerStatus, Session, SessionDetail, Stats,
    StoredEvent, Task, Todo,
};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

pub(crate) const ERROR_TTL: Duration = Duration::from_secs(5);
/// How long a hosted orchestrator is given to come up before the banner stops
/// saying it is starting and starts complaining.
pub(crate) const STARTING: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Overview,
    Activity,
    Changes,
    Review,
    Notes,
    Terminal,
    Plan,
}

/// What the main area shows when no task is selected.
#[derive(Clone, PartialEq, Eq)]
pub enum Page {
    Home,
    Batches,
    /// One repository, by the slug the sidebar groups its sessions under.
    Project(String),
    /// One chat thread, by session id.
    Session(String),
}

/// One entry in the back/forward history: a task, or a page.
#[derive(Clone, PartialEq, Eq)]
pub enum Stop {
    Task(String),
    Page(Page),
}

/// The one modal that can be up at a time.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    None,
    Palette,
    Settings,
    Import,
}

/// An attached shell: what it has printed, and the way to type at it.
pub struct Shell {
    pub output: String,
    pub input: tokio::sync::mpsc::UnboundedSender<String>,
    pub stream: JoinHandle<()>,
}

/// The orchestrator this window talks to, and how the talking is going.
pub struct Connection {
    pub orchestrator: String,
    pub token: String,
    pub token_source: &'static str,
    /// The `embedded_orchestrator` preference; the toggle in Settings writes it.
    pub embedded: bool,
    /// This process is running the orchestrator.
    pub hosted: bool,
    /// Join line for other machines when hosted.
    pub join: Option<String>,
    /// When the window opened, for the "starting" grace period on the banner.
    pub started: Instant,
    pub reachable: bool,
}

impl Connection {
    fn new(config: crate::Config) -> Self {
        Connection {
            orchestrator: config.orchestrator,
            token: config.token,
            token_source: config.token_source,
            embedded: config.embedded,
            hosted: config.hosted,
            join: config.join,
            started: Instant::now(),
            reachable: false,
        }
    }
}

/// Every text field the window owns.
pub struct Inputs {
    pub prompt: Entity<InputState>,
    pub repo_url: Entity<InputState>,
    pub base_branch: Entity<InputState>,
    pub follow_up: Entity<InputState>,
    pub query: Entity<InputState>,
    pub notes: Entity<InputState>,
    pub memory: Entity<InputState>,
    pub todo: Entity<InputState>,
    pub model: Entity<InputState>,
    pub shell: Entity<InputState>,
}

impl Inputs {
    fn new(window: &mut Window, cx: &mut Context<LgtmApp>) -> Self {
        Inputs {
            prompt: cx.new(|cx| InputState::new(window, cx).multi_line(true).auto_grow(2, 8)),
            repo_url: field("https://github.com/you/repo.git", window, cx),
            base_branch: cx.new(|cx| InputState::new(window, cx).default_value("main")),
            follow_up: field("Ask for a change…", window, cx),
            query: field("Search tasks, repositories, actions…", window, cx),
            notes: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .auto_grow(4, 16)
            }),
            memory: field("Something every run here should know…", window, cx),
            todo: field("Something to do later…", window, cx),
            model: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("the harness default")
                    .default_value(crate::theme::models(cx).model)
            }),
            shell: field("Type a command, then ↩", window, cx),
        }
    }
}

/// What the composer holds and which of its menus is open.
#[derive(Default)]
pub struct ComposerState {
    /// The repository the composer will use, as a clone URL.
    pub project: Option<String>,
    pub chips: Vec<Chip>,
    pub project_menu: bool,
    pub add_repo: bool,
    pub plus_menu: bool,
    /// The `+` menu is showing its base-branch field.
    pub branch_edit: bool,
    pub runner_menu: bool,
}

/// The chrome's own state: what is open, unfolded, scrolled, or remembered.
pub struct UiState {
    pub overlay: Overlay,
    pub palette_at: usize,
    /// Batches, and projects, whose rows are unfolded past the first few.
    pub expanded: HashSet<String>,
    /// Project slugs whose session list is folded away.
    pub collapsed: HashSet<String>,
    pub settings_scroll: ScrollHandle,
    pub sidebar_open: bool,
    /// Where the window has been, and where in that list we are.
    pub visited: Vec<Stop>,
    pub visited_at: usize,
    pub show_follow_up: bool,
    /// The Notes tab is showing its editor rather than the stored notes.
    pub editing_notes: bool,
    pub dragging: bool,
    pub task_scroll: ScrollHandle,
    pub content_scroll: ScrollHandle,
    pub project_scroll: ScrollHandle,
    pub session_scroll: ScrollHandle,
    pub project_tab: ProjectTab,
    /// The titlebar's runner popover.
    pub runner_menu: bool,
    /// Which Settings dropdown is open, by its id.
    pub settings_menu: Option<&'static str>,
    /// `<task>:<host>` for every request denied in this window. Denying is
    /// local: the orchestrator has no endpoint for it (see `review_tab`).
    pub denied: HashSet<String>,
    pub terminal_scroll: ScrollHandle,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            overlay: Overlay::None,
            palette_at: 0,
            expanded: HashSet::new(),
            collapsed: HashSet::new(),
            settings_scroll: ScrollHandle::new(),
            sidebar_open: true,
            visited: Vec::new(),
            visited_at: 0,
            show_follow_up: false,
            editing_notes: false,
            dragging: false,
            task_scroll: ScrollHandle::new(),
            content_scroll: ScrollHandle::new(),
            project_scroll: ScrollHandle::new(),
            session_scroll: ScrollHandle::new(),
            project_tab: ProjectTab::default(),
            runner_menu: false,
            settings_menu: None,
            denied: HashSet::new(),
            terminal_scroll: ScrollHandle::new(),
        }
    }
}

pub struct LgtmApp {
    pub client: Client,
    pub tx: net::Sender,
    pub tasks: Vec<Task>,
    pub runners: Vec<RunnerStatus>,
    pub batches: Vec<Batch>,
    pub goals: Vec<GoalSummary>,
    pub sessions: Vec<Session>,
    /// The open session, refreshed on the same tick as the lists.
    pub session: Option<SessionDetail>,
    /// Events per task of the open session, for the tasks whose detail has
    /// been fetched. Kept after a task finishes so its turns do not vanish.
    pub session_events: Vec<(String, Vec<StoredEvent>)>,
    /// The open project's memories and todos, while one of those tabs is open.
    pub memories: Vec<Memory>,
    pub todos: Vec<Todo>,
    /// Every plan version under the open project's goals, newest first.
    pub plans: Vec<PlanVersion>,
    /// The todo the composer was prefilled from, promoted on submit.
    pub promoting: Option<String>,
    /// Orchestrator-wide, and only refreshed on one poll in ten.
    pub stats: Option<Stats>,
    pub selected: Option<String>,
    pub page: Page,
    pub pane: Pane,
    pub review: crate::review::ReviewState,

    pub link: Connection,
    pub error: Option<String>,
    pub focus: FocusHandle,
    /// Bumped on every selection so events from the previous stream are dropped.
    pub generation: u64,
    pub(crate) stream: Option<JoinHandle<()>>,
    /// Polls the open project's lists, or fetches its plans.
    pub(crate) project_stream: Option<JoinHandle<()>>,
    /// The open task's shell, while the Terminal tab is showing it.
    pub(crate) shell: Option<Shell>,
    pub lines: Vec<Line>,
    /// The selected task's events, kept beside the rendered lines because the
    /// Overview and Review tabs read fields the lines have already flattened.
    pub events: Vec<StoredEvent>,
    /// Live tasks touching the same files, as the detail call reported them.
    pub overlaps: Vec<Overlap>,

    pub inputs: Inputs,
    pub import: ImportForm,
    pub composer: ComposerState,

    pub ui: UiState,
    _subscriptions: Vec<Subscription>,
    _pump: GpuiTask<()>,
}

impl LgtmApp {
    pub fn new(config: crate::Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let client = Client::new(config.orchestrator.clone(), config.token.clone());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        net::poll(client.clone(), tx.clone());

        let pump = pump(rx, cx);

        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut app = Self {
            client,
            tx,
            tasks: Vec::new(),
            runners: Vec::new(),
            batches: Vec::new(),
            goals: Vec::new(),
            sessions: Vec::new(),
            session: None,
            session_events: Vec::new(),
            memories: Vec::new(),
            todos: Vec::new(),
            plans: Vec::new(),
            promoting: None,
            stats: None,
            selected: None,
            page: Page::Home,
            pane: Pane::Overview,
            review: crate::review::ReviewState::new(),
            link: Connection::new(config),
            error: None,
            focus,
            generation: 0,
            stream: None,
            project_stream: None,
            shell: None,
            lines: Vec::new(),
            events: Vec::new(),
            overlaps: Vec::new(),
            inputs: Inputs::new(window, cx),
            import: ImportForm::new(window, cx),
            composer: ComposerState::default(),
            ui: UiState::default(),
            _subscriptions: Vec::new(),
            _pump: pump,
        };
        app._subscriptions = app.subscribe(window, cx);
        app
    }

    fn subscribe(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Subscription> {
        vec![
            cx.subscribe_in(
                &self.inputs.follow_up,
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
                &self.inputs.prompt,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { secondary: true }) {
                        this.submit(window, cx);
                    }
                },
            ),
            // Saved when the field is done being typed in, not per keystroke:
            // every save rewrites `~/.lgtm/desktop.toml`.
            cx.subscribe_in(
                &self.inputs.model,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                        this.save_model(cx);
                    }
                },
            ),
            cx.subscribe_in(
                &self.inputs.shell,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.send_to_shell(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &self.inputs.memory,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.add_memory(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &self.inputs.todo,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.add_todo(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &self.inputs.query,
                window,
                |this, _, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.ui.palette_at = 0;
                        cx.notify();
                    }
                },
            ),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::theme::apply(Some(window), cx);
                cx.notify();
            }),
        ]
    }
}

/// Feeds network messages into the view until it is gone.
fn pump(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Msg>,
    cx: &mut Context<LgtmApp>,
) -> GpuiTask<()> {
    cx.spawn(async move |this, cx| {
        while let Some(msg) = rx.recv().await {
            if this.update(cx, |this, cx| this.apply(msg, cx)).is_err() {
                return;
            }
        }
    })
}

fn field(
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut Context<LgtmApp>,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

/// Every keyboard action the window answers to.
fn bind_actions(root: Div, cx: &mut Context<LgtmApp>) -> Div {
    root.on_action(cx.listener(|this, _: &NewSession, window, cx| this.go_home(window, cx)))
        .on_action(cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
        .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_action(window, cx)))
        .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| this.close_overlay(window, cx)))
        .on_action(cx.listener(|this, _: &PaletteNext, _, cx| palette::step(this, 1, cx)))
        .on_action(cx.listener(|this, _: &PalettePrev, _, cx| palette::step(this, -1, cx)))
        .on_action(cx.listener(|this, _: &PaletteRun, window, cx| palette::run(this, window, cx)))
        .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.move_selection(1, cx)))
        .on_action(cx.listener(|this, _: &SelectPrev, _, cx| this.move_selection(-1, cx)))
        .on_action(cx.listener(|this, _: &ShowOverview, _, cx| this.show(Pane::Overview, cx)))
        .on_action(cx.listener(|this, _: &ShowActivity, _, cx| this.show(Pane::Activity, cx)))
        .on_action(cx.listener(|this, _: &ShowChanges, _, cx| this.show(Pane::Changes, cx)))
        .on_action(cx.listener(|this, _: &ShowReview, _, cx| this.show(Pane::Review, cx)))
        .on_action(cx.listener(|this, _: &ShowNotes, _, cx| this.show(Pane::Notes, cx)))
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
}

impl LgtmApp {
    /// The sidebar and whatever the selection or page puts beside it.
    fn main_area(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .when(self.ui.sidebar_open, |this| {
                this.child(sidebar::render_sidebar(self, window, cx))
            })
            .child(self.main_body(window, cx))
    }

    /// The selected task, or whatever page is showing instead.
    fn main_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.selected.is_some() {
            return panes::task_view(self, window, cx);
        }
        match self.page.clone() {
            Page::Batches => batches::page(self, cx),
            Page::Project(slug) => project::page(self, &slug, cx),
            Page::Session(_) => session::page(self, window, cx),
            Page::Home => home::home(self, window, cx),
        }
    }

    /// `bg-destructive/10 text-destructive`, the reference's destructive
    /// fill — a loud strip would own the window.
    fn unreachable_strip(&self, t: &crate::theme::Tokens) -> Div {
        div()
            .w_full()
            .flex()
            .flex_shrink_0()
            .items_center()
            .h(px(STATUS_H))
            .px(px(SPACE[2]))
            .bg(gpui::Hsla {
                a: 0.12,
                ..t.danger
            })
            .text_color(t.danger)
            .text_size(px(TEXT_SECONDARY))
            .child(self.banner())
    }
}

impl Focusable for LgtmApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LgtmApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let unreachable = !self.link.reachable;
        let overlay = self.ui.overlay;
        bind_actions(div(), cx)
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.bg)
            .text_color(t.fg)
            .font_family(UI_FONT)
            .text_size(px(TEXT_BODY))
            .child(titlebar::bar(self, cx))
            .when(unreachable, |this| this.child(self.unreachable_strip(&t)))
            .child(self.main_area(window, cx))
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
