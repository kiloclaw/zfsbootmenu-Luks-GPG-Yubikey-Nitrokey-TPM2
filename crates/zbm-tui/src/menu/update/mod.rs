//! What a message does to the menu: `update(model, message) -> (model,
//! actions)`, pure and deterministic. Key presses go through the key map of
//! the focused element; machine events change the snapshot or open
//! dialogs; ticks run the countdown.

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::keymap::{self, Input, Intent};
use super::model::{Dialog, Model, Screen};
use super::world::{Action, Event, Outcome};

mod dialogs;
mod events;
mod lists;
mod screens;
mod snapshots;
mod text;

pub use dialogs::follow;
pub use text::palette_items;

/// What happened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// A key press.
    Key(KeyEvent),
    /// Time passed, in milliseconds.
    Tick(u64),
    /// The machine said something.
    Event(Event),
}

/// The menu after `message`, and the actions it asks the machine for.
pub fn update(mut model: Model, message: Message) -> (Model, Vec<Action>) {
    let actions = match message {
        Message::Key(key) => key_press(&mut model, key),
        Message::Tick(ms) => tick(&mut model, ms),
        Message::Event(event) => {
            events::apply(&mut model, event);
            Vec::new()
        }
    };
    (model, actions)
}

/// A key press: it stops the countdown, then does what the key map says.
fn key_press(model: &mut Model, key: KeyEvent) -> Vec<Action> {
    model.keys = model.keys.saturating_add(1);
    if model.countdown.take().is_some() && key.code == KeyCode::Esc {
        return Vec::new();
    }
    if !matches!(model.notification, Some(Outcome::Failed { .. })) {
        model.notification = None;
    }
    match keymap::input(model.context(), key) {
        Some(input) => handle(model, input),
        None => Vec::new(),
    }
}

/// An input, routed to the focused element.
fn handle(model: &mut Model, input: Input) -> Vec<Action> {
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

/// Back one step: clear the main view's filter, or return to the main view
/// (help returns to the view it was opened from).
fn back(model: &mut Model) {
    match model.screen {
        Screen::Main => {
            model.environments.filter.clear();
            model.refresh();
        }
        Screen::Startup => {}
        Screen::Help(context) => model.screen = screens::screen_of(context),
        _ => {
            model.screen = Screen::Main;
            model.marked.clear();
        }
    }
}

/// Time passed: the countdown runs, and boots the default environment when
/// it ends.
fn tick(model: &mut Model, ms: u64) -> Vec<Action> {
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
    vec![screens::boot(model, &environment, None)]
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
mod tests;
