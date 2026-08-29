//! The composer: the project bar, the prompt card, and the menus that hang off
//! them. Pinned to the bottom of the main area, like Codex.

use crate::app::LgtmApp;
use crate::home::{Chip, AUTO_WORKER};
use crate::sidebar::repo_slug;
use crate::theme::{
    field, icon, icon_button, lighten, Tokens, ICON, RADIUS, SPACE, TEXT_BODY, TEXT_ROW,
    TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, ClickEvent, Context, Div, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::input::{Input, InputState};
use gpui_component::Sizable as _;

/// `max-w-[760px]`: the composer never grows past the reading width.
const COLUMN: f32 = 760.;
/// The gap between the composer and the window edges.
const INSET: f32 = 24.;
/// The bar that names the project.
const BAR_H: f32 = 40.;
/// The composer's corner radius.
const CARD_RADIUS: f32 = 12.;
/// The round send button.
const SEND: f32 = 32.;
/// One control in the card's bottom row.
const CONTROL_H: f32 = 28.;
const MENU_W: f32 = 320.;
const SMALL_MENU_W: f32 = 220.;
/// Where a menu that opens upward clears the card's bottom row.
const ABOVE_ROW: f32 = 52.;

/// The bar, the card, and whichever menu is open, pinned to the bottom.
pub fn composer(app: &LgtmApp, t: &Tokens, window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
    let open = app.project_menu || app.plus_menu || app.worker_menu;
    div()
        .flex()
        .flex_shrink_0()
        .justify_center()
        .px(px(INSET))
        .pb(px(INSET))
        .child(
            div()
                .relative()
                .w_full()
                .max_w(px(COLUMN))
                .flex()
                .flex_col()
                .when_some(app.error.clone(), |this, error| {
                    this.child(
                        div()
                            .pb(px(SPACE[1]))
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.danger)
                            .child(error),
                    )
                })
                .child(project_bar(app, t, cx))
                .child(card(app, t, window, cx))
                .when(open, |this| this.child(dismiss(cx)))
                .when(app.project_menu, |this| {
                    this.child(project_menu(app, t, cx))
                })
                .when(app.plus_menu, |this| this.child(plus_menu(app, t, cx)))
                .when(app.worker_menu, |this| this.child(worker_menu(app, t, cx))),
        )
}

/// A click anywhere else closes the open menu. It has to sit over the whole
/// window, and the composer is the only positioned ancestor, so it reaches out
/// past its own bounds.
fn dismiss(cx: &mut Context<LgtmApp>) -> impl IntoElement {
    div()
        .id("composer-dismiss")
        .absolute()
        .top(px(-4000.))
        .left(px(-4000.))
        .w(px(8000.))
        .h(px(8000.))
        .occlude()
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.close_menus(cx)))
}

fn project_bar(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let chosen = app.project.as_deref().map(repo_slug);
    div()
        .id("project-bar")
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(BAR_H))
        .px(px(SPACE[2]))
        .rounded_tl(px(CARD_RADIUS))
        .rounded_tr(px(CARD_RADIUS))
        .bg(Hsla { a: 0.6, ..t.muted })
        .text_size(px(TEXT_ROW))
        .text_color(if chosen.is_some() { t.fg } else { t.muted_fg })
        .cursor_pointer()
        .child(icon("folder", ICON, t.muted_fg))
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(chosen.unwrap_or_else(|| "Choose project".to_string())),
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            let open = !this.project_menu;
            this.close_menus(cx);
            this.project_menu = open;
            cx.notify();
        }))
}

fn card(app: &LgtmApp, t: &Tokens, _window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
    let ready = !app.prompt.read(cx).value().trim().is_empty() && app.project.is_some();
    let planning = app.chips.contains(&Chip::Plan);
    let worker = app
        .chips
        .iter()
        .find_map(|chip| match chip {
            Chip::Worker(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| AUTO_WORKER.to_string());
    let branch = app.chips.iter().find_map(|chip| match chip {
        Chip::Branch(name) if name.trim() != "main" && !name.trim().is_empty() => {
            Some(name.clone())
        }
        _ => None,
    });

    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .p(px(SPACE[2]))
        .rounded_bl(px(CARD_RADIUS))
        .rounded_br(px(CARD_RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(
            div()
                .text_size(px(TEXT_BODY))
                .child(Input::new(&app.prompt).appearance(false)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(
                    icon_button("plus", "plus", true, t)
                        .w(px(CONTROL_H))
                        .h(px(CONTROL_H))
                        .rounded(px(CONTROL_H / 2.))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            let open = !this.plus_menu;
                            this.close_menus(cx);
                            this.plus_menu = open;
                            cx.notify();
                        })),
                )
                .child(
                    control("plan", planning, t)
                        .child(icon(
                            "lightbulb",
                            ICON,
                            if planning { t.fg } else { t.muted_fg },
                        ))
                        .child("Plan")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.set_chip(Chip::Plan, cx);
                        })),
                )
                .child(div().flex_1())
                .when_some(branch, |this, branch| {
                    this.child(
                        div()
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.muted_fg)
                            .child(branch),
                    )
                })
                .child(
                    control("worker", false, t)
                        .child(icon("cpu", ICON, t.muted_fg))
                        .child(worker)
                        .child(icon("chevron-down", 14., t.muted_fg))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            let open = !this.worker_menu;
                            this.close_menus(cx);
                            this.worker_menu = open;
                            cx.notify();
                        })),
                )
                .child(send_button(ready, t, cx)),
        )
}

/// A text control in the card's bottom row: `Plan`, the worker picker.
fn control(id: &'static str, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(CONTROL_H))
        .px(px(SPACE[1]))
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(TEXT_ROW))
        .text_color(if active { t.fg } else { t.muted_fg })
        .when(active, |this| this.bg(t.muted))
        .hover(|this| this.bg(t.muted).text_color(t.fg))
}

fn send_button(enabled: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    div()
        .id("send")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(SEND))
        .h(px(SEND))
        .rounded(px(SEND / 2.))
        .bg(if enabled { t.primary } else { t.muted })
        .when(enabled, |this| this.cursor_pointer())
        .child(icon(
            "arrow-up",
            ICON,
            if enabled { t.primary_fg } else { t.muted_fg },
        ))
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if enabled {
                this.submit(window, cx);
            }
        }))
}

/// The surface every composer menu is drawn on. The popover colour is the card
/// colour, so a menu that opens over the card has to sit a step above it.
fn menu(width: f32, t: &Tokens) -> Div {
    div()
        .absolute()
        .w(px(width))
        .flex()
        .flex_col()
        .p(px(SPACE[0]))
        .rounded(px(RADIUS))
        .bg(lighten(lighten(t.popover)))
        .border_1()
        .border_color(t.border)
        .occlude()
}

/// One menu item: 32px, muted until hovered.
fn menu_row(id: impl Into<SharedString>, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .h(px(32.))
        .px(px(SPACE[1]))
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(px(TEXT_ROW))
        .text_color(if active { t.fg } else { t.muted_fg })
        .when(active, |this| this.bg(t.muted))
        .hover(|this| this.bg(t.muted))
}

fn separator(t: &Tokens) -> Div {
    div().h(px(1.)).my(px(SPACE[0])).bg(t.border)
}

/// Opens downward, over the card, from the bar's left edge.
fn project_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let chosen = app.project.clone();
    let top = app.error.as_ref().map_or(BAR_H, |_| BAR_H + 24.);
    menu(MENU_W, t)
        .top(px(top))
        .left_0()
        .children(app.known_repositories().into_iter().map(|url| {
            let picked = chosen.as_deref() == Some(url.as_str());
            let value = url.clone();
            menu_row(SharedString::from(format!("repo-{url}")), picked, t)
                .child(icon("folder", ICON, t.muted_fg))
                .child(div().flex_shrink_0().child(repo_slug(&url)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.muted_fg)
                        .child(url.clone()),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.project = Some(value.clone());
                    this.close_menus(cx);
                    cx.notify();
                }))
        }))
        .child(separator(t))
        .child(if app.add_repo {
            inline_field(&app.repo_url, "add-repo-ok", t, cx, |this, cx| {
                let url = this.repo_url.read(cx).value().trim().to_string();
                if url.is_empty() {
                    return;
                }
                this.project = Some(url);
                this.close_menus(cx);
                cx.notify();
            })
            .into_any_element()
        } else {
            menu_row("add-repo", false, t)
                .child(icon("plus", ICON, t.muted_fg))
                .child("Add repository…")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.add_repo = true;
                    this.repo_url
                        .update(cx, |state, cx| state.focus(window, cx));
                    cx.notify();
                }))
                .into_any_element()
        })
}

/// The `+` menu: what the composer can't fit in its bottom row.
fn plus_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    menu(SMALL_MENU_W, t)
        .bottom(px(ABOVE_ROW))
        .left(px(SPACE[1]))
        .child(if app.branch_edit {
            inline_field(&app.base_branch, "branch-ok", t, cx, |this, cx| {
                let branch = this.base_branch.read(cx).value().trim().to_string();
                this.set_chip(Chip::Branch(branch), cx);
                this.close_menus(cx);
                cx.notify();
            })
            .into_any_element()
        } else {
            menu_row("branch", false, t)
                .child(icon("git-branch", ICON, t.muted_fg))
                .child("Base branch…")
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.branch_edit = true;
                    this.base_branch
                        .update(cx, |state, cx| state.focus(window, cx));
                    cx.notify();
                }))
                .into_any_element()
        })
}

fn worker_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let mut names: Vec<String> = vec![AUTO_WORKER.to_string()];
    names.extend(app.workers.iter().map(|worker| worker.info.name.clone()));
    let current = app.chips.iter().find_map(|chip| match chip {
        Chip::Worker(name) => Some(name.clone()),
        _ => None,
    });
    menu(SMALL_MENU_W, t)
        .bottom(px(ABOVE_ROW))
        .right(px(SPACE[1]))
        .children(names.into_iter().map(|name| {
            let picked = current.as_deref().unwrap_or(AUTO_WORKER) == name;
            let chosen = name.clone();
            menu_row(SharedString::from(format!("worker-{name}")), picked, t)
                .child(icon("cpu", ICON, t.muted_fg))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .when(picked, |this| this.child(icon("check", 14., t.fg)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.set_chip(Chip::Worker(chosen.clone()), cx);
                    this.close_menus(cx);
                    cx.notify();
                }))
        }))
}

/// An inline editor inside a menu: a compact field, and the button that reads
/// it back into the composer.
fn inline_field(
    state: &Entity<InputState>,
    id: &'static str,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
    commit: impl Fn(&mut LgtmApp, &mut Context<LgtmApp>) + 'static,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(CONTROL_H + 4.))
        .px(px(SPACE[0]))
        .child(div().flex_1().min_w_0().child(field(state, t).small()))
        .child(
            div()
                .id(id)
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .w(px(CONTROL_H))
                .h(px(CONTROL_H))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|this| this.bg(t.muted))
                .child(icon("check", ICON, t.fg))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| commit(this, cx))),
        )
}
