//! Moving the selection of a list.

use crate::menu::keymap::{self, Intent};
use crate::menu::model::{Model, Screen};

/// How far a page moves.
const PAGE: usize = 10;

/// Moves the selection of the current screen's list.
pub(super) fn move_selection(model: &mut Model, intent: Intent) {
    let last = text_lines(model).saturating_sub(1);
    if let Some(list) = model.list_mut() {
        let last = list.matches.len().saturating_sub(1);
        list.selected = moved(list.selected, last, intent);
    } else {
        model.scroll = moved(model.scroll, last, intent);
    }
}

/// `position` moved by `intent`, between 0 and `last`.
fn moved(position: usize, last: usize, intent: Intent) -> usize {
    match intent {
        Intent::Up => position.saturating_sub(1),
        Intent::Down => (position + 1).min(last),
        Intent::PageUp => position.saturating_sub(PAGE),
        Intent::PageDown => (position + PAGE).min(last),
        Intent::First => 0,
        Intent::Last => last,
        _ => position,
    }
}

/// The lines of help or the log, which scroll instead of selecting.
fn text_lines(model: &Model) -> usize {
    match model.screen {
        Screen::Help(context) => {
            keymap::help(model.map, context).len() + keymap::classic_bindings(context).len() + 2
        }
        Screen::Log => model.snapshot.log.entries.len(),
        _ => 0,
    }
}
