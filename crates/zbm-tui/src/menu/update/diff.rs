//! The differences view: marking changed paths and restoring them from the
//! snapshot, and the packages that changed.

use zbm_types::Untrusted;

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

/// Asks to restore the marked paths, or the selected one, as
/// [`restore_of`] says; says when the selected path is one the snapshot
/// does not have.
fn restore(model: &mut Model) {
    if let Some(then) = restore_of(model) {
        dialogs::follow(model, then);
        return;
    }
    let Some(diff) = model.diff.as_ref().filter(|diff| diff.to.is_none()) else {
        return;
    };
    let created = diff
        .selected()
        .map_or_else(String::new, |c| c.path.printable().to_string());
    model.notification = Some(Outcome::Warning(text!(
        "restore-nothing",
        path = created,
        snapshot = diff.from.name()
    )));
}

/// The restore of the marked paths, or the selected one: a typed
/// confirmation that says what is replaced and how to undo it. Only a diff
/// against the live filesystem restores, and only paths the snapshot has.
pub(super) fn restore_of(model: &Model) -> Option<Then> {
    let diff = model.diff.as_ref().filter(|diff| diff.to.is_none())?;
    let paths = diff.to_restore();
    (!paths.is_empty()).then(|| confirmation(diff, paths))
}

/// The confirmation of restoring `paths` from the snapshot of `diff`. The
/// safety snapshot is named for the time the restore takes it, which the
/// outcome gives, so the confirmation shows only its form.
fn confirmation(diff: &DiffView, paths: Vec<Untrusted<String>>) -> Then {
    let environment = diff.from.dataset();
    let mut lines: Vec<String> = paths
        .iter()
        .take(LISTED)
        .map(|path| path.printable().to_string())
        .collect();
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
