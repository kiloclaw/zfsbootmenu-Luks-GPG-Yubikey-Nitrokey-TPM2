//! The dry run of the command line (ADR 0007, point 5; ADR 0016, point
//! 12): a change chosen there shows its plan before it runs, on its review
//! page without the field. Its `Enter` makes the dry run pending, which
//! holds while the dialogs it opened are open: a name, a choice, a
//! confirmation. The keys of the views act at once, as they did, and what
//! has no plan, as a boot or power, acts as it did. A dialog the machine
//! opens ends the dry run: it replaces those the command line opened, and
//! what it offers was not chosen there. Only the workspace opens review
//! pages: the classic map has no dry run (ADR 0016, point 10).

use crate::menu::keymap::Map;
use crate::menu::model::{Dialog, DryRun, Model, Reviewing};
use crate::menu::world::{Action, Event};

/// The command line's `Enter`: what it chooses next shows its plan first,
/// under the workspace.
pub(super) fn start(model: &mut Model) {
    if model.map == Map::Workspace {
        model.dry_run = DryRun::Pending;
    }
}

/// The machine's `event`: a dialog of its own, a question, an unlock
/// prompt, the choices of a pool that did not import or the offer to seal
/// a key again, ends the dry run, before the dialog opens. Such a dialog
/// can close without a key, as its time runs out or the machine withdraws
/// it.
pub(super) fn machine(model: &mut Model, event: &Event) {
    if matches!(
        event,
        Event::UnlockRequested { .. }
            | Event::Asked { .. }
            | Event::ImportProblem { .. }
            | Event::ResealOffered(_)
    ) {
        model.dry_run = DryRun::Off;
    }
}

/// What the menu sends of the `actions` an input asked for, once the dry
/// run took its part: while it holds, one change that has a plan, and that
/// the machine offers, is not sent; its dry run opens, and asks for its
/// plan instead. The dry run ends there, once a review page opened, and
/// once no dialog is left.
pub(super) fn intercept(model: &mut Model, mut actions: Vec<Action>) -> Vec<Action> {
    if model.dry_run == DryRun::Off {
        return actions;
    }
    if let [action] = actions.as_slice()
        && action.has_plan()
        && model.snapshot.is_offered(action)
        && let Some(action) = actions.pop()
    {
        model.dry_run = DryRun::Off;
        let reviewing = Reviewing::dry_run(action.clone());
        model.dialog = Some(Dialog::Review(Box::new(reviewing)));
        return vec![Action::Plan(Box::new(action))];
    }
    if matches!(model.dialog, None | Some(Dialog::Review(_))) {
        model.dry_run = DryRun::Off;
    }
    actions
}
