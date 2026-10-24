//! The differences view: marking changed paths and restoring them from the
//! snapshot, and the packages that changed.

use super::dialogs;
use crate::menu::keymap::Intent;
use crate::menu::model::{DiffView, Model, Segment, Then};
use crate::menu::world::{Action, Outcome, RESTORE_SNAPSHOT};
use crate::text::text;

/// How many paths the restore confirmation lists.
const LISTED: usize = 4;

/// What `intent` does in the differences view.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    let Some(diff) = &mut model.diff else {
        return Vec::new();
    };
    match (diff.segment, intent) {
        (_, Intent::Segment) => diff.switch(),
        (Segment::Files, Intent::Mark) => diff.toggle_mark(),
        (Segment::Files, Intent::Primary) => restore(model),
        (Segment::Packages, Intent::Primary) => diff.show_package_files(),
        _ => {}
    }
    Vec::new()
}

/// Asks to restore the marked paths, or the selected one: a typed
/// confirmation that says what is replaced and how to undo it.
fn restore(model: &mut Model) {
    let Some(diff) = &model.diff else {
        return;
    };
    let paths = diff.to_restore();
    if paths.is_empty() {
        let created = diff.selected().map_or_else(String::new, |c| c.path.clone());
        model.notification = Some(Outcome::Warning(text!(
            "restore-nothing",
            path = created,
            snapshot = diff.from.name()
        )));
        return;
    }
    model.dialog = Some(dialogs::confirm(confirmation(diff, paths)));
}

/// The confirmation of restoring `paths` from the snapshot of `diff`.
fn confirmation(diff: &DiffView, paths: Vec<String>) -> Then {
    let environment = diff.from.dataset();
    let mut lines: Vec<String> = paths.iter().take(LISTED).cloned().collect();
    if paths.len() > LISTED {
        lines.push(text!("restore.more", count = paths.len() - LISTED));
    }
    lines.push(text!("restore", environment = environment.to_string()));
    lines.push(text!(
        "restore.safety",
        pool = environment.pool().to_string(),
        safety = format!("{environment}@{RESTORE_SNAPSHOT}")
    ));
    Then::Confirm {
        title: text!(
            "restore.title",
            count = paths.len(),
            snapshot = diff.from.name()
        ),
        action: Action::RestoreFiles {
            snapshot: diff.from.clone(),
            paths,
        },
        lines,
        name: environment.last_component().to_owned(),
    }
}
