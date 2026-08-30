//! The welcome screen: an empty stage, with the composer pinned to the bottom.

use crate::app::LgtmApp;
use crate::theme::{icon, tokens, Tokens, ICON, RADIUS, SPACE};
use gpui::{
    div, px, AnyElement, Context, FontWeight, Hsla, IntoElement, ParentElement as _, Styled as _,
    Window,
};
use lgtm_protocol::{Executor, TaskKind, TaskSpec};

/// The outlined mark over the greeting.
const MARK: f32 = 44.;
/// `text-[22px]`: the greeting.
const GREETING: f32 = 22.;
pub const AUTO_WORKER: &str = "Auto";

/// One choice made in the composer's controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chip {
    Plan,
    Worker(String),
    Branch(String),
}

impl Chip {
    /// Two chips of the same kind replace each other; Plan toggles.
    fn same_kind(&self, other: &Chip) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
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
        sandbox: None,
        goal: None,
    })
}

impl LgtmApp {
    /// Adds a chip, replacing one of the same kind. Plan toggles off.
    pub fn set_chip(&mut self, chip: Chip, cx: &mut Context<Self>) {
        if let Some(at) = self
            .composer
            .chips
            .iter()
            .position(|held| held.same_kind(&chip))
        {
            let held = self.composer.chips.remove(at);
            if held == chip {
                cx.notify();
                return;
            }
        }
        self.composer.chips.push(chip);
        cx.notify();
    }

    /// Opens one composer menu, or closes it when it was the open one.
    pub fn toggle_menu(&mut self, menu: fn(&mut Self) -> &mut bool, cx: &mut Context<Self>) {
        let open = !*menu(self);
        self.close_menus(cx);
        *menu(self) = open;
        cx.notify();
    }

    /// Closes every composer menu. Esc and a click outside land here.
    pub fn close_menus(&mut self, cx: &mut Context<Self>) {
        self.composer.project_menu = false;
        self.composer.add_repo = false;
        self.composer.plus_menu = false;
        self.composer.branch_edit = false;
        self.composer.worker_menu = false;
        cx.notify();
    }
}

pub fn home(app: &mut LgtmApp, window: &mut Window, cx: &mut Context<LgtmApp>) -> AnyElement {
    let t = tokens(cx);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(stage(&t))
        .child(crate::composer::composer(app, &t, window, cx))
        .into_any_element()
}

/// The empty middle: a mark and the one question the app asks, sitting a
/// little above the centre.
fn stage(t: &Tokens) -> impl IntoElement {
    let muted = Hsla {
        a: 0.6,
        ..t.muted_fg
    };
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(SPACE[3]))
        .pb(px(64.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(MARK))
                .h(px(MARK))
                .rounded(px(RADIUS))
                .border_1()
                .border_color(t.border)
                .child(icon("git-branch", ICON + 4., muted)),
        )
        .child(
            div()
                .text_size(px(GREETING))
                .font_weight(FontWeight::MEDIUM)
                .child("What should the agents do?"),
        )
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
