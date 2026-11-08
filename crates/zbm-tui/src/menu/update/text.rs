//! Text fields: the filter of a list, the command palette and the command
//! line editor.

use zbm_types::Untrusted;

use super::dialogs;
use crate::menu::keymap::{self, Input, Intent, Map};
use crate::menu::model::{Model, Palette, PaletteItem, Screen, Then};
use crate::menu::world::{Action, Outcome};
use crate::text::{self, id, text};

/// Applies a typing input to `text`; returns the intent when the input is
/// not typing, and leaves `text` alone.
fn edit(text: &mut String, input: Input) -> Option<Intent> {
    match input {
        Input::Type(c) => text.push(c),
        Input::Erase => {
            text.pop();
        }
        Input::Clear => text.clear(),
        Input::Intent(intent) => return Some(intent),
    }
    None
}

/// The filter of the current list.
pub(super) fn filter(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(list) = model.list_mut() else {
        return Vec::new();
    };
    if edit(&mut list.filter, input).is_none() {
        model.refresh();
        if let Some(list) = model.list_mut() {
            list.selected = 0;
        }
        return Vec::new();
    }
    match input {
        Input::Intent(Intent::Back) => {
            list.filter.clear();
            list.filtering = false;
            model.refresh();
            Vec::new()
        }
        Input::Intent(Intent::Primary) => {
            list.filtering = false;
            if !keymap::is_offered(model.context(), Intent::Primary, model) {
                return super::offers::not_offered(model);
            }
            super::screens::intent(model, Intent::Primary)
        }
        Input::Intent(intent @ (Intent::Up | Intent::Down | Intent::PageUp | Intent::PageDown)) => {
            super::lists::move_selection(model, intent);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Opens the command palette, with what it finds now.
pub(super) fn open_palette(model: &mut Model) {
    let items = palette_items(model);
    let matches = crate::menu::filter::rank(items.iter().map(|item| item.label.as_str()), "");
    model.palette = Some(Palette {
        query: String::new(),
        items,
        matches,
        selected: 0,
    });
}

/// Makes what the open palette finds again, as the machine sent a
/// snapshot, its pools or snapshots, keeping the query; the selection
/// stays in range.
pub(super) fn palette_again(model: &mut Model) {
    if model.palette.is_none() {
        return;
    }
    let items = palette_items(model);
    if let Some(palette) = &mut model.palette {
        palette.matches =
            crate::menu::filter::rank(items.iter().map(|item| item.label.as_str()), &palette.query);
        palette.items = items;
        palette.selected = palette
            .selected
            .min(palette.matches.len().saturating_sub(1));
    }
}

/// The command palette.
pub(super) fn palette(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(mut palette) = model.palette.take() else {
        return Vec::new();
    };
    if edit(&mut palette.query, input).is_none() {
        let labels = palette.items.iter().map(|item| item.label.as_str());
        palette.matches = crate::menu::filter::rank(labels, &palette.query);
        palette.selected = 0;
        model.palette = Some(palette);
        return Vec::new();
    }
    let last = palette.matches.len().saturating_sub(1);
    match input {
        Input::Intent(Intent::Back) => return Vec::new(),
        Input::Intent(Intent::Primary) => {
            let chosen = palette
                .matches
                .get(palette.selected)
                .and_then(|r| palette.items.get(r.index));
            // A boot from here is checked as from the lists: it may not boot.
            return match chosen.map(|item| item.then.clone()) {
                Some(Then::Act(boot @ Action::Boot { .. })) => super::screens::guarded(model, boot),
                Some(then) => dialogs::follow(model, then),
                None => Vec::new(),
            };
        }
        Input::Intent(Intent::Up) => palette.selected = palette.selected.saturating_sub(1),
        Input::Intent(Intent::Down) => palette.selected = (palette.selected + 1).min(last),
        _ => {}
    }
    model.palette = Some(palette);
    Vec::new()
}

/// Everything the command palette finds: environments, their kernels, the
/// snapshots read so far, and the actions of every view the machine
/// offers.
pub fn palette_items(model: &Model) -> Vec<PaletteItem> {
    let item = |label: String, then: Then| PaletteItem { label, then };
    let mut items = Vec::new();
    for environment in &model.snapshot.environments {
        let name = &environment.name;
        let shown = name.to_string();
        let environment_text = shown.as_str();
        let boot = super::screens::boot(model, environment, None);
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
    items.retain(|item| super::offers::is_offered(model, &item.then));
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
        items.push(PaletteItem {
            label: text!("palette-destroy", snapshot = snapshot.name.to_string()),
            then: super::snapshots::destroy(vec![&snapshot.name]),
        });
    }
    if !model.marked.is_empty() {
        items.push(PaletteItem {
            label: text!("palette-destroy-marked", count = model.marked.len()),
            then: super::snapshots::destroy_marked(model),
        });
    }
    items
}

/// The palette's entries that are not about an environment: sealing TPM2
/// keys again, the views, scanning the pools again, the recovery shell and
/// power.
fn tool_items(model: &Model) -> Vec<PaletteItem> {
    let item = |label: String, then: Then| PaletteItem { label, then };
    let mut items: Vec<PaletteItem> = model
        .snapshot
        .volumes
        .iter()
        .filter_map(|volume| {
            let then = super::unlock::reseal(model, volume.name.get())?;
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

/// The command line editor: `Enter` boots with the edited line, for this
/// boot only.
pub(super) fn editor(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(mut editor) = model.editor.take() else {
        model.screen = Screen::Main;
        return Vec::new();
    };
    if edit(&mut editor.text, input).is_none() {
        model.editor = Some(editor);
        return Vec::new();
    }
    match input {
        Input::Intent(Intent::Back) => {
            model.screen = Screen::Main;
            Vec::new()
        }
        Input::Intent(Intent::Primary) => {
            if let Some(problem) = check(&editor.text) {
                model.notification = Some(Outcome::Warning(problem));
                model.editor = Some(editor);
                return Vec::new();
            }
            // The editor gives its line to the boot and closes: when the boot
            // fails, or the environment has gone, the main view says so.
            model.screen = Screen::Main;
            let Some(environment) = model
                .snapshot
                .environments
                .iter()
                .find(|e| e.name == editor.environment)
                .cloned()
            else {
                return Vec::new();
            };
            model.edited.retain(|(name, _)| *name != editor.environment);
            model.edited.push((
                editor.environment.clone(),
                Untrusted::new(editor.text.clone()),
            ));
            let action = super::screens::boot(model, &environment, editor.kernel.clone());
            super::screens::guarded(model, action)
        }
        _ => {
            model.editor = Some(editor);
            Vec::new()
        }
    }
}

/// What is wrong with a command line, if anything: an unclosed quote.
pub fn check(line: &str) -> Option<String> {
    let quotes = line.chars().filter(|c| *c == '"').count();
    (quotes % 2 == 1).then(|| text!("command-line-unclosed-quote"))
}
