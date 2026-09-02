//! The dialogs behind a thread's `…` menu: rename it, archive it, delete it.

use crate::app::{LgtmApp, Overlay, Page};
use crate::net::Action;
use crate::panes::danger_ghost;
use crate::sidebar::session_title;
use crate::theme::{
    icon_button, panel, scrim, section_label, tokens, Tokens, RADIUS, SPACE, TEXT_SECONDARY,
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

/// What the open dialog would do to the thread it names.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ThreadAction {
    Rename,
    Archive,
    Delete,
}

pub fn modal(app: &LgtmApp, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let Some((id, action)) = app.ui.thread_action.clone() else {
        return div().into_any_element();
    };
    let name = app
        .sessions
        .iter()
        .find(|open| open.id == id)
        .map(session_title)
        .unwrap_or_default();
    let body = match action {
        ThreadAction::Rename => rename_body(app, &t, cx),
        ThreadAction::Archive => confirm_body(&name, ARCHIVE, &t, cx),
        ThreadAction::Delete => confirm_body(&name, DELETE, &t, cx),
    };
    scrim("thread-scrim", &t)
        .pt(relative(0.18))
        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)))
        .child(
            panel(&t)
                .id("thread")
                .w(px(WIDTH))
                .gap(px(SPACE[3]))
                .p(px(SPACE[4]))
                .on_click(|_, _, cx| cx.stop_propagation())
                .children(body),
        )
        .into_any_element()
}

/// The dialog's name, with the cross on its own line rather than in a bar.
fn title(text: &'static str, cx: &mut Context<LgtmApp>) -> Div {
    let t = tokens(cx);
    div()
        .flex()
        .items_center()
        .child(
            div()
                .flex_1()
                .text_size(px(TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(text),
        )
        .child(icon_button("thread-close", "x", true, &t).on_click(
            cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)),
        ))
}

fn muted(text: String, t: &Tokens) -> Div {
    div()
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.muted_fg)
        .child(text)
}

fn rename_body(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    vec![
        title("Rename thread", cx).into_any_element(),
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(section_label("Title", t))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(FIELD_H))
                    .px(px(SPACE[2]))
                    .rounded(px(RADIUS))
                    .bg(t.input_fill)
                    .border_1()
                    .border_color(t.input)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&app.inputs.thread_title).appearance(false)),
                    ),
            )
            .into_any_element(),
        actions(
            "Rename",
            Button::new("thread-ok").primary(),
            cx.listener(|this, _: &ClickEvent, window, cx| this.rename_thread(window, cx)),
            cx,
        )
        .into_any_element(),
    ]
}

/// A confirm dialog's words: what it is called, and what saying yes does.
struct Confirm {
    title: &'static str,
    line: &'static str,
    confirm: &'static str,
}

const ARCHIVE: Confirm = Confirm {
    title: "Archive thread",
    line: "It leaves the sidebar. Its tasks keep running and you can still find it in the command palette.",
    confirm: "Archive",
};
const DELETE: Confirm = Confirm {
    title: "Delete thread",
    line: "This cannot be undone. The tasks it started are kept — only the thread is deleted.",
    confirm: "Delete",
};

fn confirm_body(
    name: &str,
    words: Confirm,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> Vec<AnyElement> {
    let danger = words.confirm == DELETE.confirm;
    let button = Button::new("thread-ok").when_else(
        danger,
        |this| this.custom(danger_ghost(t, cx)),
        |this| this.primary(),
    );
    vec![
        div()
            .flex()
            .flex_col()
            .gap(px(SPACE[1]))
            .child(title(words.title, cx))
            .child(muted(name.to_string(), t))
            .into_any_element(),
        muted(words.line.to_string(), t).into_any_element(),
        actions(
            words.confirm,
            button,
            cx.listener(|this, _: &ClickEvent, window, cx| this.confirm_thread_action(window, cx)),
            cx,
        )
        .into_any_element(),
    ]
}

fn actions(
    label: &'static str,
    confirm: Button,
    on_confirm: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
    cx: &mut Context<LgtmApp>,
) -> Div {
    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(SPACE[1]))
        .child(
            Button::new("thread-cancel")
                .label("Cancel")
                .ghost()
                .on_click(
                    cx.listener(|this, _: &ClickEvent, window, cx| this.close_overlay(window, cx)),
                ),
        )
        .child(confirm.label(label).on_click(on_confirm))
}

impl LgtmApp {
    /// Opens the dialog for `action` on one thread. Unarchiving is not
    /// destructive, so the menu does it outright rather than coming here.
    pub fn open_thread_action(
        &mut self,
        id: String,
        action: ThreadAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self
            .sessions
            .iter()
            .find(|open| open.id == id)
            .map(|open| open.title.clone())
            .unwrap_or_default();
        self.close_menus(cx);
        self.ui.overlay = Overlay::Thread;
        self.ui.thread_action = Some((id, action));
        if action == ThreadAction::Rename {
            self.inputs.thread_title.update(cx, |state, cx| {
                state.set_value(title, window, cx);
                state.focus(window, cx);
            });
        }
        cx.notify();
    }

    pub fn rename_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, ThreadAction::Rename)) = self.ui.thread_action.clone() else {
            return;
        };
        let title = self.inputs.thread_title.read(cx).value().trim().to_string();
        if title.is_empty() {
            return;
        }
        self.close_overlay(window, cx);
        self.act_on(id, Action::RenameSession(title), cx);
    }

    /// Archive or delete, whichever the open dialog was opened for.
    pub fn confirm_thread_action(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, action)) = self.ui.thread_action.clone() else {
            return;
        };
        self.close_overlay(window, cx);
        match action {
            // Rename commits from its field, not from this button.
            ThreadAction::Rename => {}
            ThreadAction::Archive => self.set_thread_archived(id, true, cx),
            ThreadAction::Delete => self.delete_thread(id, window, cx),
        }
    }

    pub fn set_thread_archived(&mut self, id: String, archived: bool, cx: &mut Context<Self>) {
        self.close_menus(cx);
        self.act_on(id, Action::SetSessionArchived(archived), cx);
    }

    /// Deleting the thread the window is on would leave it pointing at a page
    /// the next poll cannot fetch, so the window goes home first.
    fn delete_thread(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == Page::Session(id.clone()) {
            self.go_home(window, cx);
        }
        self.act_on(id, Action::DeleteSession, cx);
    }
}
