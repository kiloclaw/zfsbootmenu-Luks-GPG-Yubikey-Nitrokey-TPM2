//! The key bindings: one table for behaviour and help.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::update::Message;

/// What a bound key asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Intent {
    /// Select the entry above.
    Up,
    /// Select the entry below.
    Down,
    /// Boot the selected entry.
    Boot,
    /// Remove the last character of the filter.
    DeleteChar,
    /// Clear the filter.
    ClearFilter,
    /// Clear the filter if there is one, otherwise leave the menu.
    Back,
    /// Leave the menu.
    Quit,
}

/// A key binding: the key, what it does, and how the help line shows it.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    /// The key.
    pub code: KeyCode,
    /// Modifiers that must be held.
    pub modifiers: KeyModifiers,
    /// What it does.
    pub intent: Intent,
    /// The key as the help line shows it, or `None` to leave it out.
    pub label: Option<&'static str>,
    /// What it does, for the help line.
    pub help: &'static str,
}

/// A binding; a function so that the table stays readable.
const fn bind(
    code: KeyCode,
    modifiers: KeyModifiers,
    intent: Intent,
    label: Option<&'static str>,
    help: &'static str,
) -> Binding {
    Binding {
        code,
        modifiers,
        intent,
        label,
        help,
    }
}

/// Every key binding of the boot environment list. The order is the order
/// of the help line, most important first: a narrow screen shows only the
/// first ones.
pub const KEYMAP: &[Binding] = &[
    bind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        Intent::Boot,
        Some("Enter"),
        "boot",
    ),
    bind(
        KeyCode::Esc,
        KeyModifiers::NONE,
        Intent::Back,
        Some("Esc"),
        "back",
    ),
    bind(
        KeyCode::Up,
        KeyModifiers::NONE,
        Intent::Up,
        Some("Up/Down"),
        "select",
    ),
    bind(
        KeyCode::Down,
        KeyModifiers::NONE,
        Intent::Down,
        None,
        "select",
    ),
    bind(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
        Intent::Up,
        None,
        "select",
    ),
    bind(
        KeyCode::Char('n'),
        KeyModifiers::CONTROL,
        Intent::Down,
        None,
        "select",
    ),
    bind(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
        Intent::Quit,
        Some("Ctrl-C"),
        "quit",
    ),
    bind(
        KeyCode::Backspace,
        KeyModifiers::NONE,
        Intent::DeleteChar,
        None,
        "delete",
    ),
    bind(
        KeyCode::Char('u'),
        KeyModifiers::CONTROL,
        Intent::ClearFilter,
        Some("Ctrl-U"),
        "clear",
    ),
];

/// The message for a key event: a binding's intent, a typed character, or
/// nothing for key releases and unbound keys.
pub fn message(event: KeyEvent) -> Option<Message> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let bound = KEYMAP
        .iter()
        .find(|binding| binding.code == event.code && event.modifiers == binding.modifiers);
    if let Some(binding) = bound {
        return Some(Message::Intent(binding.intent));
    }
    match event.code {
        KeyCode::Char(c)
            if !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            Some(Message::Type(c))
        }
        _ => None,
    }
}

/// The help line for a screen `width` columns wide: the labelled bindings
/// in order, as `[Enter] boot  [Esc] back`, as many whole ones as fit.
pub fn help_line(width: usize) -> String {
    let mut line = String::new();
    for binding in KEYMAP {
        let Some(label) = binding.label else {
            continue;
        };
        let part = format!("[{label}] {}", binding.help);
        let separator = if line.is_empty() { "" } else { "  " };
        if line.len() + separator.len() + part.len() > width {
            break;
        }
        line.push_str(separator);
        line.push_str(&part);
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn bound_keys_become_intents() {
        let cases = [
            (key(KeyCode::Enter, KeyModifiers::NONE), Intent::Boot),
            (key(KeyCode::Up, KeyModifiers::NONE), Intent::Up),
            (key(KeyCode::Char('n'), KeyModifiers::CONTROL), Intent::Down),
            (key(KeyCode::Char('c'), KeyModifiers::CONTROL), Intent::Quit),
            (key(KeyCode::Esc, KeyModifiers::NONE), Intent::Back),
        ];

        for (event, intent) in cases {
            assert_eq!(message(event), Some(Message::Intent(intent)), "{event:?}");
        }
    }

    #[test]
    fn plain_and_shifted_characters_are_typed() {
        assert_eq!(
            message(key(KeyCode::Char('v'), KeyModifiers::NONE)),
            Some(Message::Type('v'))
        );
        assert_eq!(
            message(key(KeyCode::Char('R'), KeyModifiers::SHIFT)),
            Some(Message::Type('R'))
        );
    }

    #[test]
    fn unbound_control_keys_and_releases_do_nothing() {
        let mut release = key(KeyCode::Enter, KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;

        assert_eq!(
            message(key(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(message(key(KeyCode::F(5), KeyModifiers::NONE)), None);
        assert_eq!(message(release), None);
    }

    #[test]
    fn the_help_line_lists_the_labelled_bindings_in_order() {
        assert_eq!(
            help_line(80),
            "[Enter] boot  [Esc] back  [Up/Down] select  [Ctrl-C] quit  [Ctrl-U] clear"
        );
    }

    #[test]
    fn a_narrow_help_line_keeps_whole_bindings_only() {
        assert_eq!(help_line(52), "[Enter] boot  [Esc] back  [Up/Down] select");
        assert_eq!(help_line(5), "");
    }
}
