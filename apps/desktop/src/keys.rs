//! The keymap: every action the window answers to, and the keys that reach it.

use gpui::{actions, App, KeyBinding};

actions!(
    lgtm,
    [
        NewTask,
        OpenPalette,
        ToggleSidebar,
        SelectNext,
        SelectPrev,
        ShowActivity,
        ShowChanges,
        ShowChecks,
        ShowPlan,
        Submit,
        CloseOverlay,
        PaletteNext,
        PalettePrev,
        PaletteRun,
    ]
);

pub const CONTEXT: &str = "Lgtm";
/// The key context the palette's input runs in. Bound deeper than `Lgtm` so
/// ↑↓↩␛ reach the list instead of the text field.
const PALETTE_CONTEXT: &str = "Palette > Input";

/// The keymap, built as data so a test can assert the bindings survive a
/// refactor. `!Input` keeps the single-letter keys out of the way while a text
/// field has focus; the ⌘ bindings stay live everywhere.
fn bindings() -> Vec<KeyBinding> {
    let anywhere = Some(CONTEXT);
    let outside_inputs = Some("Lgtm && !Input");
    let palette = Some(PALETTE_CONTEXT);
    vec![
        KeyBinding::new("cmd-n", NewTask, anywhere),
        KeyBinding::new("cmd-k", OpenPalette, anywhere),
        KeyBinding::new("cmd-b", ToggleSidebar, anywhere),
        KeyBinding::new("cmd-enter", Submit, anywhere),
        KeyBinding::new("escape", CloseOverlay, outside_inputs),
        KeyBinding::new("escape", CloseOverlay, palette),
        KeyBinding::new("up", PalettePrev, palette),
        KeyBinding::new("down", PaletteNext, palette),
        KeyBinding::new("enter", PaletteRun, palette),
        KeyBinding::new("j", SelectNext, outside_inputs),
        KeyBinding::new("k", SelectPrev, outside_inputs),
        KeyBinding::new("1", ShowActivity, outside_inputs),
        KeyBinding::new("2", ShowChanges, outside_inputs),
        KeyBinding::new("3", ShowChecks, outside_inputs),
        KeyBinding::new("4", ShowPlan, outside_inputs),
        KeyBinding::new("v", crate::review::MarkViewed, outside_inputs),
        KeyBinding::new("n", crate::review::NextFile, outside_inputs),
        KeyBinding::new("p", crate::review::PrevFile, outside_inputs),
        KeyBinding::new("s", crate::review::ToggleDiffStyle, outside_inputs),
    ]
}

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings());
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn digits_one_to_four_switch_panes_outside_inputs() {
        for (key, action) in [
            ("1", "ShowActivity"),
            ("2", "ShowChanges"),
            ("3", "ShowChecks"),
            ("4", "ShowPlan"),
        ] {
            let binding = bound(key);
            assert!(
                binding.action().name().ends_with(action),
                "{key} runs {} not {action}",
                binding.action().name()
            );
            assert!(format!("{:?}", binding.predicate()).contains("Lgtm"));
        }
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
