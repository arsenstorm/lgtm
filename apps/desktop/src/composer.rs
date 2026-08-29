//! The composer: the project panel, the prompt card, and the menus that hang
//! off them. Pinned to the bottom of the main area, like Codex.
//!
//! The geometry is Codex's, measured off its own composer and reproduced here:
//! a 752 px canvas holding a rear panel inset 21 px and a card inset 8 px that
//! covers the panel's bottom. Depth comes from the overlap, not from a shadow.

use crate::app::LgtmApp;
use crate::home::{Chip, AUTO_WORKER};
use crate::sidebar::repo_slug;
use crate::theme::{
    field, icon, lighten, Tokens, ICON, RADIUS, SPACE, TEXT_BODY, TEXT_ROW, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rems, ClickEvent, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::input::{Input, InputState};
use gpui_component::Sizable as _;

/// The canvas the whole composer group is laid out on.
const CANVAS: f32 = 752.;
/// The gap between the canvas and the window edges.
const INSET: f32 = 24.;
/// The rear panel's inset from the canvas edge.
const REAR_INSET: f32 = 21.;
/// The card's inset from the canvas edge.
const CARD_INSET: f32 = 8.;
/// How much of the rear panel stays visible above the card.
const REAR_H: f32 = 38.;
/// How far the card rides up over the rear panel, hiding its bottom corners.
const OVERLAP: f32 = 20.;
/// The corner radius shared by the panel's top and the whole card.
const CARD_RADIUS: f32 = 18.;
/// The card's bottom control row, and the send button that sets its height.
const ROW_H: f32 = 28.;
/// The action row's left padding, one border in from the card edge.
const ROW_INSET: f32 = 15.;
/// The prompt's left padding: the composer's second, tighter padding system.
const TEXT_INSET: f32 = 12.;
/// The gap between the prompt and the action row, sized so the row's centre
/// lands 76 px below the card's top and the card comes to 98 px.
const ROW_GAP: f32 = 8.;
/// The clearance the divider gets on both sides.
const CLEAR: f32 = 10.;
/// The `+` hit area, wider than its 12 px glyph and pulled back onto it.
const PLUS_BOX: f32 = 24.;
const MENU_W: f32 = 320.;
const SMALL_MENU_W: f32 = 220.;
/// Where a menu that opens upward clears the card's bottom row.
const ABOVE_ROW: f32 = 52.;

/// Lucide draws in a 24 box; these sizes put the ink where Codex has it.
/// `plus` at 12 × 12, `chevron-down` at 10 × 5, `arrow-up` at 14 × 14.
const PLUS_ICON: f32 = 18.;
const CHEVRON: f32 = 16.;
const ARROW: f32 = 20.;
/// `folder` at 14 × 12, the panel's only mark.
const FOLDER: f32 = 15.;

/// The panel, the card, and whichever menu is open, pinned to the bottom.
pub fn composer(app: &LgtmApp, t: &Tokens, _window: &mut Window, cx: &mut Context<LgtmApp>) -> Div {
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
                .max_w(px(CANVAS))
                .flex()
                .flex_col()
                .when_some(app.error.clone(), |this, error| {
                    this.child(
                        div()
                            .pb(px(SPACE[1]))
                            .px(px(CARD_INSET))
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.danger)
                            .child(error),
                    )
                })
                .child(project_panel(app, t, cx))
                .child(card(app, t, cx))
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

/// The darker panel behind the card. Only its top 38 px are ever seen; the rest
/// is there so the card's rounded corners have something to sit on.
fn project_panel(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let chosen = app.project.as_deref().map(repo_slug);
    div()
        .id("project-panel")
        .flex()
        .items_center()
        .gap(px(7.))
        .mx(px(REAR_INSET))
        .h(px(REAR_H + OVERLAP))
        .pb(px(OVERLAP))
        .pl(px(13.))
        .pr(px(SPACE[2]))
        .rounded_tl(px(CARD_RADIUS))
        .rounded_tr(px(CARD_RADIUS))
        .bg(t.composer_rear)
        .text_size(px(TEXT_BODY))
        .text_color(t.composer_secondary)
        .cursor_pointer()
        .child(icon("folder", FOLDER, t.composer_secondary))
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

fn card(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .mx(px(CARD_INSET))
        .mt(px(-OVERLAP))
        .pt(px(17.))
        .pb(px(7.))
        .rounded(px(CARD_RADIUS))
        .bg(t.composer)
        .border_1()
        .border_color(t.composer_edge)
        .child(prompt(app, t, cx))
        .child(controls(app, t, cx))
}

/// The prompt, with our own placeholder laid over its empty first line.
fn prompt(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let empty = app.prompt.read(cx).value().is_empty();
    div()
        .relative()
        .px(px(TEXT_INSET))
        .text_size(px(TEXT_BODY))
        .child(Input::new(&app.prompt).appearance(false).p_0())
        .when(empty, |this| {
            this.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(TEXT_INSET))
                    // The input's own line box, so the placeholder sits exactly
                    // where the first typed line will.
                    .line_height(rems(1.25))
                    .text_size(px(TEXT_BODY))
                    .text_color(t.composer_placeholder)
                    .child("Describe your task..."),
            )
        })
}

/// The card's bottom row: `+`, the divider, Plan, the worker, send.
fn controls(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let ready = !app.prompt.read(cx).value().trim().is_empty() && app.project.is_some();
    let planning = app.chips.contains(&Chip::Plan);
    let branch = app.chips.iter().find_map(|chip| match chip {
        Chip::Branch(name) if name.trim() != "main" && !name.trim().is_empty() => {
            Some(name.clone())
        }
        _ => None,
    });

    div()
        .flex()
        .items_center()
        .h(px(ROW_H))
        .pl(px(ROW_INSET))
        .pr(px(CARD_INSET - 1.))
        .child(
            div()
                .id("plus")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .ml(px(-(PLUS_BOX - PLUS_ICON) / 2. - (PLUS_ICON - 12.) / 2.))
                .w(px(PLUS_BOX))
                .h(px(PLUS_BOX))
                .cursor_pointer()
                .child(icon("plus", PLUS_ICON, t.composer_secondary))
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let open = !this.plus_menu;
                    this.close_menus(cx);
                    this.plus_menu = open;
                    cx.notify();
                })),
        )
        .child(
            div()
                .ml(px(CLEAR))
                .w(px(1.))
                .h(px(16.))
                .flex_shrink_0()
                .bg(t.composer_divider),
        )
        .child(
            control("plan", planning, t)
                .ml(px(CLEAR))
                .child(icon(
                    "lightbulb",
                    ICON,
                    if planning {
                        t.composer_primary
                    } else {
                        t.composer_secondary
                    },
                ))
                .child("Plan")
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.set_chip(Chip::Plan, cx);
                })),
        )
        .when_some(branch, |this, branch| {
            this.child(
                div()
                    .ml(px(SPACE[1]))
                    .flex_shrink_0()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.composer_secondary)
                    .child(branch),
            )
        })
        .child(div().flex_1())
        .child(worker_control(app, t, cx))
        .child(send_button(ready, t, cx))
}

/// `<worker> <what it costs>`: Auto is `any`, a named worker its busy slots.
fn worker_control(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let name = app
        .chips
        .iter()
        .find_map(|chip| match chip {
            Chip::Worker(name) => Some(name.clone()),
            _ => None,
        })
        .unwrap_or_else(|| AUTO_WORKER.to_string());
    let slots = if name == AUTO_WORKER {
        Some("any".to_string())
    } else {
        app.workers
            .iter()
            .find(|worker| worker.info.name == name)
            .map(|worker| format!("{}/{}", worker.running.len(), worker.info.slots))
    };
    control("worker", true, t)
        .child(name)
        .when_some(slots, |this, slots| {
            this.child(div().text_color(t.composer_secondary).child(slots))
        })
        .child(
            div()
                .ml(px(1.))
                .child(icon("chevron-down", CHEVRON, t.composer_secondary)),
        )
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            let open = !this.worker_menu;
            this.close_menus(cx);
            this.worker_menu = open;
            cx.notify();
        }))
}

/// A text control in the card's bottom row: `Plan`, the worker picker. No
/// surface of its own — active means the label goes from grey to white.
fn control(id: &'static str, active: bool, t: &Tokens) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(5.))
        .h(px(ROW_H))
        .cursor_pointer()
        .text_size(px(TEXT_BODY))
        .text_color(if active {
            t.composer_primary
        } else {
            t.composer_secondary
        })
        .hover(|this| this.text_color(t.composer_primary))
}

fn send_button(enabled: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    div()
        .id("send")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .ml(px(SPACE[1]))
        .w(px(ROW_H))
        .h(px(ROW_H))
        .rounded(px(ROW_H / 2.))
        .bg(if enabled {
            t.send_bg
        } else {
            t.send_disabled_bg
        })
        .when(enabled, |this| this.cursor_pointer())
        .child(icon(
            "arrow-up",
            ARROW,
            if enabled {
                t.send_fg
            } else {
                t.send_disabled_fg
            },
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

/// Opens downward, over the card, from the panel's left edge.
fn project_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let chosen = app.project.clone();
    let top = app.error.as_ref().map_or(REAR_H, |_| REAR_H + 24.);
    menu(MENU_W, t)
        .top(px(top))
        .left(px(REAR_INSET))
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
        .left(px(CARD_INSET))
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
        .right(px(CARD_INSET))
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
        .h(px(ROW_H + 4.))
        .px(px(SPACE[0]))
        .child(div().flex_1().min_w_0().child(field(state, t).small()))
        .child(
            div()
                .id(id)
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .w(px(ROW_H))
                .h(px(ROW_H))
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|this| this.bg(t.muted))
                .child(icon("check", ICON, t.fg))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| commit(this, cx))),
        )
}
