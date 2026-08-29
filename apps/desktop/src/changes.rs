//! The Changes tab: a file tree on the left, the diff on the right.

mod rows;

use crate::app::LgtmApp;
use crate::net::Action;
use crate::review::{MarkViewed, NextFile, PrevFile, ToggleDiffStyle};
use crate::theme::{self, Tokens};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::Sizable as _;
use lgtm_diff::{DiffStyle, FileStatus};
use lgtm_protocol::TaskKind;

const TREE_WIDTH: f32 = 260.;
const INDENT: f32 = 12.;

pub fn changes_pane(
    app: &mut LgtmApp,
    _window: &mut Window,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    let t = theme::tokens(cx);
    let Some(task) = app.selected_task().cloned() else {
        return muted("no changes yet", &t);
    };
    if task.spec.kind == TaskKind::Plan {
        return muted("plan tasks have no diff", &t);
    }
    let Some(result) = task.result.as_ref() else {
        return muted("no changes yet", &t);
    };
    app.review.load(&task.id, &result.diff);
    if app.review.files.is_empty() {
        return muted("no changes yet", &t);
    }

    // Actions dispatch along the focus path, which ends at the app root, so
    // these fire only when focus is inside the pane; `LgtmApp::render` mirrors
    // them for the v/n/p/s keybindings.
    div()
        .size_full()
        .flex()
        .min_h_0()
        .on_action(cx.listener(|this, _: &MarkViewed, _, cx| {
            this.review.mark_current_viewed();
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &NextFile, _, cx| {
            this.review.step_file(1);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &PrevFile, _, cx| {
            this.review.step_file(-1);
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &ToggleDiffStyle, _, cx| {
            this.review.flip_style();
            cx.notify();
        }))
        .child(sidebar(app, &t, cx))
        .child(rows::file_column(app, &t, cx))
        .into_any_element()
}

/// Sends the collected comments as one follow-up and clears them.
pub fn request_changes(app: &mut LgtmApp, cx: &mut Context<LgtmApp>) {
    let Some(message) = app.review.request_changes_message() else {
        return;
    };
    app.review.comments.clear();
    app.act(Action::Tell(message), cx);
}

fn sidebar(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    div()
        .w(px(TREE_WIDTH))
        .flex_none()
        .flex()
        .flex_col()
        .bg(t.surface)
        .border_r_1()
        .border_color(t.border)
        .child(summary(app, t, cx))
        .child(
            div()
                .id("review-tree")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .py(px(theme::SPACE[1]))
                .children(tree_rows(app, t, cx)),
        )
        .child(footer(app, t, cx))
}

fn summary(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let files = &app.review.files;
    let adds = files.iter().fold(0, |sum, file| sum + file.additions);
    let dels = files.iter().fold(0, |sum, file| sum + file.deletions);
    let split = matches!(app.review.style, DiffStyle::Split);
    div()
        .flex()
        .flex_col()
        .gap(px(theme::SPACE[1]))
        .p(px(theme::SPACE[2]))
        .border_b_1()
        .border_color(t.border)
        .child(
            div()
                .text_size(px(theme::TEXT_SECONDARY))
                .text_color(t.text_muted)
                .child(format!("{} files · +{adds} −{dels}", files.len())),
        )
        .child(
            div()
                .flex()
                .gap(px(theme::SPACE[1]))
                .child(style_button("unified", "Unified", !split, cx))
                .child(style_button("split", "Split", split, cx)),
        )
}

fn style_button(
    id: &'static str,
    label: &'static str,
    selected: bool,
    cx: &mut Context<LgtmApp>,
) -> Button {
    let style = if id == "split" {
        DiffStyle::Split
    } else {
        DiffStyle::Unified
    };
    Button::new(id)
        .label(label)
        .xsmall()
        .when(selected, |button| button.primary())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.review.set_style(style);
            cx.notify();
        }))
}

fn tree_rows(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let Some(tree) = app.review.tree.as_ref() else {
        return Vec::new();
    };
    tree.visible()
        .into_iter()
        .map(|node| {
            let pad = px(theme::SPACE[2] + node.depth as f32 * INDENT);
            let row = div()
                .flex()
                .items_center()
                .gap(px(theme::SPACE[1]))
                .pl(pad)
                .pr(px(theme::SPACE[2]))
                .h(px(rows::ROW_HEIGHT))
                .text_size(px(theme::TEXT_SECONDARY));
            if node.is_dir {
                dir_row(row, node, t, cx)
            } else {
                file_row(app, row, node, t, cx)
            }
        })
        .collect()
}

fn dir_row(
    row: Div,
    node: lgtm_diff::tree::Node,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    let path = node.path.clone();
    let chevron = if node.expanded { "▾" } else { "▸" };
    row.id(SharedString::from(format!("dir:{}", node.path)))
        .cursor_pointer()
        .text_color(t.text_muted)
        .hover(|style| style.bg(t.surface_raised))
        .child(chevron)
        .child(node.name.clone())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            if let Some(tree) = this.review.tree.as_mut() {
                tree.toggle(&path);
            }
            cx.notify();
        }))
        .into_any_element()
}

fn file_row(
    app: &LgtmApp,
    row: Div,
    node: lgtm_diff::tree::Node,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> AnyElement {
    let Some(index) = app.review.files.iter().position(|f| f.name == node.path) else {
        return row.into_any_element();
    };
    let file = &app.review.files[index];
    let (mark, color) = status_glyph(file.status, t);
    let counts = format!("+{} −{}", file.additions, file.deletions);
    let viewed = app.review.viewed.contains(&file.name);
    let name = file.name.clone();
    let current = index == app.review.current_file;

    row.id(SharedString::from(format!("file:{}", node.path)))
        .cursor_pointer()
        .text_color(t.text)
        .when(current, |this| this.bg(t.selection))
        .hover(|style| style.bg(t.surface_raised))
        .child(div().w(px(12.)).text_color(color).child(mark))
        .child(div().flex_1().min_w_0().truncate().child(node.name.clone()))
        .child(div().text_color(t.text_muted).child(counts))
        .child(
            Checkbox::new(SharedString::from(format!("viewed:{}", node.path)))
                .checked(viewed)
                .on_click({
                    let name = name.clone();
                    cx.listener(move |this, _: &bool, _, cx| {
                        this.review.toggle_viewed(&name);
                        cx.notify();
                    })
                }),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.review.focus_file(index);
            cx.notify();
        }))
        .into_any_element()
}

fn footer(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let count = app.review.comment_count();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(theme::SPACE[1]))
        .p(px(theme::SPACE[2]))
        .border_t_1()
        .border_color(t.border)
        .child(
            div()
                .text_size(px(theme::TEXT_SECONDARY))
                .text_color(t.text_muted)
                .child(format!("{count} comments")),
        )
        .when(count > 0, |this| {
            this.child(
                Button::new("request-changes")
                    .label("Request changes")
                    .primary()
                    .small()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        request_changes(this, cx);
                        cx.notify();
                    })),
            )
        })
}

pub(crate) fn status_glyph(status: FileStatus, t: &Tokens) -> (&'static str, Hsla) {
    match status {
        FileStatus::Added => ("A", t.success),
        FileStatus::Modified => ("M", t.warning),
        FileStatus::Deleted => ("D", t.danger),
        FileStatus::Renamed => ("R", t.accent),
        FileStatus::Binary => ("B", t.text_muted),
    }
}

fn muted(text: &'static str, t: &Tokens) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(t.text_muted)
        .child(text)
        .into_any_element()
}
