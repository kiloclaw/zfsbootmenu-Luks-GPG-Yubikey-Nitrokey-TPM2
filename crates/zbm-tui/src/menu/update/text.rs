//! Text fields: the filter of a list, the command palette and the command
//! line editor.

use super::dialogs;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Model, Palette, PaletteItem, Screen, Then};
use crate::menu::world::{Action, Outcome};

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
            super::screens::intent(model, Intent::Primary)
        }
        Input::Intent(intent @ (Intent::Up | Intent::Down | Intent::PageUp | Intent::PageDown)) => {
            super::lists::move_selection(model, intent);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

/// Opens the command palette.
pub(super) fn open_palette(model: &mut Model) {
    let items = palette_items(model);
    let matches = crate::menu::filter::rank(items.iter().map(|item| item.label.as_str()), "");
    model.palette = Some(Palette {
        query: String::new(),
        matches,
        selected: 0,
    });
}

/// The command palette.
pub(super) fn palette(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(mut palette) = model.palette.take() else {
        return Vec::new();
    };
    let items = palette_items(model);
    if edit(&mut palette.query, input).is_none() {
        palette.matches =
            crate::menu::filter::rank(items.iter().map(|item| item.label.as_str()), &palette.query);
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
                .and_then(|r| items.get(r.index));
            return chosen
                .map(|item| dialogs::follow(model, item.then.clone()))
                .unwrap_or_default();
        }
        Input::Intent(Intent::Up) => palette.selected = palette.selected.saturating_sub(1),
        Input::Intent(Intent::Down) => palette.selected = (palette.selected + 1).min(last),
        _ => {}
    }
    model.palette = Some(palette);
    Vec::new()
}

/// Everything the command palette finds: environments, their kernels and
/// snapshots, and the actions of every view.
pub fn palette_items(model: &Model) -> Vec<PaletteItem> {
    let item = |label: String, then: Then| PaletteItem { label, then };
    let mut items = Vec::new();
    for environment in &model.snapshot.environments {
        let name = &environment.name;
        let boot = super::screens::boot(model, environment, None);
        items.push(item(format!("Boot {name}"), Then::Act(boot)));
        items.push(item(
            format!("Kernels of {name}"),
            Then::Focus(name.clone(), Screen::Kernels),
        ));
        items.push(item(
            format!("Snapshots of {name}"),
            Then::Focus(name.clone(), Screen::Snapshots),
        ));
        items.push(item(
            format!("Edit the command line of {name}"),
            Then::Focus(name.clone(), Screen::CommandLine),
        ));
        if environment
            .tries
            .is_some_and(|tries| tries.left < tries.total)
        {
            items.push(item(
                format!("Give {name} Its Boot Tries Back"),
                Then::Act(Action::ResetBootTries(name.clone())),
            ));
        }
        for snapshot in &environment.snapshots {
            items.push(item(
                format!("Boot a clone of {}", snapshot.name),
                Then::Act(Action::BootSnapshot(snapshot.name.clone())),
            ));
        }
    }
    items.extend(tool_items(model));
    items
}

/// The palette's entries that are not about an environment: sealing TPM2
/// keys again, the views, the recovery shell and power.
fn tool_items(model: &Model) -> Vec<PaletteItem> {
    let item = |label: String, then: Then| PaletteItem { label, then };
    let mut items: Vec<PaletteItem> = model
        .snapshot
        .volumes
        .iter()
        .filter_map(|volume| {
            let then = super::unlock::reseal(model, &volume.name)?;
            Some(item(
                format!("Re-seal the TPM2 Key of {}", volume.name),
                then,
            ))
        })
        .collect();
    let screens = [
        ("Pools and volumes", Screen::Pools),
        ("Recovery tools", Screen::Recovery),
        ("Log", Screen::Log),
    ];
    items.extend(
        screens
            .into_iter()
            .map(|(label, screen)| item(label.to_owned(), Then::Open(screen))),
    );
    let actions = [
        ("Recovery Shell", Action::Shell),
        ("Power Off", Action::PowerOff),
        ("Reboot", Action::Reboot),
        ("Reboot into Firmware Setup", Action::FirmwareSetup),
    ];
    items.extend(
        actions
            .into_iter()
            .map(|(label, action)| item(label.to_owned(), Then::Act(action))),
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
            model
                .edited
                .push((editor.environment.clone(), editor.text.clone()));
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
    (quotes % 2 == 1).then(|| "the command line has an unclosed \" quote".to_owned())
}
