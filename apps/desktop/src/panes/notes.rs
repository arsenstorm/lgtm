//! The Notes tab: the agent's scratchpad, and the editor over it.

use super::muted;
use crate::app::LgtmApp;
use crate::theme::{field, Tokens, LINE_MONO, MONO_FONT, SPACE, TEXT_MONO, TEXT_ROW};
use gpui::{
    div, px, AnyElement, ClickEvent, Context, IntoElement, ParentElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::Sizable as _;
use lgtm_protocol::Task;

pub(super) fn notes(
    app: &LgtmApp,
    task: &Task,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    let column = div()
        .flex()
        .flex_col()
        .gap(px(SPACE[2]))
        .text_size(px(TEXT_ROW));
    if app.ui.editing_notes {
        return column
            .child(field(&app.inputs.notes, t))
            .child(
                div().child(
                    Button::new("save-notes")
                        .label("Save")
                        .primary()
                        .small()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.save_notes(cx))),
                ),
            )
            .into_any_element();
    }
    column
        .child(body(&task.scratchpad, t))
        .child(
            div().child(
                Button::new("edit-notes")
                    .label("Edit")
                    .outline()
                    .small()
                    .on_click(
                        cx.listener(|this, _: &ClickEvent, window, cx| this.edit_notes(window, cx)),
                    ),
            ),
        )
        .into_any_element()
}

/// The scratchpad is the agent's own file, so it keeps monospace and its own
/// line breaks while the chrome around it stays UI type.
fn body(scratchpad: &str, t: &Tokens) -> AnyElement {
    if scratchpad.trim().is_empty() {
        return muted("No notes yet", t);
    }
    div()
        .flex()
        .flex_col()
        .font_family(MONO_FONT)
        .text_size(px(TEXT_MONO))
        .line_height(px(LINE_MONO))
        .children(scratchpad.lines().map(|line| div().child(line.to_string())))
        .into_any_element()
}
