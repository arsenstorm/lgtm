//! The composer: the project panel, the prompt card, and the menus that hang
//! off them. Pinned to the bottom of the main area, like Codex.
//!
//! The geometry is Codex's, measured off its own composer and reproduced here:
//! a 752 px canvas holding a rear panel inset 21 px and a card inset 8 px that
//! covers the panel's bottom. Depth comes from the overlap, not from a shadow.

mod menus;

use crate::app::LgtmApp;
use crate::home::Chip;
use crate::home::AUTO_WORKER;
use crate::sidebar::repo_slug;
use crate::theme::{icon, Tokens, ICON, SPACE, TEXT_BODY, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rems, ClickEvent, Context, Div, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::input::Input;

use menus::{plus_menu, project_menu, worker_menu};

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
            this.toggle_menu(|app| &mut app.project_menu, cx)
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
        .child(plus_button(t, cx))
        .child(
            div()
                .ml(px(CLEAR))
                .w(px(1.))
                .h(px(16.))
                .flex_shrink_0()
                .bg(t.composer_divider),
        )
        .child(plan_control(planning, t, cx))
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

fn plus_button(t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
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
        .on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| {
                this.toggle_menu(|app| &mut app.plus_menu, cx)
            }),
        )
}

fn plan_control(planning: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let tone = if planning {
        t.composer_primary
    } else {
        t.composer_secondary
    };
    control("plan", planning, t)
        .ml(px(CLEAR))
        .child(icon("lightbulb", ICON, tone))
        .child("Plan")
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.set_chip(Chip::Plan, cx);
        }))
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
            this.toggle_menu(|app| &mut app.worker_menu, cx)
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
