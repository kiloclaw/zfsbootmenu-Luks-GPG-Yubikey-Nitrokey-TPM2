//! The differences view: marking changed paths and restoring them from the
//! snapshot.

use super::dialogs;
use crate::menu::keymap::Intent;
use crate::menu::model::{DiffView, Model, Then};
use crate::menu::world::{Action, Outcome};

/// How many paths the restore confirmation lists.
const LISTED: usize = 4;

/// What `intent` does in the differences view.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    match intent {
        Intent::Mark => {
            if let Some(diff) = &mut model.diff {
                diff.toggle_mark();
            }
        }
        Intent::Primary => restore(model),
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
        model.notification = Some(Outcome::Warning(format!(
            "{created} was created after {}: there is nothing to restore",
            diff.from.name()
        )));
        return;
    }
    model.dialog = Some(dialogs::confirm(confirmation(diff, paths)));
}

/// The confirmation of restoring `paths` from the snapshot of `diff`.
fn confirmation(diff: &DiffView, paths: Vec<String>) -> Then {
    let environment = diff.from.dataset();
    let files = if paths.len() == 1 { "File" } else { "Files" };
    let mut lines: Vec<String> = paths.iter().take(LISTED).cloned().collect();
    if paths.len() > LISTED {
        lines.push(format!("and {} more", paths.len() - LISTED));
    }
    lines.push(format!(
        "They replace the files in {environment} as they are now."
    ));
    lines.push(format!(
        "{} is imported read/write, and a safety snapshot {environment}@before-restore is \
         taken first: roll back to it to undo.",
        environment.pool()
    ));
    Then::Confirm {
        title: format!("Restore {} {files} from {}", paths.len(), diff.from.name()),
        action: Action::RestoreFiles {
            snapshot: diff.from.clone(),
            paths,
        },
        lines,
        name: environment.last_component().to_owned(),
    }
}
