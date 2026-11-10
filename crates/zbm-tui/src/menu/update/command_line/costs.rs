//! The keys and the costs of the palette's entries (ADR 0007, point 5;
//! ADR 0010): the keys that do an entry in the view under the palette, and
//! what it asks for before it acts, a PIN among them. Both come from what
//! the keys do ([`action_of`]) and the entry's action, never from its plan,
//! which the machine would have to make for each entry (B5).

use zbm_types::{DatasetName, SnapshotName};

use super::super::action_of::action_of;
use super::super::dialogs;
use crate::menu::keymap::{self, Context, Intent};
use crate::menu::model::{Costs, Model, PaletteItem, Screen, Then};
use crate::menu::world::{Action, Encryption};

/// Gives each of `items` its keys in the view under the palette and its
/// costs. The boots of the clones of an environment's snapshots, of which
/// there are thousands, cost the same: their costs are made once for each
/// environment, and only the keys that boot a clone are looked through.
pub(super) fn annotate(model: &Model, items: &mut [PaletteItem]) {
    let context = model.view_context();
    let listed = keymap::commands(model.map, context);
    let keyed = keyed(model, context, &listed);
    let (clone_keys, other_keys): (Vec<_>, Vec<_>) =
        (keyed.iter()).partition(|(then, _)| matches!(then, Then::BootClone(_)));
    let mut clones: Option<(DatasetName, Costs)> = None;
    for item in items {
        let keys = match &item.then {
            Then::Intent(intent) => (listed.iter())
                .find(|(listed, _)| listed == intent)
                .map(|(_, keys)| keys),
            Then::BootClone(_) => keys_of(&clone_keys, &item.then),
            then => keys_of(&other_keys, then),
        };
        if let Some(keys) = keys {
            item.keys.clone_from(keys);
        }
        item.costs = match &item.then {
            Then::BootClone(snapshot) => match &clones {
                Some((environment, costs)) if environment == snapshot.dataset() => *costs,
                _ => {
                    let costs = costs_of(model, &item.then);
                    clones = Some((snapshot.dataset().clone(), costs));
                    costs
                }
            },
            then => costs_of(model, then),
        };
    }
}

/// The keys in `keyed` that do `then`.
fn keys_of<'k>(keyed: &[&'k (Then, String)], then: &Then) -> Option<&'k String> {
    keyed
        .iter()
        .find(|(keyed, _)| keyed == then)
        .map(|(_, keys)| keys)
}

/// What the keys of `context` that the toolbar or only help names do,
/// each with those keys: not those of what only a character reaches,
/// `listed`, whose entries do what the key does and have its keys
/// already.
fn keyed<T>(model: &Model, context: Context, listed: &[(Intent, T)]) -> Vec<(Then, String)> {
    let does = |intent| {
        let is_listed = listed.iter().any(|(listed, _)| *listed == intent);
        if is_listed || !keymap::is_shown(context, intent, model) {
            return None;
        }
        action_of(model, context, intent)
    };
    keymap::keys_by(model.map, context, &does)
}

/// What `then` costs: its action's costs, and a review page's, a
/// promotion's typed name among them; the boot of a command line edited,
/// which the lock keeps; and a key's, what the key does in the view under
/// the palette. A sheet, a view or a place costs nothing: the sheet's
/// entries have their own.
pub(crate) fn costs_of(model: &Model, then: &Then) -> Costs {
    match then {
        Then::Act(action) => of_action(model, action),
        Then::Confirm { action, .. } | Then::Review(action) => Costs {
            review: true,
            ..of_action(model, action)
        },
        Then::Name(naming) => naming.action().map_or_else(Costs::default, |action| Costs {
            review: dialogs::confirmed_name(naming).is_some(),
            ..of_action(model, &action)
        }),
        Then::BootClone(snapshot) => of_action(model, &boot_clone(snapshot)),
        Then::Focus(_, Screen::CommandLine) | Then::EditFor(..) => Costs {
            pin: model.snapshot.menu_locked,
            ..Costs::default()
        },
        Then::Intent(intent) => action_of(model, model.view_context(), *intent)
            .map_or_else(Costs::default, |then| costs_of(model, &then)),
        Then::Open(_) | Then::Focus(..) | Then::Go(_) | Then::Revert(_) => Costs::default(),
    }
}

/// The boot of a clone of `snapshot`, as its costs see it: they do not
/// depend on the clone's name, which is asked for once it is chosen.
fn boot_clone(snapshot: &SnapshotName) -> Action {
    Action::BootSnapshot(snapshot.clone(), snapshot.dataset().clone())
}

/// What `action` costs on the machine of `model`: the PIN when the locked
/// menu keeps it, or it is the shell, which asks for the PIN itself; the
/// read/write import of the pool it writes, when that is imported
/// read-only; the key of the environment it changes, boots or opens a
/// shell in, when that is locked.
fn of_action(model: &Model, action: &Action) -> Costs {
    let snapshot = &model.snapshot;
    let pin = snapshot.menu_locked
        && (matches!(action, Action::Shell) || action.is_kept_by_lock(snapshot.bootfs.as_ref()));
    let changed = action.changed_environment();
    let writes = action.needs_writable_pool()
        && changed.is_some_and(|environment| {
            let pool = environment.pool();
            (snapshot.pools.iter())
                .any(|known| known.imported && known.name == pool && !known.writable)
        });
    let key = keyed_environment(action).is_some_and(|environment| {
        (snapshot.environments.iter())
            .any(|known| known.name == *environment && known.encryption == Encryption::Locked)
    });
    Costs {
        pin,
        writes,
        key,
        review: false,
    }
}

/// The environment whose key `action` loads: the one it boots or opens a
/// shell in, or the one it changes.
fn keyed_environment(action: &Action) -> Option<&DatasetName> {
    match action {
        Action::Boot { environment, .. } => Some(environment),
        Action::Chroot(target) => Some(target.dataset()),
        other => other.changed_environment(),
    }
}

/// Whether the locked menu keeps what `intent` does in `context` to
/// `model`'s selection, which the toolbar, the next steps and the line view
/// mark: never a key that opens a sheet, or the countdown's boot.
pub(crate) fn is_kept(model: &Model, context: Context, intent: Intent) -> bool {
    model.snapshot.menu_locked
        && action_of(model, context, intent).is_some_and(|then| costs_of(model, &then).pin)
}
