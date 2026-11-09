//! The palette's entries: environments, their kernels and snapshots, the
//! pools, destroying snapshots, the tools and what only a key reaches.

use super::super::{offers, screens, snapshots, unlock};
use crate::menu::keymap::{self, Map};
use crate::menu::model::{Model, PaletteItem, Screen, Then};
use crate::menu::world::Action;
use crate::text::{self, id, text};

/// An entry of the palette, labelled in the menu's language.
fn item(label: String, then: Then) -> PaletteItem {
    PaletteItem {
        label,
        english: None,
        then,
    }
}

/// The palette's entries, labelled in the menu's language.
pub(super) fn entries(model: &Model) -> Vec<PaletteItem> {
    let mut items = Vec::new();
    for environment in &model.snapshot.environments {
        let name = &environment.name;
        let shown = name.to_string();
        let environment_text = shown.as_str();
        let boot = screens::boot(model, environment, None);
        items.push(item(
            text!("palette-boot", environment = environment_text),
            Then::Act(boot),
        ));
        items.push(item(
            text!("view-kernels.of", environment = environment_text),
            Then::Focus(name.clone(), Screen::Kernels),
        ));
        items.push(item(
            text!("view-snapshots.of", environment = environment_text),
            Then::Focus(name.clone(), Screen::Snapshots),
        ));
        items.push(item(
            text!("palette-edit", environment = environment_text),
            Then::Focus(name.clone(), Screen::CommandLine),
        ));
        if environment
            .tries
            .is_some_and(|tries| tries.left < tries.total)
        {
            items.push(item(
                text!("palette-reset-tries", environment = environment_text),
                Then::Act(Action::ResetBootTries(name.clone())),
            ));
        }
        // Newest first, whatever the order of their column.
        let mut loaded: Vec<_> = model
            .loaded_snapshots(name)
            .unwrap_or_default()
            .iter()
            .collect();
        loaded.sort_by_key(|snapshot| std::cmp::Reverse(snapshot.created));
        for snapshot in loaded {
            items.push(item(
                text!("palette-boot-clone", snapshot = snapshot.name.to_string()),
                Then::BootClone(snapshot.name.clone()),
            ));
        }
    }
    for pool in model.snapshot.pools.iter().filter(|pool| pool.imported) {
        items.push(item(
            text!("palette-export", pool = pool.name.to_string()),
            Then::Act(Action::Export(pool.name.clone())),
        ));
    }
    items.extend(destroy_items(model));
    items.extend(tool_items(model));
    items.extend(key_items(model));
    items.retain(|item| offers::is_offered(model, &item.then));
    items
}

/// The palette's entries that destroy snapshots, the workspace's alone and
/// over the snapshots view, whose selection and marks they name: the
/// selected snapshot, and the marked ones. Not one for each snapshot read,
/// which would double the palette of a long list (B5).
fn destroy_items(model: &Model) -> Vec<PaletteItem> {
    if model.map != Map::Workspace || model.screen != Screen::Snapshots {
        return Vec::new();
    }
    let mut items = Vec::new();
    if let Some(snapshot) = model.selected_snapshot() {
        items.push(item(
            text!("palette-destroy", snapshot = snapshot.name.to_string()),
            snapshots::destroy(vec![&snapshot.name]),
        ));
    }
    if !model.marked.is_empty() {
        items.push(item(
            text!("palette-destroy-marked", count = model.marked.len()),
            snapshots::destroy_marked(model),
        ));
    }
    items
}

/// The palette's entries that are not about an environment: sealing TPM2
/// keys again, the views, scanning the pools again, the recovery shell and
/// power.
fn tool_items(model: &Model) -> Vec<PaletteItem> {
    let mut items: Vec<PaletteItem> = model
        .snapshot
        .volumes
        .iter()
        .filter_map(|volume| {
            let then = unlock::reseal(model, volume.name.get())?;
            Some(item(
                text!("reseal.title", volume = volume.name.printable().to_string()),
                then,
            ))
        })
        .collect();
    let screens = [
        (id!("tool-pools"), Screen::Pools),
        (id!("palette-recovery-tools"), Screen::Recovery),
        (id!("tool-log"), Screen::Log),
    ];
    items.extend(
        screens
            .into_iter()
            .map(|(label, screen)| item(text::get(label), Then::Open(screen))),
    );
    let actions = [
        (id!("palette-rescan-pools"), Action::ScanPools),
        (id!("tool-shell.title"), Action::Shell),
        (id!("power-off"), Action::PowerOff),
        (id!("power-reboot"), Action::Reboot),
        (id!("power-firmware-setup"), Action::FirmwareSetup),
    ];
    items.extend(
        actions
            .into_iter()
            .map(|(label, action)| item(text::get(label), Then::Act(action))),
    );
    items
}

/// The palette's entries of what only a character or a chord reaches in
/// the view under it (rule 1 of decision 18), each with its keys: one does
/// what its key does there.
fn key_items(model: &Model) -> Vec<PaletteItem> {
    let context = model.view_context();
    keymap::commands(model.map, context)
        .into_iter()
        .map(|(intent, keys)| {
            let label = text!(
                "palette-key",
                action = text::title(keymap::describe(context, intent)),
                dot = model.theme.dot(),
                keys = keys
            );
            item(label, Then::Intent(intent))
        })
        .collect()
}
