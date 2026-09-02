//! The keymap: every action the window answers to, and the keys that reach it.

use gpui::{actions, App, KeyBinding};

actions!(
    lgtm,
    [
        NewSession,
        OpenPalette,
        ToggleSidebar,
        SelectNext,
        SelectPrev,
        ShowActivity,
        ShowChanges,
        ShowReview,
        ShowPlan,
        Submit,
        CloseOverlay,
        PaletteNext,
        PalettePrev,
        PaletteRun,
    ]
);

pub const CONTEXT: &str = "Lgtm";
/// What the window adds to its context while a task is open. The list and
/// review keys live in here rather than in `Lgtm`, so that a plain letter typed
/// on a page with a composer is text for the prompt, not a shortcut.
pub const PANE_CONTEXT: &str = "Lgtm Panes";
/// The key context the palette's input runs in. Bound deeper than `Lgtm` so
/// ↑↓↩␛ reach the list instead of the text field.
const PALETTE_CONTEXT: &str = "Palette > Input";

/// The keymap, built as data so a test can assert the bindings survive a
/// refactor. `!Input` keeps the single-letter keys out of the way while a text
/// field has focus; the ⌘ bindings stay live everywhere.
fn bindings() -> Vec<KeyBinding> {
    let anywhere = Some(CONTEXT);
    let outside_inputs = Some("Lgtm && !Input");
    let panes = Some("Panes && !Input");
    let palette = Some(PALETTE_CONTEXT);
    vec![
        KeyBinding::new("cmd-n", NewSession, anywhere),
        KeyBinding::new("cmd-k", OpenPalette, anywhere),
        KeyBinding::new("cmd-b", ToggleSidebar, anywhere),
        KeyBinding::new("cmd-enter", Submit, anywhere),
        KeyBinding::new("escape", CloseOverlay, outside_inputs),
        KeyBinding::new("escape", CloseOverlay, palette),
        KeyBinding::new("up", PalettePrev, palette),
        KeyBinding::new("down", PaletteNext, palette),
        KeyBinding::new("enter", PaletteRun, palette),
        KeyBinding::new("j", SelectNext, panes),
        KeyBinding::new("k", SelectPrev, panes),
        KeyBinding::new("1", ShowActivity, panes),
        KeyBinding::new("2", ShowChanges, panes),
        KeyBinding::new("3", ShowReview, panes),
        KeyBinding::new("4", ShowPlan, panes),
        KeyBinding::new("v", crate::review::MarkViewed, panes),
        KeyBinding::new("n", crate::review::NextFile, panes),
        KeyBinding::new("p", crate::review::PrevFile, panes),
        KeyBinding::new("s", crate::review::ToggleDiffStyle, panes),
    ]
}

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every single-key binding, in the order a person would try them.
    const SINGLE: [&str; 10] = ["1", "2", "3", "4", "j", "k", "v", "n", "p", "s"];

    /// The pane and review keys are the ones a restyle is most likely to drop,
    /// because the widgets they drive get swapped out. Assert the keymap still
    /// resolves them, in the context that excludes focused text fields.
    fn bound(key: &str) -> KeyBinding {
        bindings()
            .into_iter()
            .find(|binding| match binding.keystrokes() {
                [only] => only.inner().key == key && !only.inner().modifiers.modified(),
                _ => false,
            })
            .unwrap_or_else(|| panic!("nothing bound to {key}"))
    }

    #[test]
    fn the_digits_switch_panes_in_tab_order() {
        for (key, action) in [
            ("1", "ShowActivity"),
            ("2", "ShowChanges"),
            ("3", "ShowReview"),
            ("4", "ShowPlan"),
        ] {
            let binding = bound(key);
            assert!(
                binding.action().name().ends_with(action),
                "{key} runs {} not {action}",
                binding.action().name()
            );
        }
    }

    /// Typing on a page with a composer has to reach the prompt, so no bare key
    /// may be bound in the window's own context: they all wait for a task.
    #[test]
    fn the_single_keys_only_answer_while_a_task_is_open() {
        for key in SINGLE {
            let predicate = format!("{:?}", bound(key).predicate());
            assert!(predicate.contains("Panes"), "{key} is bound in {predicate}");
            assert!(predicate.contains("Input"), "{key} is bound in {predicate}");
        }
        assert!(PANE_CONTEXT.contains(CONTEXT));
    }

    #[test]
    fn review_keys_reach_the_review_actions() {
        for (key, action) in [
            ("v", "MarkViewed"),
            ("n", "NextFile"),
            ("p", "PrevFile"),
            ("s", "ToggleDiffStyle"),
        ] {
            assert!(bound(key).action().name().ends_with(action));
        }
    }

    /// The palette's keys have to win over the text field's own bindings, which
    /// only happens while they are bound in a context under `Input`.
    #[test]
    fn palette_keys_are_bound_below_the_input_context() {
        let palette: Vec<KeyBinding> = bindings()
            .into_iter()
            .filter(|binding| format!("{:?}", binding.predicate()).contains("Palette"))
            .collect();
        assert_eq!(palette.len(), 4);
        for binding in palette {
            assert!(format!("{:?}", binding.predicate()).contains("Input"));
        }
    }
}
