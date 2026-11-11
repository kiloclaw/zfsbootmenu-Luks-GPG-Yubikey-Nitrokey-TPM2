//! The palette's actions: what changes or runs something, for each
//! environment, pool and volume, for the selection of the view under it,
//! the tools, and what only a key reaches. Each entry is found by its
//! command's name and target too (rule 4 of decision 18); a name is
//! looked up once for each kind of entry, not for each entry (B5).

use zbm_types::{DatasetName, ImportPolicy};

use super::super::{dialogs, offers, screens, snapshots, unlock};
use super::items;
use crate::menu::keymap::{self, Map};
use crate::menu::model::{Costs, Model, PaletteItem, Screen, Section, Then};
use crate::menu::world::{Access, Action, Environment, Origin, VolumeState};
use crate::text::{self, Id, id, text};

/// An entry of `section`, labelled in the menu's language, which a query
/// finds by `name`, then `target` when it has one.
pub(super) fn item(
    section: Section,
    label: String,
    name: &str,
    target: &str,
    then: Then,
) -> PaletteItem {
    let also = if target.is_empty() {
        name.to_owned()
    } else {
        format!("{name} {target}")
    };
    PaletteItem {
        label,
        also,
        section,
        then,
        keys: String::new(),
        costs: Costs::default(),
    }
}

/// An entry of the actions.
fn action(label: String, name: &str, target: &str, then: Then) -> PaletteItem {
    item(Section::Actions, label, name, target, then)
}

/// The command name `id`, in the menu's language.
fn command(id: Id) -> String {
    text::get(id)
}

/// The palette's entries, labelled in the menu's language: its actions,
/// then where to go, then help; those the machine offers.
pub(super) fn entries(model: &Model) -> Vec<PaletteItem> {
    let mut items = environment_actions(model);
    items.extend(pool_actions(model));
    items.extend(volume_actions(model));
    items.extend(snapshot_actions(model));
    items.extend(kernel_actions(model));
    items.extend(tool_actions());
    items.extend(key_actions(model));
    items.extend(items::places(model));
    items.extend(items::help(model));
    items.retain(|item| offers::is_offered(model, &item.then));
    items
}

/// What each environment does: its boot, the edit of its command line and
/// its revert, its save and inherit in the workspace ([`saves`]), its
/// default, a snapshot of it, a chroot into it, its boot tries given back,
/// and the boot of a clone of each snapshot read ([`clone_boots`]).
fn environment_actions(model: &Model) -> Vec<PaletteItem> {
    let boot = command(id!("command-boot"));
    let edit = command(id!("command-edit"));
    let revert = command(id!("command-revert"));
    let saved = [
        id!("command-commandline"),
        id!("command-inherit-commandline"),
    ]
    .map(command);
    let default = command(id!("command-default"));
    let snapshot = command(id!("command-snapshot"));
    let chroot = command(id!("command-chroot"));
    let tries = command(id!("command-tries"));
    let clone = (!is_said_by_clone_label(&boot)).then_some(boot.as_str());
    let mut items = Vec::new();
    for environment in &model.snapshot.environments {
        let name = &environment.name;
        let target = name.to_string();
        let at = target.as_str();
        let then = Then::Act(screens::boot(model, environment, None));
        let label = text!("palette-boot", environment = at);
        items.push(action(label, &boot, at, then));
        let then = Then::Focus(name.clone(), Screen::CommandLine);
        let label = text!("palette-edit", environment = at);
        items.push(action(label, &edit, at, then));
        if model.edited.iter().any(|(edited, _)| edited == name) {
            let label = text!("palette-revert", environment = at);
            items.push(action(label, &revert, at, Then::Revert(name.clone())));
        }
        items.extend(saves(model, environment, &saved, at));
        let then = Then::Act(Action::SetDefaultEnvironment(name.clone()));
        let label = text!("palette-default", environment = at);
        items.push(action(label, &default, at, then));
        let then = snapshots::new_snapshot(model, name.clone());
        let label = text!("palette-snapshot", environment = at);
        items.push(action(label, &snapshot, at, then));
        let then = Then::Act(Action::Chroot(name.clone().into()));
        items.push(action(
            text!("palette-chroot", target = at),
            &chroot,
            at,
            then,
        ));
        if environment
            .tries
            .is_some_and(|tries| tries.left < tries.total)
        {
            let then = Then::Act(Action::ResetBootTries(name.clone()));
            let label = text!("palette-reset-tries", environment = at);
            items.push(action(label, &tries, at, then));
        }
        items.extend(clone_boots(model, name, clone));
    }
    items
}

/// The command line of `environment`, `at`, saved for every boot, and
/// inherited when it is set on the environment itself: found by the names
/// `save` and `inherit`. The workspace's alone (ADR 0016, point 10).
fn saves(
    model: &Model,
    environment: &Environment,
    [save, inherit]: &[String; 2],
    at: &str,
) -> Vec<PaletteItem> {
    if model.map != Map::Workspace {
        return Vec::new();
    }
    let name = &environment.name;
    let label = text!("palette-set-command-line", environment = at);
    let mut items = vec![action(label, save, at, Then::SaveCommandLine(name.clone()))];
    if environment.command_line.origin == Origin::Local {
        let then = Then::Act(Action::InheritCommandLine(name.clone()));
        let label = text!("palette-inherit-command-line", environment = at);
        items.push(action(label, inherit, at, then));
    }
    items
}

/// The boot of a clone of each snapshot of `environment` read, newest
/// first, whatever the order of their column, found by `boot` and the
/// snapshot; by its label alone without `boot`, where the label says both
/// ([`is_said_by_clone_label`]), which builds no other name for each of
/// thousands of snapshots (B5): `Tab` makes it then ([`clone_command`]).
fn clone_boots(model: &Model, environment: &DatasetName, boot: Option<&str>) -> Vec<PaletteItem> {
    let mut loaded: Vec<_> = model
        .loaded_snapshots(environment)
        .unwrap_or_default()
        .iter()
        .collect();
    loaded.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created));
    loaded
        .into_iter()
        .map(|snapshot| {
            let target = snapshot.name.to_string();
            let label = text!("palette-boot-clone", snapshot = target.as_str());
            let then = Then::BootClone(snapshot.name.clone());
            let Some(boot) = boot else {
                return item(Section::Actions, label, "", "", then);
            };
            action(label, boot, &target, then)
        })
        .collect()
}

/// Whether the label of a clone's boot says `boot`, the command's name,
/// before the snapshot, whatever the case: whatever a query for the name
/// and the snapshot matches, the label matches too.
fn is_said_by_clone_label(boot: &str) -> bool {
    const MARK: &str = "\u{e000}";
    let label = text!("palette-boot-clone", snapshot = MARK).to_lowercase();
    let before = label.find(MARK).and_then(|at| label.get(..at));
    !boot.is_empty() && before.is_some_and(|before| before.contains(&boot.to_lowercase()))
}

/// The command's name and target of the boot of a clone of `snapshot`, in
/// the menu's language, as `Tab` completes it.
pub(super) fn clone_command(snapshot: &zbm_types::SnapshotName) -> String {
    format!("{} {snapshot}", command(id!("command-boot")))
}

/// What each pool does, as its imports offer it (`i`): an imported one is
/// exported, imported read/write or read-only again, or rewound when it
/// has a checkpoint; another is imported with this host's ID, read-only or
/// read/write (`w`), with its last host's, or forced, unless a pool of its
/// name is imported. The rewind of the selected pool is a key's entry too,
/// which says when it has no checkpoint.
fn pool_actions(model: &Model) -> Vec<PaletteItem> {
    let export = command(id!("command-export"));
    let read_write = command(id!("command-read-write"));
    let read_only = command(id!("command-read-only"));
    let import = command(id!("command-import"));
    let last_host = command(id!("command-import-last-host"));
    let force = command(id!("command-import-force"));
    let rewind = command(id!("command-rewind"));
    let mut items = Vec::new();
    for pool in &model.snapshot.pools {
        let named = dialogs::Named::of(model, pool);
        let target = pool.name.to_string();
        let at = target.as_str();
        if pool.imported {
            let then = Then::Act(Action::Export(pool.name.clone()));
            items.push(action(
                text!("palette-export", pool = at),
                &export,
                at,
                then,
            ));
            let then = dialogs::toggle(named, pool.writable).then;
            let (label, name) = if pool.writable {
                (text!("palette-read-only", pool = at), &read_only)
            } else {
                (text!("palette-read-write", pool = at), &read_write)
            };
            items.push(action(label, name, at, then));
        } else if !model.is_shadowed(pool) {
            let this = dialogs::import(named, Access::ReadOnly, ImportPolicy::Strict);
            let label = text!("palette-import", pool = at);
            items.push(action(label, &import, at, Then::Act(this)));
            let then = Then::Act(dialogs::import(
                named,
                Access::ReadWrite,
                ImportPolicy::Strict,
            ));
            let label = text!("palette-read-write", pool = at);
            items.push(action(label, &read_write, at, then));
            let last = dialogs::import(named, Access::ReadOnly, ImportPolicy::Hostid);
            let label = text!("palette-import-last-host", pool = at);
            items.push(action(label, &last_host, at, Then::Act(last)));
            let then = dialogs::force_import(model.map, named);
            let label = text!("palette-import-force", pool = at);
            items.push(action(label, &force, at, then));
        }
        if pool.imported && pool.checkpoint == Some(true) {
            let then = dialogs::rewind(model.map, named, screens::rewind_lines(pool));
            items.push(action(
                text!("palette-rewind", pool = at),
                &rewind,
                at,
                then,
            ));
        }
    }
    items
}

/// What each volume does: one not open is unlocked, but not while the
/// machine works, as between two questions of the action that opens it,
/// as its key does not; and one whose TPM2 key no longer unseals is
/// sealed again.
fn volume_actions(model: &Model) -> Vec<PaletteItem> {
    let unlock_name = command(id!("command-unlock"));
    let reseal = command(id!("command-reseal"));
    let mut items = Vec::new();
    for volume in &model.snapshot.volumes {
        let target = volume.name.printable().to_string();
        let at = target.as_str();
        if volume.state != VolumeState::Open && model.working.is_none() {
            let then = Then::Act(screens::open(volume));
            let label = text!("palette-unlock", volume = at);
            items.push(action(label, &unlock_name, at, then));
        }
        if let Some(then) = unlock::reseal(model, volume.name.get()) {
            items.push(action(
                text!("reseal.title", volume = at),
                &reseal,
                at,
                then,
            ));
        }
    }
    items
}

/// What the selected snapshot does, and the marked ones, over the
/// snapshots view, whose selection and marks they name, as its `Enter`
/// menu offers it: one entry for each action, not for each snapshot read,
/// which would double the palette of a long list (B5). The workspace's
/// destroy them too; the boot of a clone has an entry of each snapshot.
fn snapshot_actions(model: &Model) -> Vec<PaletteItem> {
    if model.screen != Screen::Snapshots {
        return Vec::new();
    }
    let mut items = Vec::new();
    let workspace = model.map == Map::Workspace;
    if let Some(snapshot) = model.selected_snapshot() {
        let target = snapshot.name.to_string();
        let at = target.as_str();
        let snapshot_name = &snapshot.name;
        let actions = [
            (
                text!("palette-roll-back", snapshot = at),
                id!("command-rollback"),
                snapshots::rollback(model, snapshot),
            ),
            (
                text!("palette-clone", snapshot = at),
                id!("command-clone"),
                snapshots::clone(model, snapshot_name),
            ),
            (
                text!("palette-duplicate", snapshot = at),
                id!("command-duplicate"),
                snapshots::duplicate(model, snapshot),
            ),
            (
                text!("palette-promote", snapshot = at),
                id!("command-promote"),
                snapshots::promote(model, snapshot_name),
            ),
            (
                text!("palette-diff", snapshot = at),
                id!("command-diff"),
                Then::Act(Action::Diff(snapshot_name.clone(), None)),
            ),
            (
                text!("palette-chroot", target = at),
                id!("command-chroot"),
                Then::Act(Action::Chroot(snapshot_name.clone().into())),
            ),
        ];
        for (label, id, then) in actions {
            items.push(action(label, &command(id), at, then));
        }
        if workspace {
            let then = snapshots::destroy(vec![snapshot_name]);
            let label = text!("palette-destroy", snapshot = at);
            items.push(action(label, &command(id!("command-destroy")), at, then));
        }
    }
    if workspace && !model.marked.is_empty() {
        let label = text!("palette-destroy-marked", count = model.marked.len());
        let then = snapshots::destroy_marked(model);
        items.push(action(label, &command(id!("command-destroy")), "", then));
    }
    items
}

/// What the selected kernel does, over the kernels view: it becomes the
/// default of its environment, unless it is; the environment inherits its
/// default kernel.
fn kernel_actions(model: &Model) -> Vec<PaletteItem> {
    if model.screen != Screen::Kernels {
        return Vec::new();
    }
    let Some(environment) = model.focused() else {
        return Vec::new();
    };
    let target = environment.name.to_string();
    let at = target.as_str();
    let mut items = Vec::new();
    let selected = model.kernels.selected_item();
    let kernel = selected.and_then(|index| environment.kernels.get(index));
    if let Some(kernel) = kernel.filter(|kernel| !kernel.is_default) {
        let version = kernel.version.printable().to_string();
        let label = text!(
            "palette-default-kernel",
            kernel = version.as_str(),
            environment = at
        );
        let then = Then::Act(Action::SetDefaultKernel(
            environment.name.clone(),
            kernel.version.clone(),
        ));
        let target = format!("{at} {version}");
        items.push(action(
            label,
            &command(id!("command-default")),
            &target,
            then,
        ));
    }
    let then = Then::Act(Action::InheritDefaultKernel(environment.name.clone()));
    let label = text!("palette-inherit", environment = at);
    items.push(action(label, &command(id!("command-inherit")), at, then));
    items
}

/// The tools: scanning the pools again, the recovery shell and power,
/// whose targets say which.
fn tool_actions() -> Vec<PaletteItem> {
    let power = id!("command-power");
    let actions = [
        (
            id!("palette-rescan-pools"),
            id!("command-rescan"),
            None,
            Action::ScanPools,
        ),
        (
            id!("tool-shell.title"),
            id!("command-shell"),
            None,
            Action::Shell,
        ),
        (
            id!("power-off"),
            power,
            Some(id!("command-power.off")),
            Action::PowerOff,
        ),
        (
            id!("power-reboot"),
            power,
            Some(id!("command-power.reboot")),
            Action::Reboot,
        ),
        (
            id!("power-firmware-setup"),
            power,
            Some(id!("command-power.firmware")),
            Action::FirmwareSetup,
        ),
    ];
    actions
        .into_iter()
        .map(|(label, id, target, act)| {
            let target = target.map(text::get).unwrap_or_default();
            action(text::get(label), &command(id), &target, Then::Act(act))
        })
        .collect()
}

/// What only a character or a chord reaches in the view under the
/// palette (rule 1 of decision 18), found by its command's name, with its
/// keys on the right ([`super::costs::annotate`]): one does what its key
/// does there.
fn key_actions(model: &Model) -> Vec<PaletteItem> {
    let context = model.view_context();
    keymap::commands(model.map, context)
        .into_iter()
        .map(|(intent, _)| {
            let label = text::title(keymap::describe(context, intent));
            let name = keymap::command_name(intent).map(text::get);
            action(label, &name.unwrap_or_default(), "", Then::Intent(intent))
        })
        .collect()
}
