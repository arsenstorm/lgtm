//! The add-project dialog: one clone URL, from wherever a project is added.

use crate::app::LgtmApp;
use crate::tasks::repo_slug;
use crate::theme::{
    icon, icon_button, panel, scrim, section_label, tokens, Tokens, ICON, RADIUS, SPACE,
    TEXT_SECONDARY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, relative, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;

const WIDTH: f32 = 560.;
/// `text-xl`: a dialog names itself in the body, not in a title bar.
const TITLE: f32 = 20.;
/// The field is a place to type, not a control in a row: it gets height.
const FIELD_H: f32 = 44.;

pub fn modal(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let url = app.inputs.repo_url.read(cx).value().trim().to_string();
    scrim("add-project-scrim", &t)
        .pt(relative(0.18))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            panel(&t)
                .id("add-project")
                .w(px(WIDTH))
                .gap(px(SPACE[3]))
                .p(px(SPACE[4]))
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(title(&t, cx))
                .child(repository(app, &url, &t))
                .child(actions(cx)),
        )
        .into_any_element()
}

/// The dialog's name, with the cross on its own line rather than in a bar.
fn title(t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .text_size(px(TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Add project"),
        )
        .child(icon_button("add-project-close", "x", true, t).on_click(
            cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)),
        ))
}

fn repository(app: &LgtmApp, url: &str, t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label("Repository", t))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .h(px(FIELD_H))
                .px(px(SPACE[2]))
                .rounded(px(RADIUS))
                .bg(t.input_fill)
                .border_1()
                .border_color(t.input)
                .child(icon("folder", ICON, t.muted_fg))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&app.inputs.repo_url).appearance(false)),
                ),
        )
        .child(hint(url, t))
}

/// What the URL will be called once it is added, so a typo shows before the
/// project appears in the sidebar under a name nobody meant.
fn hint(url: &str, t: &Tokens) -> Div {
    div()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .when(url.is_empty(), |this| {
            this.child("A git clone URL, over HTTPS or SSH.")
        })
        .when(!url.is_empty(), |this| {
            this.child(format!("Adds {} to your projects.", repo_slug(url)))
        })
}

fn actions(cx: &mut Context<LgtmApp>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(SPACE[1]))
        .child(
            Button::new("add-project-cancel")
                .label("Cancel")
                .ghost()
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)),
                ),
        )
        .child(
            Button::new("add-project-ok")
                .label("Add project")
                .primary()
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.add_project(window, cx)),
                ),
        )
}

impl LgtmApp {
    /// Opens the dialog on an empty field, wherever it was asked for.
    pub fn open_add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menus(cx);
        self.ui.overlay = crate::app::Overlay::AddProject;
        self.inputs.repo_url.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    /// Takes the URL as the composer's project, so the next prompt runs in it.
    pub fn add_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.inputs.repo_url.read(cx).value().trim().to_string();
        if url.is_empty() {
            return;
        }
        self.close_overlay(window, cx);
        self.composer.project = Some(url);
        self.go_home(window, cx);
    }
}
