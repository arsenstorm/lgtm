//! The diff column: one block per file, one row per diff line.

use super::comments::attach;
use crate::app::LgtmApp;
use crate::theme::{self, Tokens};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, AppContext as _, ClickEvent, Context, Div, Hsla, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::checkbox::Checkbox;
use gpui_component::input::InputState;
use lgtm_diff::{Anchor, FileDiff, FileStatus, Line, LineKind, Row};

pub(super) const ROW_HEIGHT: f32 = theme::LINE_MONO;
/// The sticky bar naming the file, `h-8` like the reference.
const FILE_HEADER_H: f32 = 32.;
/// 4ch each for the old/new line numbers, plus a 1ch sign column.
pub(super) const GUTTER: f32 = 32.;
const SIGN: f32 = 10.;
pub(super) const PLUS: f32 = 14.;

/// What every row builder needs: the app it reads, the tokens it paints with,
/// and the context its handlers are bound to.
pub(super) struct Ui<'a, 'b> {
    pub app: &'a LgtmApp,
    pub t: &'a Tokens,
    pub cx: &'a mut Context<'b, LgtmApp>,
}

pub(super) fn file_column(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> AnyElement {
    let mut ui = Ui { app, t, cx };
    let blocks: Vec<Div> = app
        .review
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| file_block(&mut ui, index, file))
        .collect();
    div()
        .id("review-diff")
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(t.bg)
        .overflow_scroll()
        .track_scroll(&app.review.scroll)
        .font_family(theme::MONO_FONT)
        .text_size(px(theme::TEXT_MONO))
        .line_height(px(ROW_HEIGHT))
        .text_color(t.fg)
        .children(blocks)
        .into_any_element()
}

fn file_block(ui: &mut Ui, index: usize, file: &FileDiff) -> Div {
    let viewed = ui.app.review.viewed.contains(&file.name);
    let binary = matches!(file.status, FileStatus::Binary);
    let block = div()
        .flex()
        .flex_col()
        .min_w_full()
        .child(file_header(ui, index, file, viewed));
    if viewed {
        return block;
    }
    if binary {
        return block.child(
            div()
                .px(px(theme::SPACE[2]))
                .text_color(ui.t.muted_fg)
                .child("binary file"),
        );
    }
    let rows: Vec<Div> = lgtm_diff::layout(file, ui.app.review.style)
        .into_iter()
        .enumerate()
        .map(|(row, layout_row)| diff_row(ui, file, layout_row, &format!("{index}:{row}")))
        .collect();
    block.children(rows)
}

fn file_header(ui: &mut Ui, index: usize, file: &FileDiff, viewed: bool) -> Div {
    let (mark, color) = super::status_glyph(file.status, ui.t);
    let path = match file.prev_name.as_ref() {
        Some(prev) => format!("{prev} → {}", file.name),
        None => file.name.clone(),
    };
    let name = file.name.clone();
    div()
        .flex()
        .items_center()
        .gap(px(theme::SPACE[1]))
        .min_w_full()
        .h(px(FILE_HEADER_H))
        .px(px(theme::SPACE[2]))
        .bg(ui.t.card)
        .border_b_1()
        .border_color(ui.t.border)
        .child(
            div()
                .text_color(color)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(mark),
        )
        .child(div().child(path))
        .child(
            div()
                .flex_1()
                .child(super::counts(file.additions, file.deletions, ui.t)),
        )
        .child(
            Checkbox::new(SharedString::from(format!("viewed-head:{index}")))
                .label("Viewed")
                .checked(viewed)
                .on_click(ui.cx.listener(move |this, _: &bool, _, cx| {
                    this.review.toggle_viewed(&name);
                    cx.notify();
                })),
        )
}

fn diff_row(ui: &mut Ui, file: &FileDiff, row: Row<'_>, key: &str) -> Div {
    match row {
        Row::Hunk(hunk) => div()
            .min_w_full()
            .h(px(ROW_HEIGHT))
            .px(px(theme::SPACE[1]))
            .bg(ui.t.hunk_bg)
            .text_color(ui.t.muted_fg)
            .whitespace_nowrap()
            .child(hunk.header.clone()),
        Row::Unified(line) => {
            let anchor = line.anchor(&file.name);
            let block = div().flex().flex_col().min_w_full().child(code_line(
                ui,
                line,
                key,
                anchor.clone(),
            ));
            attach(ui, block, anchor, key)
        }
        Row::Split { left, right } => split_row(ui, file, (left, right), key),
    }
}

fn split_row(
    ui: &mut Ui,
    file: &FileDiff,
    sides: (Option<&Line>, Option<&Line>),
    key: &str,
) -> Div {
    let (left, right) = sides;
    let (left_key, right_key) = (format!("{key}L"), format!("{key}R"));
    let left_anchor = left.and_then(|line| line.anchor(&file.name));
    let right_anchor = right.and_then(|line| line.anchor(&file.name));
    let column = || div().flex_1().min_w_0().flex().flex_col();
    div()
        .flex()
        .flex_col()
        .min_w_full()
        .child(
            div()
                .flex()
                .min_w_full()
                .child(half(ui, left, &left_key, left_anchor.clone()))
                .child(half(ui, right, &right_key, right_anchor.clone())),
        )
        .child(
            div()
                .flex()
                .items_start()
                .min_w_full()
                .child(attach(ui, column(), left_anchor, &left_key))
                .child(attach(ui, column(), right_anchor, &right_key)),
        )
}

/// One side of a split row; a missing side is an empty tinted cell.
fn half(ui: &mut Ui, line: Option<&Line>, key: &str, anchor: Option<Anchor>) -> Div {
    let cell = div().flex_1().min_w_0();
    match line {
        Some(line) => cell.child(code_line(ui, line, key, anchor)),
        None => cell.h(px(ROW_HEIGHT)).bg(ui.t.gutter),
    }
}

fn code_line(ui: &mut Ui, line: &Line, key: &str, anchor: Option<Anchor>) -> Div {
    let t = ui.t;
    let (tint, emph) = colours(line.kind, t);
    let group = SharedString::from(format!("row:{key}"));
    let context = matches!(line.kind, LineKind::Context);
    let (sign, sign_color) = match line.kind {
        LineKind::Addition => ("+", t.diff_add),
        LineKind::Deletion => ("−", t.diff_del),
        LineKind::Context => (" ", t.muted_fg),
    };
    div()
        .group(group.clone())
        .flex()
        .items_center()
        .min_w_full()
        .h(px(ROW_HEIGHT))
        .when_some(tint, |this, colour| this.bg(colour))
        .hover(|this| this.bg(theme::lighten(tint.unwrap_or(t.bg))))
        .child(plus_button(ui, &group, key, anchor))
        // Only context rows need the gutter fill; a changed row is already tinted.
        .child(number(line.old_no, context, t))
        .child(number(line.new_no, context, t))
        .child(
            div()
                .w(px(SIGN))
                .flex_none()
                .text_color(sign_color)
                .child(sign),
        )
        .child(text_cell(line, emph))
}

fn number(no: Option<u32>, context: bool, t: &Tokens) -> Div {
    div()
        .w(px(GUTTER))
        .flex_none()
        .pr(px(4.))
        .text_right()
        .when(context, |this| this.bg(t.gutter))
        .text_color(t.muted_fg)
        .child(no.map(|n| n.to_string()).unwrap_or_default())
}

fn text_cell(line: &Line, emph: Option<Hsla>) -> Div {
    let cell = div().flex().flex_none().pl(px(4.)).whitespace_nowrap();
    if line.segments.is_empty() {
        return cell.child(line.text.clone());
    }
    cell.children(line.segments.iter().map(|segment| {
        div()
            .whitespace_nowrap()
            .when_some(emph.filter(|_| segment.changed), |this, colour| {
                this.bg(colour)
            })
            .child(segment.text.clone())
    }))
}

/// The hover-only "add a comment" affordance at the gutter's left edge.
fn plus_button(ui: &mut Ui, group: &SharedString, key: &str, anchor: Option<Anchor>) -> AnyElement {
    let cell = div().w(px(PLUS)).flex_none();
    let Some(anchor) = anchor else {
        return cell.into_any_element();
    };
    cell.id(SharedString::from(format!("add:{key}")))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .opacity(0.)
        .group_hover(group.clone(), |style| style.opacity(1.))
        .bg(ui.t.primary)
        .text_color(ui.t.primary_fg)
        .child("+")
        .on_click(ui.cx.listener(move |this, _: &ClickEvent, window, cx| {
            let anchor = anchor.clone();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .rows(3)
                    .placeholder("leave a comment")
            });
            this.review.draft = Some((anchor, input));
            cx.notify();
        }))
        .into_any_element()
}

fn colours(kind: LineKind, t: &Tokens) -> (Option<Hsla>, Option<Hsla>) {
    match kind {
        LineKind::Addition => (Some(t.diff_add_bg), Some(t.diff_add_emph)),
        LineKind::Deletion => (Some(t.diff_del_bg), Some(t.diff_del_emph)),
        LineKind::Context => (None, None),
    }
}
