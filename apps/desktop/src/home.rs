//! The composer, shown whenever no task is selected.

use crate::app::{prompt_preview, LgtmApp, OTHER_REPOSITORY};
use crate::sidebar::{now_ms, relative_age, repo_slug, status_color};
use crate::theme::{tokens, Tokens, SPACE, TEXT_SECONDARY, TEXT_TITLE};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::Input;
use gpui_component::select::Select;
use gpui_component::switch::Switch;
use gpui_component::Sizable as _;

const RECENT: usize = 8;
const COLUMN: f32 = 720.;
const EMPTY_HINT: &str =
    "Paste the join line `lgtm serve` printed on another machine to add a worker.";

pub fn home(app: &mut LgtmApp, _window: &mut Window, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    let empty = app.tasks.is_empty();
    div()
        .id("home")
        .flex_1()
        .min_w_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .p(px(SPACE[4]))
        .child(
            div()
                .w_full()
                .max_w(px(COLUMN))
                .flex()
                .flex_col()
                .gap(px(SPACE[3]))
                .child(
                    div()
                        .text_size(px(TEXT_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("What should the agents do?"),
                )
                .child(composer(app, &t, cx))
                .when(empty, |this| {
                    this.child(
                        div()
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.text_muted)
                            .child(EMPTY_HINT),
                    )
                })
                .when(!empty, |this| this.child(recent(app, &t, cx))),
        )
        .into_any_element()
}

fn composer(app: &mut LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let other = app.repository_is_other(cx);
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[2]))
        .p(px(SPACE[2]))
        .rounded(px(12.))
        .bg(t.surface_raised)
        .border_1()
        .border_color(t.border)
        .child(Input::new(&app.prompt))
        .when(other, |this| this.child(Input::new(&app.repo_url)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(
                    div().w(px(200.)).child(
                        Select::new(&app.repo_select)
                            .small()
                            .placeholder("Repository"),
                    ),
                )
                .child(
                    div()
                        .w(px(140.))
                        .child(Input::new(&app.base_branch).small()),
                )
                .child(
                    div().w(px(160.)).child(
                        Select::new(&app.worker_select)
                            .small()
                            .placeholder("Worker"),
                    ),
                )
                .child(
                    Switch::new("plan-first")
                        .label("Plan first")
                        .checked(app.plan_first)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.plan_first = *checked;
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    Button::new("start")
                        .label("Start")
                        .primary()
                        .small()
                        .tooltip("⌘↩")
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, window, cx| this.submit(window, cx)),
                        ),
                ),
        )
        .when_some(app.error.clone(), |this, error| {
            this.child(
                div()
                    .text_size(px(TEXT_SECONDARY))
                    .text_color(t.danger)
                    .child(error),
            )
        })
}

fn recent(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let now = now_ms();
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[0]))
        .child(
            div()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.text_muted)
                .font_weight(FontWeight::SEMIBOLD)
                .child("Recent"),
        )
        .children(app.tasks.iter().take(RECENT).map(|task| {
            let id = task.id.clone();
            div()
                .id(SharedString::from(format!("recent-{id}")))
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .px(px(SPACE[1]))
                .py(px(SPACE[0]))
                .rounded(px(6.))
                .cursor_pointer()
                .hover(|this| this.bg(t.surface))
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(6.))
                        .h(px(6.))
                        .rounded_full()
                        .bg(status_color(task, &app.tasks, t)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(prompt_preview(&task.spec.prompt, 64)),
                )
                .child(
                    div()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.text_muted)
                        .child(repo_slug(&task.spec.repository)),
                )
                .child(
                    div()
                        .w(px(32.))
                        .text_right()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.text_muted)
                        .child(relative_age(task.created_at, now)),
                )
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)),
                )
        }))
}

/// The repository picker's value, resolved back to a clone URL.
pub fn chosen_repository(app: &LgtmApp, cx: &Context<LgtmApp>) -> String {
    let picked = app
        .repo_select
        .read(cx)
        .selected_value()
        .cloned()
        .unwrap_or_default();
    if picked.is_empty() || picked == OTHER_REPOSITORY {
        return app.repo_url.read(cx).value().to_string();
    }
    app.tasks
        .iter()
        .find(|task| repo_slug(&task.spec.repository) == picked)
        .map(|task| task.spec.repository.clone())
        .unwrap_or(picked)
}
