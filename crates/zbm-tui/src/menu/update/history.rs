//! The boot history (ADR 0016, point 7), which the workspace alone shows:
//! its inspector's last boot and its Log tab. The workspace asks for it
//! once the main view is drawn and no countdown runs, and again when the
//! Log tab opens without it, as after Escape cancelled its read; never
//! while the countdown runs, as the boot it ends with would wait for the
//! read, and Escape would cancel the read instead of stopping it.

use crate::menu::keymap::Map;
use crate::menu::model::Model;
use crate::menu::world::Action;

/// Asks the machine for the boot history, unless the menu is not the
/// workspace, the countdown runs, or the history came or is coming.
pub(super) fn load(model: &mut Model) -> Vec<Action> {
    let wanted = model.map == Map::Workspace && model.history.is_none();
    if !wanted || model.countdown.is_some() || model.reading_history {
        return Vec::new();
    }
    model.reading_history = true;
    vec![Action::LoadHistory]
}
