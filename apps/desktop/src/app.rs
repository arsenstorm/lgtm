//! The root view: state, key bindings, and the window bar / sidebar / main split.

use crate::home::Chip;
use crate::import::ImportForm;
use crate::keys::{
    CloseOverlay, NewTask, OpenPalette, PaletteNext, PalettePrev, PaletteRun, SelectNext,
    SelectPrev, ShowActivity, ShowChanges, ShowChecks, ShowPlan, Submit, ToggleSidebar, CONTEXT,
};
use crate::net;
use crate::render::Line;
use crate::theme::{tokens, SPACE, STATUS_H, TEXT_BODY, TEXT_SECONDARY, UI_FONT};
use crate::{batches, home, import, palette, panes, settings, sidebar, titlebar};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext as _, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, ScrollHandle, Styled as _,
    Subscription, Task as GpuiTask, Window,
};
use gpui_component::input::{InputEvent, InputState};
use lgtm_client::Client;
use lgtm_protocol::{Batch, Task, WorkerStatus};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;

pub(crate) const ERROR_TTL: Duration = Duration::from_secs(5);
/// How long a hosted orchestrator is given to come up before the banner stops
/// saying it is starting and starts complaining.
pub(crate) const STARTING: Duration = Duration::from_secs(5);

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
    /// The `embedded_orchestrator` preference; the toggle in Settings writes it.
    pub embedded: bool,
    /// This process is running the orchestrator.
    pub hosted: bool,
    /// Join line for other machines when hosted.
    pub join: Option<String>,
    /// When the window opened, for the "starting" grace period on the banner.
    pub(crate) started: Instant,
    pub reachable: bool,
    pub error: Option<String>,
    pub focus: FocusHandle,
    /// Bumped on every selection so events from the previous stream are dropped.
    pub generation: u64,
    pub(crate) stream: Option<JoinHandle<()>>,
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
    /// The `+` menu is showing its base-branch field.
    pub branch_edit: bool,
    pub worker_menu: bool,

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
    pub fn new(config: crate::Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let client = Client::new(config.orchestrator.clone(), config.token.clone());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        net::poll(client.clone(), tx.clone());

        let prompt = cx.new(|cx| InputState::new(window, cx).multi_line(true).auto_grow(2, 8));
        let repo_url = field("https://github.com/you/repo.git", window, cx);
        let base_branch = cx.new(|cx| InputState::new(window, cx).default_value("main"));
        let follow_up = field("Ask for a change…", window, cx);
        let query = field("Search tasks, repositories, actions…", window, cx);
        let import = ImportForm::new(window, cx);

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
        let mut app = Self {
            client,
            tx,
            tasks: Vec::new(),
            workers: Vec::new(),
            batches: Vec::new(),
            selected: None,
            page: Page::Home,
            pane: Pane::Activity,
            review: crate::review::ReviewState::new(),
            orchestrator: config.orchestrator,
            token: config.token,
            token_source: config.token_source,
            embedded: config.embedded,
            hosted: config.hosted,
            join: config.join,
            started: Instant::now(),
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
            branch_edit: false,
            worker_menu: false,
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
            _subscriptions: Vec::new(),
            _pump: pump,
        };
        app._subscriptions = app.subscribe(window, cx);
        app
    }

    fn subscribe(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<Subscription> {
        vec![
            cx.subscribe_in(
                &self.follow_up,
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
                &self.prompt,
                window,
                |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { secondary: true }) {
                        this.submit(window, cx);
                    }
                },
            ),
            cx.subscribe_in(&self.query, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.palette_at = 0;
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                crate::theme::apply(Some(window), cx);
                cx.notify();
            }),
        ]
    }
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
    root.on_action(cx.listener(|this, _: &NewTask, window, cx| this.go_home(window, cx)))
        .on_action(cx.listener(|this, _: &OpenPalette, window, cx| this.open_palette(window, cx)))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
        .on_action(cx.listener(|this, _: &Submit, window, cx| this.submit_action(window, cx)))
        .on_action(cx.listener(|this, _: &CloseOverlay, window, cx| this.close_overlay(window, cx)))
        .on_action(cx.listener(|this, _: &PaletteNext, _, cx| palette::step(this, 1, cx)))
        .on_action(cx.listener(|this, _: &PalettePrev, _, cx| palette::step(this, -1, cx)))
        .on_action(cx.listener(|this, _: &PaletteRun, window, cx| palette::run(this, window, cx)))
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
}

impl LgtmApp {
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
        let unreachable = !self.reachable;
        let overlay = self.overlay;
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
