//! The welcome screen: the composer, and the tasks you touched last.

use crate::app::{prompt_preview, LgtmApp, OTHER_REPOSITORY};
use crate::sidebar::{now_ms, relative_age, repo_slug, status_color};
use crate::theme::{
    field, section_label, tokens, Tokens, RADIUS, RADIUS_PILL, SPACE, TEXT_SECONDARY, TEXT_TITLE,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::select::Select;
use gpui_component::switch::Switch;
use lgtm_protocol::Task;

const RECENT: usize = 6;
/// `max-w-xl`, the width of the reference welcome column.
const COLUMN: f32 = 576.;
const TILE: f32 = 48.;
const SUBTITLE: &str = "Describe a change and let an agent do it, or open a task to review.";
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
                .gap(px(SPACE[4]))
                .child(masthead(&t))
                .child(composer(app, &t, cx))
                .when(empty, |this| {
                    this.child(
                        div()
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.muted_fg)
                            .child(EMPTY_HINT),
                    )
                })
                .when(!empty, |this| this.child(recent(app, &t, cx))),
        )
        .into_any_element()
}

fn masthead(t: &Tokens) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(SPACE[2]))
        .child(
            div()
                .w(px(TILE))
                .h(px(TILE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_PILL))
                .bg(t.muted)
                .text_size(px(20.))
                .text_color(t.fg)
                .child("▤"),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(SPACE[0]))
                .child(
                    div()
                        .text_size(px(TEXT_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .child("LGTM"),
                )
                .child(div().text_color(t.muted_fg).child(SUBTITLE)),
        )
}

fn composer(app: &mut LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let other = app.repository_is_other(cx);
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[2]))
        .p(px(SPACE[3]))
        .rounded(px(RADIUS))
        .bg(t.card)
        .border_1()
        .border_color(t.border)
        .child(field(&app.prompt, t))
        .when(other, |this| this.child(field(&app.repo_url, t)))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(SPACE[1]))
                .child(
                    div().w(px(180.)).child(picker(
                        Select::new(&app.repo_select)
                            .placeholder("Repository")
                            .cleanable(false),
                        t,
                    )),
                )
                .child(div().w(px(120.)).child(field(&app.base_branch, t)))
                .child(
                    div().w(px(150.)).child(picker(
                        Select::new(&app.worker_select)
                            .placeholder("Worker")
                            .cleanable(false),
                        t,
                    )),
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

/// Selects default to the page background and a visible border; the reference
/// pickers are `bg-input/50` pills with no border, like every other field.
fn picker<D>(select: Select<D>, t: &Tokens) -> Select<D>
where
    D: gpui_component::select::SelectDelegate,
{
    select
        .bg(t.input_fill)
        .border_color(gpui::transparent_black())
}

fn recent(app: &LgtmApp, t: &Tokens, cx: &mut Context<LgtmApp>) -> Div {
    let now = now_ms();
    div()
        .flex()
        .flex_col()
        .gap(px(SPACE[1]))
        .child(section_label("Recent tasks", t).px(px(SPACE[0])))
        .child(
            div()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(RADIUS))
                .bg(t.card)
                .border_1()
                .border_color(t.border)
                .children(
                    app.tasks
                        .iter()
                        .take(RECENT)
                        .enumerate()
                        .map(|(index, task)| row(app, task, index, now, t, cx)),
                ),
        )
}

fn row(
    app: &LgtmApp,
    task: &Task,
    index: usize,
    now: u64,
    t: &Tokens,
    cx: &mut Context<LgtmApp>,
) -> gpui::Stateful<Div> {
    let id = task.id.clone();
    let worker = task.worker.clone().unwrap_or_else(|| "unassigned".into());
    div()
        .id(SharedString::from(format!("recent-{id}")))
        .flex()
        .items_center()
        .gap(px(SPACE[2]))
        .px(px(SPACE[2]))
        .py(px(10.))
        .cursor_pointer()
        .when(index > 0, |this| this.border_t_1().border_color(t.border))
        .hover(|this| this.bg(t.muted))
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
                .flex()
                .flex_col()
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(prompt_preview(&task.spec.prompt, 64)),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(TEXT_SECONDARY))
                        .text_color(t.muted_fg)
                        .child(format!("{} · {worker}", repo_slug(&task.spec.repository))),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(TEXT_SECONDARY))
                .text_color(t.muted_fg)
                .child(format!("{} ago", relative_age(task.created_at, now))),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(id.clone(), cx)))
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
