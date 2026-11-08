//! The keyboard layout keys are typed on, as the console on the display
//! tells it with each key, the one `Ctrl-K` chose to translate them into
//! (`docs/dev/menu-design/unlocking.md`), and the one a key is looked up in
//! by its place on a US keyboard (`docs/dev/menu-design/keymap.md`, rule 3).

use std::sync::LazyLock;

use zbm_display::keys::KeyPress;
use zbm_display::layout::Positions;

use super::{Dialog, Model};
use crate::menu::keyboard;
use crate::menu::keymap::{self, Modifiers, Typing};

/// The US layout by key position, for an image without its keymap.
static US: LazyLock<Positions> = LazyLock::new(Positions::us);

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
            Some(_) => self.chosen_layout(),
            None => self.typed_layout(),
        }
    }

    /// The name of the layout `Ctrl-K` chose, if it chose one.
    fn chosen_layout(&self) -> Option<&str> {
        let chosen = self.layout?;
        self.snapshot
            .keyboard
            .layouts
            .get(chosen)
            .map(String::as_str)
    }

    /// The layouts `press` is translated from and into by key position,
    /// once `Ctrl-K` chose one: the layout the key was typed on (the
    /// console's own when the input does not say), and the chosen one.
    /// Only in the unlock dialog and for action keys: the filter, the
    /// palette, a name and a typed confirmation take the characters as
    /// typed. Keys with `Ctrl` or `Alt`, a terminal's characters, and
    /// layouts whose keymap did not load are not translated.
    pub(crate) fn translation(&self, press: &KeyPress) -> Option<(&Positions, &Positions)> {
        let held = Modifiers::of(press).has_any(Modifiers::CTRL | Modifiers::ALT);
        let is_field =
            keymap::is_text(self.context()) && !matches!(self.dialog, Some(Dialog::Unlock(_)));
        if held || is_field || self.is_typed_on_a_terminal() {
            return None;
        }
        let positions = &self.snapshot.keyboard.positions;
        Some((
            positions.get(self.typed_layout()?)?,
            positions.get(self.chosen_layout()?)?,
        ))
    }

    /// The layout the character of `press` is of, to look it up by its
    /// place on a US keyboard (rule 3): the one `Ctrl-K` chose when the
    /// key was translated into it, else the one it was typed on. Only for
    /// a keyboard of the display: a terminal's characters, and the kernel
    /// console's, whose keymap the menu is not told, are as typed. None
    /// for the US layout itself and a layout whose keymap did not load.
    pub(crate) fn typing(&self, press: &KeyPress) -> Option<Typing<'_>> {
        let Some(TypedOn::Layout(typed)) = &self.typed_on else {
            return None;
        };
        let name = match self.translation(press) {
            Some(_) => self.chosen_layout()?,
            None => typed.as_str(),
        };
        let positions = &self.snapshot.keyboard.positions;
        let layout = positions.get(name).filter(|_| name != keyboard::US)?;
        let us = positions.get(keyboard::US).unwrap_or(&US);
        Some(Typing { layout, us })
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
