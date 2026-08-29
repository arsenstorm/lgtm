//! The three menus that open off the composer: project, `+`, and worker.

use super::{ABOVE_ROW, CARD_INSET, MENU_W, REAR_H, REAR_INSET, ROW_H, SMALL_MENU_W};
use crate::app::LgtmApp;
use crate::home::{Chip, AUTO_WORKER};
use crate::tasks::repo_slug;
use crate::theme::{
    field, icon, lighten, tokens, Tokens, ICON, RADIUS, SPACE, TEXT_ROW, TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, ClickEvent, Context, Div, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::input::InputState;
use gpui_component::Sizable as _;

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
pub(super) fn project_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let chosen = app.project.clone();
    let top = app.error.as_ref().map_or(REAR_H, |_| REAR_H + 24.);
    menu(MENU_W, t)
        .top(px(top))
        .left(px(REAR_INSET))
        .children(
            app.known_repositories()
                .into_iter()
                .map(|url| repo_row(url, chosen.as_deref(), t, cx)),
        )
        .child(separator(t))
        .child(if app.add_repo {
            inline_field(&app.repo_url, "add-repo-ok", cx, |this, cx| {
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

fn repo_row(
    url: String,
    chosen: Option<&str>,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> impl IntoElement {
    let picked = chosen == Some(url.as_str());
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
                .child(url),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.project = Some(value.clone());
            this.close_menus(cx);
            cx.notify();
        }))
}

/// The `+` menu: what the composer can't fit in its bottom row.
pub(super) fn plus_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    menu(SMALL_MENU_W, t)
        .bottom(px(ABOVE_ROW))
        .left(px(CARD_INSET))
        .child(if app.branch_edit {
            inline_field(&app.base_branch, "branch-ok", cx, |this, cx| {
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

pub(super) fn worker_menu(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
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
    cx: &mut Context<LgtmApp>,
    commit: impl Fn(&mut LgtmApp, &mut Context<LgtmApp>) + 'static,
) -> Div {
    let t = &tokens(cx);
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
