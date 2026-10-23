//! Moving the selection of a list.

use crate::menu::keymap::Intent;
use crate::menu::model::Model;

/// How far a page moves.
const PAGE: usize = 10;

/// Moves the selection of the current screen's list.
pub(super) fn move_selection(model: &mut Model, intent: Intent) {
    let Some(list) = model.list_mut() else {
        return;
    };
    let last = list.matches.len().saturating_sub(1);
    list.selected = match intent {
        Intent::Up => list.selected.saturating_sub(1),
        Intent::Down => (list.selected + 1).min(last),
        Intent::PageUp => list.selected.saturating_sub(PAGE),
        Intent::PageDown => (list.selected + PAGE).min(last),
        Intent::First => 0,
        Intent::Last => last,
        _ => list.selected,
    };
}
