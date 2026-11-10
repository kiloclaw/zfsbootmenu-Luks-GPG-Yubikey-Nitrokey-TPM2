//! What a key does to the selection of a view, as one [`Then`]: the action
//! it sends, or the confirmation, review page or name that leads to it, or
//! the view it opens. The views' keys follow it, and the command line's
//! keys and costs and the lock's marks read it, so that a key and what it
//! is said to cost cannot disagree. A key that opens a sheet, or does
//! something else, does none: its sheet's entries have their own.

use zbm_types::{ImportPolicy, PoolName, SnapshotName, Untrusted};

use super::{dialogs, diff, screens, snapshots};
use crate::menu::keymap::{Context, Intent, Map};
use crate::menu::model::{Item, Kind, Model, Node, RECOVERY, RecoveryTool, Screen, Then};
use crate::menu::world::{Access, Action};

/// What `intent` does in `context` to `model`'s selection; `None` when it
/// opens a sheet, moves, or does nothing there.
pub(crate) fn action_of(model: &Model, context: Context, intent: Intent) -> Option<Then> {
    match (context, intent) {
        (_, Intent::Log) => Some(Then::Open(Screen::Log)),
        (_, Intent::Recovery) => Some(Then::Open(Screen::Recovery)),
        (_, Intent::Help) => Some(Then::Open(Screen::Help(context))),
        (Context::Main, _) => on_environment(model, intent),
        (Context::Kernels, _) => on_kernel(model, intent),
        (Context::Snapshots, _) => on_snapshot(model, intent),
        (Context::Pools, _) => on_pool(model, intent),
        (Context::Recovery, Intent::Primary) => recovery_tool(model),
        (Context::Diff, Intent::Primary) => diff::restore_of(model),
        _ => None,
    }
}

/// What `intent` does to the selected environment of the main view.
fn on_environment(model: &Model, intent: Intent) -> Option<Then> {
    let environment = model.selected_environment()?;
    let name = environment.name.clone();
    Some(match intent {
        Intent::Primary | Intent::Boot => Then::Act(screens::boot(model, environment, None)),
        Intent::Kernels => Then::Focus(name, Screen::Kernels),
        Intent::Snapshots => Then::Focus(name, Screen::Snapshots),
        Intent::Edit => Then::Focus(name, Screen::CommandLine),
        Intent::SetDefault => Then::Act(Action::SetDefaultEnvironment(name)),
        Intent::Pools => Then::Open(Screen::Pools),
        Intent::ReadWrite => Then::Act(read_write(model, &name.pool())),
        Intent::Chroot => Then::Act(Action::Chroot(name.into())),
        Intent::NewSnapshot => snapshots::new_snapshot(model, name),
        Intent::Shell => Then::Act(Action::Shell),
        Intent::RevertCommandLine => Then::Revert(name),
        _ => return None,
    })
}

/// The read/write import of `pool`, the imported pool of that name, by its
/// identifier when a pool not imported has the name too.
fn read_write(model: &Model, pool: &PoolName) -> Action {
    let imported = model
        .snapshot
        .pools
        .iter()
        .find(|known| known.imported && &known.name == pool);
    let named = imported.map_or_else(|| pool.into(), |known| dialogs::Named::of(model, known));
    dialogs::import(named, Access::ReadWrite, ImportPolicy::Strict)
}

/// What `intent` does to the selected kernel of the focused environment;
/// its command line is edited for a boot with that kernel.
fn on_kernel(model: &Model, intent: Intent) -> Option<Then> {
    let environment = model.focused()?;
    let index = model.kernels.selected_item()?;
    let kernel = environment.kernels.get(index)?;
    let name = environment.name.clone();
    Some(match intent {
        Intent::Primary | Intent::Boot => Then::Act(screens::boot(
            model,
            environment,
            Some(kernel.version.clone()),
        )),
        Intent::SetDefault if kernel.is_default => Then::Act(Action::InheritDefaultKernel(name)),
        Intent::SetDefault => Then::Act(Action::SetDefaultKernel(name, kernel.version.clone())),
        Intent::InheritDefault => Then::Act(Action::InheritDefaultKernel(name)),
        Intent::Edit => Then::EditFor(name, kernel.version.clone()),
        _ => return None,
    })
}

/// What `intent` does to the selected snapshot, or the marked ones: `Enter`
/// opens their sheet, and the marks, the order and the folds change only
/// the view.
fn on_snapshot(model: &Model, intent: Intent) -> Option<Then> {
    match intent {
        Intent::NewSnapshot => {
            return Some(snapshots::new_snapshot(model, model.focus.clone()?));
        }
        Intent::Diff => return snapshots::diff_of(model).ok().flatten().map(Then::Act),
        Intent::Destroy if !model.marked.is_empty() => {
            return Some(snapshots::destroy_marked(model));
        }
        _ => {}
    }
    let snapshot = model.selected_snapshot()?;
    let name: &SnapshotName = &snapshot.name;
    Some(match intent {
        Intent::Boot => Then::BootClone(name.clone()),
        Intent::Rollback => snapshots::rollback(model, snapshot),
        Intent::Destroy => snapshots::destroy(vec![name]),
        Intent::Clone => snapshots::clone(model, name),
        Intent::Duplicate => snapshots::duplicate(model, snapshot),
        Intent::CloneAndPromote => snapshots::promote(model, name),
        Intent::Chroot => Then::Act(Action::Chroot(name.clone().into())),
        _ => return None,
    })
}

/// What `intent` does to the selected row of the pools view: a volume
/// opens, but not while the machine works, as between two of its
/// questions; `Enter` on a device of the workspace's tree asks for the
/// kernel's messages about it; a pool is imported read/write or rewound,
/// unless a pool of its name is imported, or it has no checkpoint. Its
/// details and its imports are sheets.
fn on_pool(model: &Model, intent: Intent) -> Option<Then> {
    if let Some(volume) = model.selected_volume() {
        let opens = matches!(intent, Intent::Primary | Intent::Unlock);
        return (opens && model.working.is_none()).then(|| Then::Act(screens::open(volume)));
    }
    if intent == Intent::Primary
        && let Some(Node {
            kind: Kind::Device,
            item: Item::Device(_, device),
            ..
        }) = model
            .selected_node()
            .filter(|_| model.map == Map::Workspace)
    {
        let path = Untrusted::new(device.path().to_owned());
        return Some(Then::Act(Action::LoadDeviceLog(path)));
    }
    let pool = model.selected_pool()?;
    let named = dialogs::Named::of(model, pool);
    match intent {
        Intent::ReadWrite if !model.is_shadowed(pool) => Some(Then::Act(dialogs::import(
            named,
            Access::ReadWrite,
            ImportPolicy::Strict,
        ))),
        // Unknown, as for a pool that is not imported, the machine checks.
        Intent::Rewind if pool.checkpoint != Some(false) => Some(dialogs::rewind(
            model.map,
            named,
            screens::rewind_lines(pool),
        )),
        _ => None,
    }
}

/// What the selected recovery tool does.
fn recovery_tool(model: &Model) -> Option<Then> {
    let (_, tool) = RECOVERY.get(model.recovery.selected_item()?)?;
    Some(match tool {
        RecoveryTool::Shell => Then::Act(Action::Shell),
        RecoveryTool::Chroot => {
            let environment = model.selected_environment()?;
            Then::Act(Action::Chroot(environment.name.clone().into()))
        }
        RecoveryTool::Log => Then::Open(Screen::Log),
        RecoveryTool::Pools => Then::Open(Screen::Pools),
        RecoveryTool::FirmwareSetup => Then::Act(Action::FirmwareSetup),
        RecoveryTool::Reboot => Then::Act(Action::Reboot),
        RecoveryTool::PowerOff => Then::Act(Action::PowerOff),
    })
}
