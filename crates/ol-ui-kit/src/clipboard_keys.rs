use gpui::{Action, App, KeyBinding, Keymap, NoAction, OperatingSystem};
use gpui_kit::base::input::{Copy, Cut, Paste};

fn reaches_the_clipboard(action: &dyn Action) -> bool {
    let action = action.as_any();
    action.is::<Copy>() || action.is::<Cut>() || action.is::<Paste>()
}

fn refusals(keymap: &Keymap) -> Vec<KeyBinding> {
    keymap
        .bindings()
        .filter(|binding| reaches_the_clipboard(binding.action()))
        .map(|binding| {
            let keys = binding
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.inner().unparse(OperatingSystem::Windows))
                .collect::<Vec<_>>()
                .join(" ");
            let context = binding.predicate().map(|predicate| predicate.to_string());
            // A run binds these once, and a binding takes its keys for good.
            let keys: &'static str = Box::leak(keys.into_boxed_str());
            KeyBinding::new(keys, NoAction, context.as_deref())
        })
        .collect()
}

// For a run that agents drive: no key reaches the Windows clipboard through a text field.
pub fn refuse_clipboard_keys(cx: &mut App) {
    let refusals = refusals(&cx.key_bindings().borrow());
    cx.bind_keys(refusals);
}

#[cfg(test)]
mod tests {
    use gpui::{KeyContext, Keystroke};
    use gpui_kit::base::input::SelectAll;

    use super::*;

    fn bound(keymap: &Keymap, key: &'static str) -> Vec<&'static str> {
        let context = [KeyContext::parse("Input").unwrap()];
        keymap
            .bindings_for_input(&[Keystroke::parse(key).unwrap()], &context)
            .0
            .iter()
            .map(|binding| binding.action().name())
            .collect()
    }

    #[test]
    fn a_key_that_copies_cuts_or_pastes_reaches_nothing_and_the_others_still_work() {
        let mut keymap = Keymap::new(vec![
            KeyBinding::new("ctrl-c", Copy, Some("Input")),
            KeyBinding::new("ctrl-insert", Copy, Some("Input")),
            KeyBinding::new("ctrl-x", Cut, Some("Input")),
            KeyBinding::new("shift-delete", Cut, Some("Input")),
            KeyBinding::new("ctrl-v", Paste, Some("Input")),
            KeyBinding::new("shift-insert", Paste, Some("Input")),
            KeyBinding::new("ctrl-a", SelectAll, Some("Input")),
        ]);
        assert_eq!(bound(&keymap, "ctrl-v"), ["input::Paste"]);
        let refusals = refusals(&keymap);
        assert_eq!(refusals.len(), 6);
        keymap.add_bindings(refusals);
        for key in [
            "ctrl-c",
            "ctrl-insert",
            "ctrl-x",
            "shift-delete",
            "ctrl-v",
            "shift-insert",
        ] {
            assert!(
                bound(&keymap, key).is_empty(),
                "{key} still reaches the clipboard"
            );
        }
        assert_eq!(bound(&keymap, "ctrl-a"), ["input::SelectAll"]);
    }
}
