//! The welcome screen: the composer, and the tasks you touched last.

use crate::app::{prompt_preview, LgtmApp};
use crate::sidebar::{now_ms, relative_age, repo_slug, status_color};
use crate::theme::{section_label, tokens, Tokens, RADIUS, RADIUS_PILL, SPACE, TEXT_SECONDARY};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, ClickEvent, Context, Div, FontWeight, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use lgtm_protocol::{Executor, Task, TaskKind, TaskSpec};

const RECENT: usize = 6;
/// `max-w-xl`, the width of the reference welcome column.
const COLUMN: f32 = 576.;
/// The round send button.
const SEND: f32 = 28.;
pub const AUTO_WORKER: &str = "Auto";

/// One choice made in the composer's `+` menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chip {
    Plan,
    Worker(String),
    Branch(String),
}

impl Chip {
    pub fn label(&self) -> String {
        match self {
            Chip::Plan => "Plan".to_string(),
            Chip::Worker(name) => name.clone(),
            Chip::Branch(branch) => branch.clone(),
        }
    }

    /// Two chips of the same kind replace each other; Plan toggles.
    fn same_kind(&self, other: &Chip) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// What the `+` menu is showing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlusView {
    Root,
    Workers,
    Branch,
}

/// The task the composer would start, or None while it is incomplete.
pub fn compose(prompt: &str, project: Option<&str>, chips: &[Chip]) -> Option<TaskSpec> {
    let prompt = prompt.trim();
    let repository = project.map(str::trim).filter(|url| !url.is_empty())?;
    if prompt.is_empty() {
        return None;
    }
    let mut base_branch = "main".to_string();
    let mut worker = None;
    let mut kind = TaskKind::Run;
    for chip in chips {
        match chip {
            Chip::Plan => kind = TaskKind::Plan,
            Chip::Worker(name) if name != AUTO_WORKER => worker = Some(name.clone()),
            Chip::Worker(_) => worker = None,
            Chip::Branch(branch) if !branch.trim().is_empty() => {
                base_branch = branch.trim().to_string()
            }
            Chip::Branch(_) => {}
        }
    }
    Some(TaskSpec {
        repository: repository.to_string(),
        base_branch,
        prompt: prompt.to_string(),
        executor: Executor::Claude,
        worker,
        issue: None,
        linear: None,
        kind,
        parent: None,
        depends_on: vec![],
        batch: None,
    })
}

impl LgtmApp {
    /// Adds a chip, replacing one of the same kind. Plan toggles off.
    pub fn set_chip(&mut self, chip: Chip, cx: &mut Context<Self>) {
        if let Some(at) = self.chips.iter().position(|held| held.same_kind(&chip)) {
            let held = self.chips.remove(at);
            if held == chip {
                cx.notify();
                return;
            }
        }
        self.chips.push(chip);
        cx.notify();
    }

    pub fn remove_chip(&mut self, chip: &Chip, cx: &mut Context<Self>) {
        self.chips.retain(|held| held != chip);
        cx.notify();
    }
}

pub fn home(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) -> AnyElement {
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
                .gap(px(SPACE[2]))
                .child(
                    div()
                        .flex()
                        .child(crate::composer::project_chip(app, &t, cx))
                        .child(div().flex_1()),
                )
                .child(crate::composer::card(app, &t, window, cx))
                .when_some(app.error.clone(), |this, error| {
                    this.child(
                        div()
                            .text_size(px(TEXT_SECONDARY))
                            .text_color(t.danger)
                            .child(error),
                    )
                })
                .when(!empty, |this| {
                    this.child(div().h(px(SPACE[2]))).child(recent(app, &t, cx))
                }),
        )
        .into_any_element()
}

/// The round primary send button.
pub fn send_button(enabled: bool, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    div()
        .id("send")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(SEND))
        .h(px(SEND))
        .rounded(px(SEND / 2.))
        .bg(if enabled { t.primary } else { t.muted })
        .text_color(if enabled { t.primary_fg } else { t.muted_fg })
        .when(enabled, |this| this.cursor_pointer())
        .child("↑")
        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
            if enabled {
                this.submit(window, cx);
            }
        }))
}

/// One removable chip in the composer's bottom row.
pub fn chip_pill(chip: &Chip, t: &Tokens, cx: &mut Context<LgtmApp>) -> impl IntoElement {
    let held = chip.clone();
    div()
        .id(SharedString::from(format!("chip-{}", chip.label())))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(SPACE[0]))
        .h(px(20.))
        .px(px(SPACE[1]))
        .rounded(px(RADIUS_PILL))
        .bg(t.muted)
        .text_size(px(TEXT_SECONDARY))
        .text_color(t.fg)
        .cursor_pointer()
        .hover(|this| this.text_color(t.danger))
        .child(chip.label())
        .child(div().text_color(t.muted_fg).child("✕"))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.remove_chip(&held, cx)))
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

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://github.com/you/repo.git";

    #[test]
    fn nothing_composes_without_a_prompt_or_a_project() {
        assert!(compose("", Some(URL), &[]).is_none());
        assert!(compose("   ", Some(URL), &[]).is_none());
        assert!(compose("do it", None, &[]).is_none());
        assert!(compose("do it", Some("  "), &[]).is_none());
    }

    #[test]
    fn defaults_are_main_no_worker_and_a_run() {
        let spec = compose("do it", Some(URL), &[]).unwrap();
        assert_eq!(spec.repository, URL);
        assert_eq!(spec.base_branch, "main");
        assert_eq!(spec.prompt, "do it");
        assert!(spec.worker.is_none());
        assert_eq!(spec.kind, TaskKind::Run);
    }

    #[test]
    fn chips_choose_the_kind_the_worker_and_the_branch() {
        let chips = vec![
            Chip::Plan,
            Chip::Worker("MacBook".into()),
            Chip::Branch("develop".into()),
        ];
        let spec = compose("do it", Some(URL), &chips).unwrap();
        assert_eq!(spec.kind, TaskKind::Plan);
        assert_eq!(spec.worker.as_deref(), Some("MacBook"));
        assert_eq!(spec.base_branch, "develop");
    }

    #[test]
    fn the_auto_worker_leaves_the_choice_to_the_orchestrator() {
        let chips = vec![Chip::Worker(AUTO_WORKER.into())];
        assert!(compose("do it", Some(URL), &chips)
            .unwrap()
            .worker
            .is_none());
    }
}
