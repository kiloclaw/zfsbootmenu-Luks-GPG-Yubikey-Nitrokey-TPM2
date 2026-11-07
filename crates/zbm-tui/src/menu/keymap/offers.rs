//! What a key starts, as the machine offers it (ADR 0016, point 2): the
//! toolbar and the help show only the keys of what is offered, and a key
//! of something else does nothing but say so.

use super::{Context, Intent, Map};
use crate::menu::model::{Dialog, Kind, Model, RECOVERY, Segment};
use crate::menu::world::Offer;
use crate::text::{Id, id};
use Context::{Diff, Kernels, Main, Pools, Recovery, Snapshots};

/// The kinds of the entries of the menu `intent` opens in `context`;
/// `None` when it opens none.
fn menu_of(context: Context, intent: Intent) -> Option<&'static [Offer]> {
    match (context, intent) {
        (Snapshots, Intent::Primary) => Some(&[
            Offer::BootSnapshot,
            Offer::Rollback,
            Offer::Clone,
            Offer::Duplicate,
            Offer::Diff,
            Offer::Chroot,
        ]),
        (_, Intent::Power) => Some(&[Offer::Power, Offer::FirmwareSetup]),
        _ => None,
    }
}

/// The kind of action `intent` starts in `context` on `model`'s selection;
/// `None` when it starts none: it moves, opens a view or a menu.
fn offer_of(context: Context, intent: Intent, model: &Model) -> Option<Offer> {
    match (context, intent) {
        // The command line is edited for a boot.
        (Main | Kernels, Intent::Primary | Intent::Boot | Intent::Edit) => Some(Offer::Boot),
        (Snapshots, Intent::Boot) => Some(Offer::BootSnapshot),
        (Main, Intent::SetDefault) => Some(Offer::SetDefaultEnvironment),
        (Kernels, Intent::SetDefault | Intent::InheritDefault) => Some(Offer::DefaultKernel),
        (Main | Snapshots, Intent::NewSnapshot) => Some(Offer::CreateSnapshot),
        (Snapshots, Intent::Rollback) => Some(Offer::Rollback),
        (Snapshots, Intent::Clone | Intent::CloneAndPromote) => Some(Offer::Clone),
        (Snapshots, Intent::Duplicate) => Some(Offer::Duplicate),
        // Snapshots are marked for a diff, and paths for a restore.
        (Snapshots, Intent::Diff | Intent::Mark) => Some(Offer::Diff),
        (Diff, Intent::Primary | Intent::Mark) => model
            .diff
            .as_ref()
            .filter(|diff| diff.segment == Segment::Files)
            .map(|_| Offer::RestoreFiles),
        (Main | Snapshots, Intent::Chroot) => Some(Offer::Chroot),
        (Main | Pools, Intent::ReadWrite) | (Pools, Intent::Import | Intent::Rewind) => {
            Some(Offer::Import)
        }
        (Pools, Intent::Unlock) => Some(Offer::OpenVolume),
        (Pools, Intent::Primary) => model.selected_volume().map(|_| Offer::OpenVolume),
        (Recovery, Intent::Primary) => model
            .recovery
            .selected_item()
            .and_then(|index| RECOVERY.get(index))
            .and_then(|(_, tool)| tool.offer()),
        (Main, Intent::Shell) => Some(Offer::Shell),
        _ => None,
    }
}

/// Whether `intent` in `context` starts nothing, what the machine of
/// `model` offers, or a menu with at least one entry it offers. Files are
/// restored only over the live filesystem a diff compared the snapshot
/// with, never from a diff of two snapshots, which did not look at it.
pub(crate) fn is_offered(context: Context, intent: Intent, model: &Model) -> bool {
    let offers = &model.snapshot.offers;
    if let Some(kinds) = menu_of(context, intent) {
        return kinds.iter().any(|kind| offers.contains(kind));
    }
    match offer_of(context, intent, model) {
        None => true,
        Some(Offer::RestoreFiles) if model.diff.as_ref().is_some_and(|diff| diff.to.is_some()) => {
            false
        }
        Some(kind) => offers.contains(&kind),
    }
}

/// Whether `intent` in `context` acts on the selected snapshot, which the
/// header of a folded group of the timeline is not: `Enter` there opens
/// the group, and a diff of marked snapshots needs none selected.
pub(crate) fn needs_snapshot(context: Context, intent: Intent, model: &Model) -> bool {
    context == Snapshots
        && match intent {
            Intent::Primary
            | Intent::Mark
            | Intent::Boot
            | Intent::Rollback
            | Intent::Clone
            | Intent::CloneAndPromote
            | Intent::Duplicate
            | Intent::Chroot => true,
            Intent::Diff => model.marked.is_empty(),
            _ => false,
        }
}

/// Whether `intent` in the workspace's Pools tab is a key of another kind
/// of row than the one selected: a volume's on a pool, a vdev or a device,
/// whose keys act on their pool, or a pool's on a volume.
pub(crate) fn is_misplaced(context: Context, intent: Intent, model: &Model) -> bool {
    if context != Pools || model.map != Map::Workspace {
        return false;
    }
    let volume = model.selected_volume().is_some();
    match intent {
        Intent::Unlock => !volume,
        Intent::Import | Intent::ReadWrite | Intent::Rewind => volume,
        _ => false,
    }
}

/// Whether the toolbar, the next steps and the line view show `intent` in
/// `context`: it is offered, and does something to the selection, which
/// on the header of a folded group a snapshot's action does not, nor a
/// volume's key on a pool's row of the workspace's tree.
pub(crate) fn is_shown(context: Context, intent: Intent, model: &Model) -> bool {
    is_offered(context, intent, model)
        && !(needs_snapshot(context, intent, model) && model.folded_group().is_some())
        && !is_misplaced(context, intent, model)
}

/// What `intent` does in `context`, as the toolbar, the help and the line
/// view say it of `model`'s selection: `Enter` on a device of the
/// workspace's tree shows the kernel's messages about it, and on a review
/// page that refuses its change closes it; elsewhere as
/// [`describe`](super::describe) says.
pub(crate) fn label(context: Context, intent: Intent, model: &Model) -> Id {
    let is_device = || (model.selected_node()).is_some_and(|node| node.kind == Kind::Device);
    if context == Pools && intent == Intent::Primary && model.map == Map::Workspace && is_device() {
        return id!("action-messages");
    }
    // A review page that refuses the change has no field: `Enter` closes
    // it, and the arrows scroll what it would lose.
    if context == Context::Choose && matches!(model.dialog, Some(Dialog::Review(_))) {
        match intent {
            Intent::Primary => return id!("action-close"),
            Intent::Up | Intent::Down => return id!("action-scroll"),
            _ => {}
        }
    }
    super::describe(context, intent)
}
