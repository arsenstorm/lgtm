//! The composer: the project chip, the prompt card, and the `+` menu.

use crate::app::LgtmApp;
use crate::home::{chip_pill, send_button, Chip, PlusView, AUTO_WORKER};
use crate::sidebar::repo_slug;
use crate::theme::{field, tokens, Tokens, RADIUS, ROW_H, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, ClickEvent, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::popover::Popover;
use gpui_component::Sizable as _;

const MENU_W: f32 = 300.;

pub fn project_chip(app: &LgtmApp, _t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let this = cx.entity();
    let label = match app.project.as_deref() {
        Some(url) => format!("📁 {}", repo_slug(url)),
        None => "📁 Choose project".to_string(),
    };
    Popover::new("project-menu")
        .open(app.project_menu)
        .on_open_change({
            let this = this.clone();
            move |open, _, cx| {
                let open = *open;
                this.update(cx, |app, cx| {
                    app.project_menu = open;
                    app.add_repo = false;
                    cx.notify();
                });
            }
        })
        .trigger(Button::new("project-trigger").label(label).ghost().small())
        .content(move |_, window, cx| project_menu(&this, window, cx))
}

fn project_menu(this: &Entity<LgtmApp>, _window: &mut Window, cx: &mut App) -> Div {
    let t = tokens(cx);
    let app = this.read(cx);
    let repos = app.known_repositories();
    let chosen = app.project.clone();
    let adding = app.add_repo;
    let repo_url = app.repo_url.clone();

    div()
        .w(px(MENU_W))
        .flex()
        .flex_col()
        .gap(px(2.))
        .children(repos.into_iter().map(|url| {
            let picked = chosen.as_deref() == Some(url.as_str());
            let this = this.clone();
            let value = url.clone();
            menu_row(SharedString::from(format!("repo-{url}")), picked, &t)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().truncate().child(repo_slug(&url)))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(TEXT_SECONDARY))
                                .text_color(t.muted_fg)
                                .child(url.clone()),
                        ),
                )
                .h_auto()
                .py(px(SPACE[0]))
                .on_click(move |_, _, cx| {
                    this.update(cx, |app, cx| {
                        app.project = Some(value.clone());
                        app.project_menu = false;
                        cx.notify();
                    });
                })
        }))
        .child({
            let this = this.clone();
            menu_row("add-repo", false, &t)
                .child("Add repository…")
                .on_click(move |_, window, cx| {
                    this.update(cx, |app, cx| {
                        app.add_repo = true;
                        app.repo_url.update(cx, |state, cx| state.focus(window, cx));
                        cx.notify();
                    });
                })
        })
        .when(adding, |menu| {
            let this = this.clone();
            menu.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(SPACE[0]))
                    .pt(px(SPACE[0]))
                    .child(div().flex_1().child(field(&repo_url, &t).small()))
                    .child(
                        Button::new("add-repo-ok")
                            .label("Add")
                            .primary()
                            .small()
                            .on_click(move |_: &ClickEvent, _, cx| {
                                this.update(cx, |app, cx| {
                                    let url = app.repo_url.read(cx).value().trim().to_string();
                                    if url.is_empty() {
                                        return;
                                    }
                                    app.project = Some(url);
                                    app.project_menu = false;
                                    app.add_repo = false;
                                    cx.notify();
                                });
                            }),
                    ),
            )
        })
}

pub fn card(app: &LgtmApp, t: &Tokens, _window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
    let ready = !app.prompt.read(cx).value().trim().is_empty() && app.project.is_some();
    let chips: Vec<Chip> = app.chips.clone();
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .p(px(SPACE[2]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(gpui_component::input::Input::new(&app.prompt).appearance(false))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[0]))
                .child(plus_menu(app, cx))
                .children(chips.iter().map(|chip| chip_pill(chip, t, cx)))
                .child(div().flex_1())
                .child(send_button(ready, t, cx)),
        )
}

fn plus_menu(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let this = cx.entity();
    Popover::new("plus-menu")
        .open(app.plus_menu)
        .on_open_change({
            let this = this.clone();
            move |open, _, cx| {
                let open = *open;
                this.update(cx, |app, cx| {
                    app.plus_menu = open;
                    app.plus_view = PlusView::Root;
                    cx.notify();
                });
            }
        })
        .trigger(Button::new("plus-trigger").label("＋").ghost().small())
        .content(move |_, window, cx| plus_content(&this, window, cx))
}

fn plus_content(this: &Entity<LgtmApp>, _window: &mut Window, cx: &mut App) -> Div {
    let t = tokens(cx);
    let app = this.read(cx);
    let view = app.plus_view;
    let planning = app.chips.contains(&Chip::Plan);
    let mut workers: Vec<String> = vec![AUTO_WORKER.to_string()];
    workers.extend(app.workers.iter().map(|worker| worker.info.name.clone()));
    let branch = app.base_branch.clone();

    let menu = div().w(px(MENU_W)).flex().flex_col().gap(px(2.));
    match view {
        PlusView::Root => menu
            .child({
                let this = this.clone();
                menu_row("plan", planning, &t)
                    .child("Plan")
                    .on_click(move |_, _, cx| {
                        this.update(cx, |app, cx| {
                            app.set_chip(Chip::Plan, cx);
                            app.plus_menu = false;
                        });
                    })
            })
            .child({
                let this = this.clone();
                menu_row("workers", false, &t)
                    .child("Worker…")
                    .on_click(move |_, _, cx| {
                        this.update(cx, |app, cx| {
                            app.plus_view = PlusView::Workers;
                            cx.notify();
                        });
                    })
            })
            .child({
                let this = this.clone();
                menu_row("branch", false, &t)
                    .child("Branch…")
                    .on_click(move |_, window, cx| {
                        this.update(cx, |app, cx| {
                            app.plus_view = PlusView::Branch;
                            app.base_branch
                                .update(cx, |state, cx| state.focus(window, cx));
                            cx.notify();
                        });
                    })
            }),
        PlusView::Workers => menu.children(workers.into_iter().map(|name| {
            let this = this.clone();
            let chosen = name.clone();
            menu_row(SharedString::from(format!("worker-{name}")), false, &t)
                .child(name)
                .on_click(move |_, _, cx| {
                    this.update(cx, |app, cx| {
                        app.set_chip(Chip::Worker(chosen.clone()), cx);
                        app.plus_menu = false;
                    });
                })
        })),
        PlusView::Branch => menu.child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[0]))
                .child(div().flex_1().child(field(&branch, &t).small()))
                .child({
                    let this = this.clone();
                    Button::new("branch-ok")
                        .label("Set")
                        .primary()
                        .small()
                        .on_click(move |_: &ClickEvent, _, cx| {
                            this.update(cx, |app, cx| {
                                let branch = app.base_branch.read(cx).value().trim().to_string();
                                app.set_chip(Chip::Branch(branch), cx);
                                app.plus_menu = false;
                            });
                        })
                }),
        ),
    }
}

/// One row in a composer popover: the sidebar row, without the key hint.
fn menu_row(id: impl Into<SharedString>, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(ROW_H))
        .px(px(SPACE[1]))
        .rounded(px(8.))
        .cursor_pointer()
        .text_color(if active { t.fg } else { t.muted_fg })
        .when(active, |this| this.bg(t.muted))
        .hover(|this| this.bg(t.muted))
}
