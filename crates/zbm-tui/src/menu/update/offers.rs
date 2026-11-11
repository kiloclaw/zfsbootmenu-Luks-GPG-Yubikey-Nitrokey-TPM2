//! The menu sends only what the machine offers (ADR 0016, point 2): the
//! entries of a menu or of the palette that lead to an action of another
//! kind are left out, and an action of another kind that got through
//! anyway is dropped before it leaves.

use crate::menu::keymap;
use crate::menu::model::{Choice, Make, Model, Screen, Then};
use crate::menu::world::{Action, Offer, Outcome};
use crate::text::text;

/// The kind of action `then` leads to; `None` when it leads to none.
fn offer_of(then: &Then) -> Option<Offer> {
    match then {
        Then::Act(action) | Then::Confirm { action, .. } | Then::Review(action) => action.offer(),
        Then::Name(naming) => Some(match naming.make {
            Make::Snapshot(_) => Offer::CreateSnapshot,
            Make::Clone(_) | Make::Promote(_) => Offer::Clone,
            Make::BootClone(_) => Offer::BootSnapshot,
            Make::Duplicate(_) => Offer::Duplicate,
        }),
        Then::BootClone(_) => Some(Offer::BootSnapshot),
        // The command line is edited for a boot.
        Then::Focus(_, Screen::CommandLine) | Then::EditFor(..) => Some(Offer::Boot),
        Then::SaveCommandLine(_) => Some(Offer::CommandLine),
        Then::Open(_) | Then::Focus(..) | Then::Intent(_) | Then::Go(_) | Then::Revert(_) => None,
    }
}

/// Whether what `then` leads to is no action, or one the machine of
/// `model` offers; a key's, as the toolbar shows it in the view under the
/// palette, which leaves out a key that does nothing to the selection.
pub(super) fn is_offered(model: &Model, then: &Then) -> bool {
    if let Then::Intent(intent) = then {
        return keymap::is_shown(model.view_context(), *intent, model);
    }
    offer_of(then).is_none_or(|kind| model.snapshot.offers.contains(&kind))
}

/// The `choices` the machine of `model` offers.
pub(super) fn offered(model: &Model, choices: Vec<Choice>) -> Vec<Choice> {
    choices
        .into_iter()
        .filter(|choice| is_offered(model, &choice.then))
        .collect()
}

/// Says in the status line that the machine does not offer what was asked
/// for; it asks for nothing.
pub(super) fn not_offered(model: &mut Model) -> Vec<Action> {
    model.notification = Some(Outcome::Warning(text!("not-offered")));
    Vec::new()
}

/// The `actions` the machine of `model` offers. Another one is a mistake
/// of the menu: it is dropped, and the status line says so.
pub(super) fn only_offered(model: &mut Model, mut actions: Vec<Action>) -> Vec<Action> {
    let before = actions.len();
    actions.retain(|action| model.snapshot.is_offered(action));
    if actions.len() < before {
        not_offered(model);
    }
    actions
}
