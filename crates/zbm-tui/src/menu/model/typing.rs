//! The keyboard layout keys are typed on, as the console on the display
//! tells it with each key, and the one `Ctrl-K` chose to translate them
//! into (`docs/dev/menu-design/unlocking.md`).

use super::Model;

/// Where a key was typed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypedOn {
    /// On a keyboard of the display, with the layout of this name.
    Layout(String),
    /// On a terminal, which sends characters in a layout of its own: the
    /// serial terminal the console on the display serves too.
    Characters,
}

impl TypedOn {
    /// What the console's layout name `name` says: an empty one is a
    /// terminal's character.
    pub(crate) fn named(name: String) -> TypedOn {
        if name.is_empty() {
            TypedOn::Characters
        } else {
            TypedOn::Layout(name)
        }
    }
}

impl Model {
    /// The keyboard layout the unlock dialog shows: the one `Ctrl-K` chose;
    /// until it chose one, the layout keys are typed on, which nothing
    /// translates. After a Super+Space, the console names the new layout
    /// with the first key typed on it, and not before.
    pub fn layout(&self) -> Option<&str> {
        match self.layout {
            Some(chosen) => self
                .snapshot
                .keyboard
                .layouts
                .get(chosen)
                .map(String::as_str),
            None => self.typed_layout(),
        }
    }

    /// The name of the layout keys are typed on: the one the input names,
    /// or the console's own, the first, when no input says; none for a
    /// terminal's characters.
    pub(crate) fn typed_layout(&self) -> Option<&str> {
        match &self.typed_on {
            Some(TypedOn::Layout(name)) => Some(name),
            Some(TypedOn::Characters) => None,
            None => self.snapshot.keyboard.layouts.first().map(String::as_str),
        }
    }

    /// Whether keys come from a terminal, which sends characters in its own
    /// layout: the serial terminal the console on the display serves, as
    /// the input says, or, when no input says, a serial console or SSH. A
    /// key the input names a layout for is a keyboard's, whatever terminal
    /// the menu's own console is.
    pub(crate) fn is_typed_on_a_terminal(&self) -> bool {
        match &self.typed_on {
            Some(TypedOn::Layout(_)) => false,
            Some(TypedOn::Characters) => true,
            None => !self.snapshot.keyboard.local,
        }
    }
}
