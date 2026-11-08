//! What a message does to the menu: `update(model, message) -> (model,
//! actions)`, pure and deterministic. Key presses go through the key map of
//! the focused element; machine events change the snapshot or open
//! dialogs, and may ask for the snapshots of the column open again; ticks
//! run the countdown.

use zbm_display::keys::{Key, KeyPress};

use super::keymap::{self, Input, Intent, Tab};
use super::model::{Dialog, Model, Screen, TypedOn};
use super::world::{Action, Event, Outcome};

mod dialogs;
mod diff;
mod events;
mod history;
mod lists;
mod offers;
mod review;
mod screens;
mod snapshots;
mod text;
mod unlock;

pub use dialogs::follow;
pub use text::palette_items;

/// What happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// A key press.
    Key(KeyPress),
    /// The keys that follow come from there, or from an input that does
    /// not say.
    TypedOn(Option<TypedOn>),
    /// Time passed, in milliseconds.
    Tick(u64),
    /// The machine said something; boxed, as a snapshot is large.
    Event(Box<Event>),
    /// The machine started performing an action on its own, or ended.
    Busy(bool),
}

/// The menu after `message`, and the actions it asks the machine for:
/// only those the machine offers.
pub fn update(mut model: Model, message: Message) -> (Model, Vec<Action>) {
    let actions = match message {
        Message::Key(key) => {
            let mut actions = key_press(&mut model, key);
            events::start(&mut model);
            if model.screen == Screen::Log {
                actions.extend(history::load(&mut model));
            }
            actions
        }
        Message::TypedOn(typed_on) => {
            model.typed_on = typed_on;
            Vec::new()
        }
        Message::Tick(ms) => tick(&mut model, ms),
        Message::Event(event) => events::apply(&mut model, *event),
        Message::Busy(busy) => {
            model.working = busy.then(|| model.working.unwrap_or(0));
            if !busy {
                model.not_loaded();
                model.reading_history = false;
            }
            Vec::new()
        }
    };
    // A device's messages belong to the Pools tab: leaving it, but for its
    // help, closes them, and those read once it is left are not shown.
    if !matches!(model.screen, Screen::Pools | Screen::Help(_)) {
        model.device_log = None;
    }
    let actions = offers::only_offered(&mut model, actions);
    (model, actions)
}

/// A key press: it stops the countdown, then does what the key map says.
fn key_press(model: &mut Model, key: KeyPress) -> Vec<Action> {
    model.keys = model.keys.saturating_add(1);
    if model.countdown.take().is_some() && key.key == Key::Escape {
        return Vec::new();
    }
    if !matches!(model.notification, Some(Outcome::Failed { .. })) {
        model.notification = None;
    }
    let key = unlock::translated(model, key);
    let typing = model.typing(&key);
    match keymap::input_on(model.map, model.context(), key, typing) {
        Some(input) => handle(model, input),
        None => Vec::new(),
    }
}

/// An input, routed to the focused element.
fn handle(model: &mut Model, input: Input) -> Vec<Action> {
    if input == Input::Intent(Intent::Layout) {
        unlock::next_layout(model);
        return Vec::new();
    }
    if model.dialog.is_some() {
        return dialogs::handle(model, input);
    }
    if model.palette.is_some() {
        return text::palette(model, input);
    }
    if model.screen == Screen::CommandLine {
        return text::editor(model, input);
    }
    if model.list().is_some_and(|list| list.filtering) {
        return text::filter(model, input);
    }
    let Input::Intent(intent) = input else {
        return Vec::new();
    };
    if !keymap::is_offered(model.context(), intent, model) {
        return offers::not_offered(model);
    }
    if let Some(actions) = global(model, intent) {
        return actions;
    }
    screens::intent(model, intent)
}

/// The intents of every view; `None` when the view decides.
fn global(model: &mut Model, intent: Intent) -> Option<Vec<Action>> {
    match intent {
        Intent::Palette => text::open_palette(model),
        Intent::Help => {
            let context = model.context();
            model.screen = Screen::Help(context);
            model.scroll = 0;
        }
        Intent::Log => {
            model.screen = Screen::Log;
            model.scroll = 0;
        }
        Intent::Recovery => model.screen = Screen::Recovery,
        Intent::Power => dialogs::power(model),
        Intent::Redraw => {}
        Intent::Back => back(model),
        Intent::Tab(tab) => return Some(screens::open_tab(model, tab)),
        Intent::NextTab => return Some(screens::cycle_tab(model, Tab::next)),
        Intent::PreviousTab => return Some(screens::cycle_tab(model, Tab::previous)),
        Intent::Filter => {
            if let Some(list) = model.list_mut() {
                list.filtering = true;
            }
        }
        Intent::Up
        | Intent::Down
        | Intent::PageUp
        | Intent::PageDown
        | Intent::First
        | Intent::Last => {
            lists::move_selection(model, intent);
        }
        _ => return None,
    }
    Some(Vec::new())
}

/// Back one step: clear the main view's filter, close a device's kernel
/// messages, or return to the main view, under the workspace during
/// start-up to its view (help returns to the view it was opened from).
fn back(model: &mut Model) {
    match model.screen {
        Screen::Pools if model.device_log.is_some() => model.device_log = None,
        Screen::Main => {
            model.environments.filter.clear();
            model.refresh();
        }
        Screen::Startup => {}
        Screen::Help(context) => model.screen = screens::screen_of(context),
        Screen::Diff => {
            model.screen = Screen::Snapshots;
            model.diff = None;
        }
        _ => {
            model.screen = model.boot_view();
            model.marked.clear();
        }
    }
}

/// Time passed: the tab bar's clock advances, and with it the timeline's
/// days; the countdown runs, as start-up's dialog's does; and the first
/// time after the main view opened with no countdown running, the menu
/// asks the machine to scan the pools, which the views list once it
/// answers, and the workspace asks for the boot history, which its
/// inspector shows. While the countdown runs, nothing is read: the boot it
/// ends with would wait for the scan, and Escape would cancel the scan
/// instead of stopping the countdown.
fn tick(model: &mut Model, ms: u64) -> Vec<Action> {
    model.ticked = model.ticked.saturating_add(ms);
    model.follow_clock();
    let mut actions = count_down(model, ms);
    let counting = model.countdown.is_some();
    if actions.is_empty() && model.started && !counting && !model.scanned {
        model.scanned = true;
        actions.push(Action::ScanPools);
        actions.extend(history::load(model));
    }
    actions
}

/// The countdown runs, and when it ends boots the selection: the default
/// environment, the one asked for once, or the fallback of a default that
/// used up its boot tries. It boots only environments, and never asks:
/// nobody may be there.
fn count_down(model: &mut Model, ms: u64) -> Vec<Action> {
    if let Some(ran) = &mut model.working {
        *ran = ran.saturating_add(ms);
    }
    if model.dialog.is_some() {
        return dialogs::count_down(model, ms);
    }
    let Some(left) = model.countdown else {
        return Vec::new();
    };
    let left = left.saturating_sub(ms);
    if left > 0 {
        model.countdown = Some(left);
        return Vec::new();
    }
    model.countdown = None;
    let Some(environment) = model.selected_environment().cloned() else {
        return Vec::new();
    };
    let mut boot = screens::boot(model, &environment, None);
    if let Action::Boot { countdown, .. } = &mut boot {
        *countdown = true;
    }
    vec![boot]
}

/// Opens the error dialog for a failed outcome.
pub(crate) fn show_failure(model: &mut Model, outcome: &Outcome) {
    if let Outcome::Failed { what, why, next } = outcome {
        model.dialog = Some(Dialog::Error {
            what: what.clone(),
            why: why.clone(),
            next: next.clone(),
        });
    }
}

#[cfg(test)]
mod columns_tests;
#[cfg(test)]
mod commands_tests;
#[cfg(test)]
mod destroy_tests;
#[cfg(test)]
mod health_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod import_tests;
#[cfg(test)]
mod marks_tests;
#[cfg(test)]
mod offers_tests;
#[cfg(test)]
mod question_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod startup_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timeline_tests;
#[cfg(test)]
mod tree_tests;
#[cfg(test)]
mod unlock_tests;
