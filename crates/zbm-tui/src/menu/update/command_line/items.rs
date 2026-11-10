//! The palette's items and help: where to go, as each environment's
//! kernels and snapshots, the environment, pool or volume itself on its
//! tab, and the views; then the help of the view under it, of each tab
//! and of the command line.

use super::entries::item;
use crate::menu::keymap::{self, Context};
use crate::menu::model::{Model, PaletteItem, Place, Screen, Section, Then};
use crate::text::{self, id, text};

/// An entry of the items.
fn place(label: String, name: &str, target: &str, then: Then) -> PaletteItem {
    item(Section::Items, label, name, target, then)
}

/// Where the palette goes: the kernels and snapshots of each environment,
/// each environment, pool and volume on its tab, and the views.
pub(super) fn places(model: &Model) -> Vec<PaletteItem> {
    let kernels = text::get(id!("command-kernels"));
    let snapshots = text::get(id!("command-snapshots"));
    let go = text::get(id!("command-go"));
    let mut items = Vec::new();
    for environment in &model.snapshot.environments {
        let name = &environment.name;
        let target = name.to_string();
        let at = target.as_str();
        let then = Then::Focus(name.clone(), Screen::Kernels);
        items.push(place(
            text!("view-kernels.of", environment = at),
            &kernels,
            at,
            then,
        ));
        let then = Then::Focus(name.clone(), Screen::Snapshots);
        let label = text!("view-snapshots.of", environment = at);
        items.push(place(label, &snapshots, at, then));
        let then = Then::Go(Place::Environment(name.clone()));
        let label = text!("palette-go-environment", environment = at);
        items.push(place(label, &go, at, then));
    }
    for pool in &model.snapshot.pools {
        let target = crate::menu::model::pool_name(pool, model.import_id(pool));
        let then = Then::Go(Place::Pool(pool.name.clone(), pool.id));
        let label = text!("palette-go-pool", pool = target.as_str());
        items.push(place(label, &go, &target, then));
    }
    for volume in &model.snapshot.volumes {
        let target = volume.name.printable().to_string();
        let then = Then::Go(Place::Volume(volume.name.clone()));
        let label = text!("palette-go-volume", volume = target.as_str());
        items.push(place(label, &go, &target, then));
    }
    let views = [
        (id!("tool-pools"), id!("command-pools"), Screen::Pools),
        (
            id!("palette-recovery-tools"),
            id!("command-recovery"),
            Screen::Recovery,
        ),
        (id!("tool-log"), id!("command-log"), Screen::Log),
    ];
    items.extend(views.into_iter().map(|(label, command, screen)| {
        place(
            text::get(label),
            &text::get(command),
            "",
            Then::Open(screen),
        )
    }));
    items
}

/// The help of the view under the palette, of the view of each tab, and
/// of the command line: the Snapshots tab's while an environment is
/// focused, which it shows.
pub(super) fn help(model: &Model) -> Vec<PaletteItem> {
    let help = text::get(id!("command-help"));
    let mut contexts = vec![model.view_context(), Context::Main];
    if model.focused().is_some() {
        contexts.push(Context::Snapshots);
    }
    contexts.extend([Context::Pools, Context::Log, Context::Command]);
    let mut items: Vec<PaletteItem> = Vec::new();
    for context in contexts {
        let then = Then::Open(Screen::Help(context));
        if items.iter().any(|item| item.then == then) {
            continue;
        }
        let view = text::get(keymap::view_name(context));
        let label = text!("palette-help", view = view.as_str());
        items.push(item(Section::Help, label, &help, &view, then));
    }
    items
}
