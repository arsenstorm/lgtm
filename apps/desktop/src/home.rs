//! The welcome screen: an empty stage, with the composer pinned to the bottom.

use crate::app::LgtmApp;
use crate::theme::{icon, tokens, Models, Tokens, ICON, RADIUS, SPACE};
use gpui::{
    div, px, AnyElement, Context, FontWeight, Hsla, IntoElement, ParentElement as _, Styled as _,
    Window,
};
use lgtm_protocol::{TaskKind, TaskSpec};

/// The outlined mark over the greeting.
const MARK: f32 = 44.;
/// `text-[22px]`: the greeting.
const GREETING: f32 = 22.;
pub const AUTO_RUNNER: &str = "Auto";

/// One choice made in the composer's controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chip {
    Plan,
    Runner(String),
    Branch(String),
}

impl Chip {
    /// Two chips of the same kind replace each other; Plan toggles.
    fn same_kind(&self, other: &Chip) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// The base branch the chips ask for; `main` when they ask for none.
pub fn branch_of(chips: &[Chip]) -> String {
    chips
        .iter()
        .find_map(|chip| match chip {
            Chip::Branch(branch) if !branch.trim().is_empty() => Some(branch.trim().to_string()),
            _ => None,
        })
        .unwrap_or_else(|| "main".to_string())
}

/// The runner the chips pin the task to, or `None` for the orchestrator's pick.
pub fn runner_of(chips: &[Chip]) -> Option<String> {
    chips.iter().find_map(|chip| match chip {
        Chip::Runner(name) if name != AUTO_RUNNER => Some(name.clone()),
        _ => None,
    })
}

/// The task the composer would start, or None while it is incomplete. What the
/// chips do not say comes from Settings → Models.
pub fn compose(
    prompt: &str,
    project: Option<&str>,
    chips: &[Chip],
    models: &Models,
) -> Option<TaskSpec> {
    let prompt = prompt.trim();
    let repository = project.map(str::trim).filter(|url| !url.is_empty())?;
    if prompt.is_empty() {
        return None;
    }
    let kind = match chips.contains(&Chip::Plan) {
        true => TaskKind::Plan,
        false => TaskKind::Run,
    };
    Some(TaskSpec {
        repository: repository.to_string(),
        base_branch: branch_of(chips),
        prompt: prompt.to_string(),
        executor: models.executor,
        runner: runner_of(chips),
        issue: None,
        linear: None,
        kind,
        parent: None,
        depends_on: vec![],
        depends_on_condition: Default::default(),
        batch: None,
        sandbox: None,
        requirements: vec![],
        goal: None,
        review_executor: models.review.executor(),
        model: Some(models.model.clone()).filter(|model| !model.trim().is_empty()),
        allowed_hosts: Vec::new(),
        session: None,
        created_by: None,
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

    /// Closes every transient menu. Esc and a click outside land here.
    pub fn close_menus(&mut self, cx: &mut Context<Self>) {
        self.ui.runner_menu = false;
        self.ui.session_project_menu = false;
        self.composer.project_menu = false;
        self.composer.add_repo = false;
        self.composer.plus_menu = false;
        self.composer.branch_edit = false;
        self.composer.runner_menu = false;
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

    use crate::theme::Pick;
    use lgtm_protocol::Executor;

    const URL: &str = "https://github.com/you/repo.git";

    fn models() -> Models {
        Models {
            executor: Executor::Claude,
            model: String::new(),
            review: Pick::Auto,
            orchestrate: Pick::Off,
        }
    }

    fn spec(prompt: &str, project: Option<&str>, chips: &[Chip]) -> Option<TaskSpec> {
        compose(prompt, project, chips, &models())
    }

    #[test]
    fn nothing_composes_without_a_prompt_or_a_project() {
        assert!(spec("", Some(URL), &[]).is_none());
        assert!(spec("   ", Some(URL), &[]).is_none());
        assert!(spec("do it", None, &[]).is_none());
        assert!(spec("do it", Some("  "), &[]).is_none());
    }

    #[test]
    fn defaults_are_main_no_runner_and_a_run() {
        let spec = spec("do it", Some(URL), &[]).unwrap();
        assert_eq!(spec.repository, URL);
        assert_eq!(spec.base_branch, "main");
        assert_eq!(spec.prompt, "do it");
        assert!(spec.runner.is_none());
        assert_eq!(spec.kind, TaskKind::Run);
    }

    #[test]
    fn chips_choose_the_kind_the_runner_and_the_branch() {
        let chips = vec![
            Chip::Plan,
            Chip::Runner("MacBook".into()),
            Chip::Branch("develop".into()),
        ];
        let spec = spec("do it", Some(URL), &chips).unwrap();
        assert_eq!(spec.kind, TaskKind::Plan);
        assert_eq!(spec.runner.as_deref(), Some("MacBook"));
        assert_eq!(spec.base_branch, "develop");
    }

    #[test]
    fn the_auto_runner_leaves_the_choice_to_the_orchestrator() {
        let chips = vec![Chip::Runner(AUTO_RUNNER.into())];
        assert!(spec("do it", Some(URL), &chips).unwrap().runner.is_none());
    }

    #[test]
    fn the_settings_defaults_fill_in_the_harness_the_model_and_the_reviewer() {
        let codex = Models {
            executor: Executor::Codex,
            model: "gpt-5".into(),
            review: Pick::Claude,
            orchestrate: Pick::Off,
        };
        let spec = compose("do it", Some(URL), &[], &codex).unwrap();
        assert_eq!(spec.executor, Executor::Codex);
        assert_eq!(spec.model.as_deref(), Some("gpt-5"));
        assert_eq!(spec.review_executor, Some(Executor::Claude));
        // An empty model field means the harness picks, not an empty flag.
        assert!(compose("do it", Some(URL), &[], &models())
            .unwrap()
            .model
            .is_none());
    }
}
