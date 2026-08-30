//! The Memories and TODOs tabs: what the project knows, and what it owes.

use super::muted;
use crate::app::LgtmApp;
use crate::net::Action;
use crate::theme::{field, icon_button, section_label, Tokens, RADIUS, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, Entity, IntoElement, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::Sizable as _;
use lgtm_protocol::{Memory, Todo, TodoStatus};

pub(super) fn memories(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut out = vec![
        section_label("Memories", t).into_any_element(),
        add_row(
            &app.inputs.memory,
            "add-memory",
            t,
            cx.listener(|this, _: &ClickEvent, window, cx| this.add_memory(window, cx)),
        )
        .into_any_element(),
    ];
    if app.memories.is_empty() {
        out.push(muted("Nothing remembered for this project.", t));
    }
    out.extend(
        app.memories
            .iter()
            .map(|memory| memory_row(memory, t, cx).into_any_element()),
    );
    out
}

pub(super) fn todos(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Vec<AnyElement> {
    let mut out = vec![
        section_label("TODOs", t).into_any_element(),
        add_row(
            &app.inputs.todo,
            "add-todo",
            t,
            cx.listener(|this, _: &ClickEvent, window, cx| this.add_todo(window, cx)),
        )
        .into_any_element(),
    ];
    if app.todos.is_empty() {
        out.push(muted("Nothing on the list.", t));
    }
    out.extend(
        app.todos
            .iter()
            .map(|todo| todo_row(todo, t, cx).into_any_element()),
    );
    out
}

/// The field a list is added to, and the button that commits it. ↩ in the
/// field does the same thing.
fn add_row(
    state: &Entity<InputState>,
    id: &'static str,
    t: &Tokens,
    add: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .child(div().flex_1().min_w_0().child(field(state, t).small()))
        .child(Button::new(id).label("Add").primary().small().on_click(add))
}

/// The surface every row in these lists shares.
fn row(t: &Tokens) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(SPACE[1]))
        .p(px(SPACE[1]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
}

fn memory_row(memory: &Memory, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let id = memory.id.clone();
    row(t)
        .child(div().flex_1().min_w_0().child(memory.content.clone()))
        .child(delete(
            &memory.id,
            t,
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.act_on(id.clone(), Action::DeleteMemory, cx)
            }),
        ))
}

fn todo_row(todo: &Todo, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let done = todo.status == TodoStatus::Done;
    let (finish, promote, remove) = (todo.id.clone(), todo.clone(), todo.id.clone());
    row(t)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().truncate().child(todo.title.clone()))
                .when(!todo.description.trim().is_empty(), |this| {
                    this.child(
                        div()
                            .truncate()
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.muted_fg)
                            .child(todo.description.clone()),
                    )
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(status_label(todo.status)),
        )
        .when(!done, |this| {
            this.child(
                Button::new(SharedString::from(format!("done:{}", todo.id)))
                    .label("Done")
                    .outline()
                    .small()
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.act_on(finish.clone(), Action::FinishTodo, cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("promote:{}", todo.id)))
                    .label("Promote")
                    .outline()
                    .small()
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.promote_todo(&promote, window, cx)
                    })),
            )
        })
        .child(delete(
            &todo.id,
            t,
            cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.act_on(remove.clone(), Action::DeleteTodo, cx)
            }),
        ))
}

fn status_label(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Open => "open",
        TodoStatus::InProgress => "in progress",
        TodoStatus::Done => "done",
    }
}

fn delete(
    id: &str,
    t: &Tokens,
    click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    icon_button(
        SharedString::from(format!("delete:{id}")),
        "trash-2",
        true,
        t,
    )
    .on_click(click)
}
