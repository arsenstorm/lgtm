//! Review comments: the cards under a diff line, and the draft being written.

use super::rows::{Ui, GUTTER, PLUS};
use crate::review::Comment;
use crate::theme::{self, icon, Tokens};
use gpui::{
    div, px, ClickEvent, Context, Div, Entity, InteractiveElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::Sizable as _;
use lgtm_diff::Anchor;

/// Hangs the comments (and any open draft) for `anchor` under `block`.
pub(super) fn attach(ui: &mut Ui, block: Div, anchor: Option<Anchor>, key: &str) -> Div {
    let Some(anchor) = anchor else {
        return block;
    };
    let cards: Vec<Div> = ui
        .app
        .review
        .comments
        .iter()
        .enumerate()
        .filter(|(_, comment)| comment.anchor == anchor)
        .map(|(index, comment)| card(index, comment, key, ui.t, ui.cx))
        .collect();
    let draft = ui
        .app
        .review
        .draft
        .as_ref()
        .filter(|(open, _)| *open == anchor)
        .map(|(_, input)| draft_card(input, ui.t, ui.cx));
    block.children(cards).children(draft)
}

/// `file:line`, with the leading directories dropped — the caption the
/// reference comment card carries.
fn caption(anchor: &Anchor) -> String {
    let name = anchor.file.rsplit('/').next().unwrap_or(&anchor.file);
    format!("{name}:{}", anchor.line)
}

fn card(
    index: usize,
    comment: &Comment,
    key: &str,
    t: &Tokens,
    cx: &mut Context<crate::app::LgtmApp>,
) -> Div {
    shell(t)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(theme::SPACE[1]))
                .font_family(theme::MONO_FONT)
                .text_size(px(theme::TEXT_MONO))
                .text_color(t.muted_fg)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(caption(&comment.anchor)),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("drop:{key}:{index}")))
                        .cursor_pointer()
                        .child(icon("x", 12., t.muted_fg))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if index < this.review.comments.len() {
                                this.review.comments.remove(index);
                            }
                            cx.notify();
                        })),
                ),
        )
        .child(div().child(comment.text.clone()))
}

fn draft_card(
    input: &Entity<InputState>,
    t: &Tokens,
    cx: &mut Context<crate::app::LgtmApp>,
) -> Div {
    shell(t).child(theme::field(input, t)).child(
        div()
            .flex()
            .justify_end()
            .gap(px(theme::SPACE[1]))
            .child(
                Button::new("comment-cancel")
                    .label("Cancel")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.review.draft = None;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("comment-save")
                    .label("Comment")
                    .primary()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        let Some((anchor, input)) = this.review.draft.take() else {
                            return;
                        };
                        let text = input.read(cx).value().to_string();
                        if !text.trim().is_empty() {
                            this.review.comments.push(Comment { anchor, text });
                        }
                        cx.notify();
                    })),
            ),
    )
}

fn shell(t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(theme::SPACE[1]))
        .my(px(theme::SPACE[1]))
        .ml(px(PLUS + GUTTER * 2.))
        .mr(px(theme::SPACE[2]))
        .p(px(theme::SPACE[2]))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .rounded(px(theme::RADIUS))
        .font_family(theme::UI_FONT)
        .text_size(px(theme::TEXT_BODY))
        .line_height(px(20.))
        .text_color(t.fg)
}
