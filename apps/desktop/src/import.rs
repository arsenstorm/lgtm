//! The import dialog: turn a GitHub label or a Linear state into a batch.

use crate::app::LgtmApp;
use crate::net;
use crate::theme::{
    field, icon_button, panel, scrim, section_label, tokens, Tokens, HEADER_H, MONO_FONT, RADIUS,
    SPACE, TEXT_MONO, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, relative, AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
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
    pub fn request(&self, dry_run: bool, cx: &App) -> Option<BatchRequest> {
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
            dry_run,
        })
    }
}

pub fn modal(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let form = &app.import;
    let github = form.source == Source::Github;
    scrim("import-scrim", &t)
        .pt(relative(0.1))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            panel(&t)
                .id("import")
                .w(px(WIDTH))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(HEADER_H))
                        .px(px(SPACE[2]))
                        .border_b_1()
                        .border_color(t.border)
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Import a batch"),
                        )
                        .child(
                            icon_button("import-close", "x", true, &t).on_click(cx.listener(
                                |this, _: &ClickEvent, window, cx| this.close_overlay(window, cx),
                            )),
                        ),
                )
                .child(
                    div()
                        .id("import-body")
                        .flex()
                        .flex_col()
                        .gap(px(SPACE[2]))
                        .max_h(px(460.))
                        .overflow_y_scroll()
                        .p(px(SPACE[2]))
                        .child(div().flex().gap(px(SPACE[0])).children(
                            [(Source::Github, "GitHub"), (Source::Linear, "Linear")].map(
                                |(source, label)| {
                                    Button::new(SharedString::from(format!("source-{label}")))
                                        .label(label)
                                        .xsmall()
                                        .ghost()
                                        .selected(form.source == source)
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _, cx| {
                                                this.import.source = source;
                                                this.import.issues.clear();
                                                cx.notify();
                                            },
                                        ))
                                },
                            ),
                        ))
                        .when(github, |this| {
                            this.child(row("Owner", &form.owner, &t))
                                .child(row("Repository", &form.repo, &t))
                                .child(row("Label", &form.label, &t))
                        })
                        .when(!github, |this| {
                            this.child(row("Team", &form.team, &t))
                                .child(row("State", &form.state, &t))
                                .child(row("Repository", &form.repository, &t))
                        })
                        .child(row("Base branch", &form.base, &t))
                        .child(row("Max", &form.max, &t))
                        .child(
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
                                ),
                        )
                        .when(!form.issues.is_empty(), |this| {
                            this.child(preview(&form.issues, &t))
                        }),
                )
                .child(
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
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| send(this, true, cx)),
                                ),
                        )
                        .child(
                            Button::new("import")
                                .label("Import")
                                .primary()
                                .small()
                                .on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        send(this, false, cx)
                                    }),
                                ),
                        ),
                ),
        )
        .into_any_element()
}

fn send(app: &mut LgtmApp, dry_run: bool, cx: &mut Context<LgtmApp>) {
    let Some(request) = app.import.request(dry_run, cx) else {
        return;
    };
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
