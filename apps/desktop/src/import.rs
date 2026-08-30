//! The import dialog: turn a GitHub label or a Linear state into a batch.

use crate::app::LgtmApp;
use crate::net;
use crate::theme::{
    field, modal_header, panel, scrim, section_label, tokens, Tokens, MONO_FONT, RADIUS, SPACE,
    TEXT_MONO, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, relative, AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::switch::Switch;
use gpui_component::{Selectable as _, Sizable as _};
use lgtm_client::{BatchRequest, IssuePreview};
use lgtm_protocol::{BatchSource, Executor};

const WIDTH: f32 = 520.;
const DEFAULT_MAX: u32 = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Github,
    Linear,
}

pub struct ImportForm {
    pub source: Source,
    pub owner: Entity<InputState>,
    pub repo: Entity<InputState>,
    pub label: Entity<InputState>,
    pub team: Entity<InputState>,
    pub state: Entity<InputState>,
    pub repository: Entity<InputState>,
    pub base: Entity<InputState>,
    pub max: Entity<InputState>,
    pub plan: bool,
    pub approve: bool,
    /// What the last dry run found.
    pub issues: Vec<IssuePreview>,
}

impl ImportForm {
    pub fn new(window: &mut Window, cx: &mut Context<LgtmApp>) -> Self {
        let text = |placeholder: &'static str, window: &mut Window, cx: &mut Context<LgtmApp>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let with = |value: &'static str, window: &mut Window, cx: &mut Context<LgtmApp>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        Self {
            source: Source::Github,
            owner: text("owner", window, cx),
            repo: text("repo", window, cx),
            label: text("agent", window, cx),
            team: text("ENG", window, cx),
            state: text("Todo", window, cx),
            repository: text("https://github.com/you/repo.git", window, cx),
            base: with("main", window, cx),
            max: with("20", window, cx),
            plan: false,
            approve: false,
            issues: Vec::new(),
        }
    }

    /// The request the fields describe, or None while one is still empty.
    pub fn request(&self, cx: &App) -> Option<BatchRequest> {
        let read = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let filled = |value: String| (!value.is_empty()).then_some(value);
        let (source, repository) = match self.source {
            Source::Github => (
                BatchSource::GithubLabel {
                    owner: filled(read(&self.owner))?,
                    repo: filled(read(&self.repo))?,
                    label: filled(read(&self.label))?,
                },
                None,
            ),
            Source::Linear => (
                BatchSource::Linear {
                    team: filled(read(&self.team))?,
                    state: filled(read(&self.state))?,
                },
                Some(filled(read(&self.repository))?),
            ),
        };
        Some(BatchRequest {
            source,
            repository,
            base_branch: filled(read(&self.base)).unwrap_or_else(|| "main".into()),
            executor: Executor::Claude,
            worker: None,
            plan: self.plan,
            approve_plans: self.approve,
            max: read(&self.max).parse().unwrap_or(DEFAULT_MAX),
            dry_run: false,
            sandbox: None,
        })
    }
}

pub fn modal(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let form = &app.import;
    scrim("import-scrim", &t)
        .pt(relative(0.1))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            panel(&t)
                .id("import")
                .w(px(WIDTH))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(modal_header("Import a batch", "import-close", &t, cx))
                .child(
                    div()
                        .id("import-body")
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[2]))
                        .max_h(px(460.))
                        .overflow_y_scroll()
                        .p(px(SPACE[2]))
                        .child(source_buttons(form, cx))
                        .children(fields(form, &t))
                        .child(switches(form, cx))
                        .when(!form.issues.is_empty(), |this| {
                            this.child(preview(&form.issues, &t))
                        }),
                )
                .child(footer(&t, cx)),
        )
        .into_any_element()
}

fn source_buttons(form: &ImportForm, cx: &mut Context<LgtmApp>) -> Div {
    div().flex().gap(px(SPACE[0])).children(
        [(Source::Github, "GitHub"), (Source::Linear, "Linear")].map(|(source, label)| {
            Button::new(SharedString::from(format!("source-{label}")))
                .label(label)
                .xsmall()
                .ghost()
                .selected(form.source == source)
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.import.source = source;
                    this.import.issues.clear();
                    cx.notify();
                }))
        }),
    )
}

/// The source's own fields, then the two every source shares.
fn fields(form: &ImportForm, t: &Tokens) -> Vec<Div> {
    let own: [(&'static str, &Entity<InputState>); 3] = match form.source {
        Source::Github => [
            ("Owner", &form.owner),
            ("Repository", &form.repo),
            ("Label", &form.label),
        ],
        Source::Linear => [
            ("Team", &form.team),
            ("State", &form.state),
            ("Repository", &form.repository),
        ],
    };
    own.into_iter()
        .chain([("Base branch", &form.base), ("Max", &form.max)])
        .map(|(label, input)| row(label, input, t))
        .collect()
}

fn switches(form: &ImportForm, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[3]))
        .child(
            Switch::new("plan-first")
                .label("Plan first")
                .checked(form.plan)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    this.import.plan = *checked;
                    cx.notify();
                })),
        )
        .child(
            Switch::new("approve-plans")
                .label("Approve plans")
                .checked(form.approve)
                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                    this.import.approve = *checked;
                    cx.notify();
                })),
        )
}

fn footer(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(SPACE[1]))
        .p(px(SPACE[2]))
        .border_t_1()
        .border_color(t.border)
        .child(
            Button::new("dry-run")
                .label("Dry run")
                .outline()
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    send(this, cx, |request| request.dry_run = true)
                })),
        )
        .child(
            Button::new("import")
                .label("Import")
                .primary()
                .small()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| send(this, cx, |_| {}))),
        )
}

/// Posts the form, after `adjust` has its say (the dry run sets its flag).
fn send(app: &mut LgtmApp, cx: &mut Context<LgtmApp>, adjust: impl FnOnce(&mut BatchRequest)) {
    let Some(mut request) = app.import.request(cx) else {
        return;
    };
    adjust(&mut request);
    net::create_batch(app.client.clone(), request, app.tx.clone());
    cx.notify();
}

fn row(label: &'static str, input: &Entity<InputState>, t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(
            div()
                .w(px(110.))
                .flex_shrink_0()
                .text_color(t.muted_fg)
                .child(label),
        )
        .child(div().flex_1().child(field(input, t).small()))
}

fn preview(issues: &[IssuePreview], t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .child(section_label("Found", t))
        .child(
            div()
                .flex()
                .flex_col()
                .rounded(px(RADIUS))
                .bg(t.muted)
                .p(px(SPACE[1]))
                .font_family(MONO_FONT)
                .text_size(px(TEXT_MONO))
                .children(issues.iter().map(|issue| {
                    div()
                        .flex()
                        .gap(px(SPACE[1]))
                        .child(div().text_color(t.muted_fg).child(issue.key.clone()))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(issue.title.clone()),
                        )
                })),
        )
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(format!("{} issues", issues.len())),
        )
}
