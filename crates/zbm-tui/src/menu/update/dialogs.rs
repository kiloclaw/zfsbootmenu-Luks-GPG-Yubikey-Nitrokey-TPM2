//! Dialogs: choosing, confirming, errors; unlocking is in `unlock`.

use zbm_types::{Answer, ImportPolicy, PoolName};

use super::screens;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Asking, Choice, Dialog, Model, Then};
use crate::menu::world::{Action, Outcome};
use crate::text::text;

/// A choice dialog.
pub(super) fn choose(title: &str, lines: Vec<String>, choices: Vec<Choice>) -> Dialog {
    Dialog::Choose {
        title: title.to_owned(),
        lines,
        choices,
        selected: 0,
    }
}

/// The confirmation dialog of a [`Then::Confirm`]; another `then` is a
/// programming error that yields an empty confirmation.
pub(super) fn confirm(then: Then) -> Dialog {
    match then {
        Then::Confirm {
            action,
            title,
            lines,
            name,
        } => Dialog::Confirm {
            title,
            lines,
            name,
            typed: String::new(),
            action,
        },
        _ => Dialog::Error {
            what: text!("error-nothing-to-confirm"),
            why: Vec::new(),
            next: Vec::new(),
        },
    }
}

/// The confirmation of a forced import.
pub(super) fn force_import(pool: &PoolName) -> Then {
    Then::Confirm {
        action: Action::Import(pool.clone(), ImportPolicy::Force),
        title: text!("force-import.title", pool = pool.to_string()),
        lines: vec![
            text!("force-import", pool = pool.to_string()),
            text!("force-import.risk"),
        ],
        name: pool.to_string(),
    }
}

/// Does what `then` says: asks for an action, opens a confirmation or a
/// screen.
pub fn follow(model: &mut Model, then: Then) -> Vec<Action> {
    match then {
        Then::Act(action) => return vec![action],
        Then::Confirm { .. } => model.dialog = Some(confirm(then)),
        Then::Open(screen) => {
            model.screen = screen;
            model.scroll = 0;
        }
        Then::Focus(environment, screen) => screens::focus(model, environment, screen),
    }
    Vec::new()
}

/// The power choices.
pub(super) fn power(model: &mut Model) {
    let act = |label: String, action: Action| Choice {
        label,
        then: Then::Act(action),
    };
    model.dialog = Some(choose(
        &text!("power-title"),
        Vec::new(),
        vec![
            act(text!("power-off"), Action::PowerOff),
            act(text!("power-reboot"), Action::Reboot),
            act(text!("power-firmware-setup"), Action::FirmwareSetup),
        ],
    ));
}

/// The details of a pool, with what can be done to it.
pub(super) fn pool_details(model: &mut Model, name: &PoolName) {
    let Some(pool) = model.snapshot.pools.iter().find(|pool| &pool.name == name) else {
        return;
    };
    let mode = if pool.writable {
        text!("pool-read-write")
    } else {
        text!("pool-read-only")
    };
    let mut lines = vec![format!("{name}: {} · {mode}", pool.health.as_str())];
    if let Some(hostid) = &pool.foreign_hostid {
        lines.push(text!("pool-last-host", hostid = hostid.as_str()));
    }
    let mut choices = Vec::new();
    if pool.imported && !pool.writable {
        choices.push(Choice {
            label: text!("pool-import-read-write"),
            then: Then::Act(Action::ImportReadWrite(name.clone())),
        });
    }
    if !pool.imported {
        choices.push(Choice {
            label: text!("pool-import-last-host"),
            then: Then::Act(Action::Import(name.clone(), ImportPolicy::Hostid)),
        });
        choices.push(Choice {
            label: text!("pool-force-import"),
            then: force_import(name),
        });
    }
    if pool.checkpoint {
        choices.push(Choice {
            label: text!("pool-rewind"),
            then: Then::Confirm {
                action: Action::RewindCheckpoint(name.clone()),
                title: text!("rewind.title", pool = name.to_string()),
                lines: vec![text!("rewind", pool = name.to_string())],
                name: name.to_string(),
            },
        });
    }
    model.dialog = Some(choose(
        &text!("pool-title", pool = name.to_string()),
        lines,
        choices,
    ));
}

/// An input while a dialog is open.
pub(super) fn handle(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(dialog) = model.dialog.take() else {
        return Vec::new();
    };
    match dialog {
        Dialog::Unlock(prompt) => super::unlock::prompt(model, input, prompt),
        Dialog::Question(asking) => answer(model, input, asking),
        Dialog::Choose {
            title,
            lines,
            choices,
            selected,
        } => chosen(model, input, (title, lines, choices, selected)),
        Dialog::Confirm {
            title,
            lines,
            name,
            typed,
            action,
        } => confirmed(model, input, (title, lines, name, typed, action)),
        Dialog::Error { .. } => {
            if !matches!(input, Input::Intent(Intent::Primary | Intent::Back)) {
                model.dialog = Some(dialog);
            }
            Vec::new()
        }
    }
}

/// A question of start-up: `Enter` goes on, `Esc` stops.
fn answer(model: &mut Model, input: Input, asking: Asking) -> Vec<Action> {
    match input {
        Input::Intent(Intent::Primary) => vec![Action::Answer(Answer::Continue)],
        Input::Intent(Intent::Back) => vec![Action::Answer(Answer::Escape)],
        _ => {
            model.dialog = Some(Dialog::Question(asking));
            Vec::new()
        }
    }
}

/// Counts the time of start-up's dialog down by `ms`: when it runs out,
/// a question goes on and a volume stops waiting for its device.
pub(super) fn count_down(model: &mut Model, ms: u64) -> Vec<Action> {
    let left = match &mut model.dialog {
        Some(Dialog::Question(asking)) => &mut asking.left,
        Some(Dialog::Unlock(prompt)) => &mut prompt.wait,
        _ => return Vec::new(),
    };
    let Some(time) = left.as_mut() else {
        return Vec::new();
    };
    *time = time.saturating_sub(ms);
    if *time > 0 {
        return Vec::new();
    }
    match model.dialog.take() {
        Some(Dialog::Unlock(prompt)) => vec![Action::SkipUnlock(prompt.volume)],
        _ => vec![Action::Answer(Answer::Continue)],
    }
}

/// The choice dialog.
fn chosen(
    model: &mut Model,
    input: Input,
    (title, lines, choices, selected): (String, Vec<String>, Vec<Choice>, usize),
) -> Vec<Action> {
    // The row after the choices is Cancel.
    let last = choices.len();
    let selected = match input {
        Input::Intent(Intent::Back) => return Vec::new(),
        Input::Intent(Intent::Primary) => {
            return match choices.into_iter().nth(selected) {
                Some(choice) => follow(model, choice.then),
                None => Vec::new(),
            };
        }
        Input::Intent(Intent::Up) => selected.saturating_sub(1),
        Input::Intent(Intent::Down) => (selected + 1).min(last),
        _ => selected,
    };
    model.dialog = Some(Dialog::Choose {
        title,
        lines,
        choices,
        selected,
    });
    Vec::new()
}

/// The typed confirmation: the action runs only when the name was typed.
fn confirmed(
    model: &mut Model,
    input: Input,
    (title, lines, name, mut typed, action): (String, Vec<String>, String, String, Action),
) -> Vec<Action> {
    match input {
        Input::Type(c) => typed.push(c),
        Input::Erase => {
            typed.pop();
        }
        Input::Clear => typed.clear(),
        Input::Intent(Intent::Back) => return Vec::new(),
        Input::Intent(Intent::Primary) if typed == name => return vec![action],
        Input::Intent(Intent::Primary) => {
            model.notification = Some(Outcome::Warning(text!(
                "confirm-type-first",
                name = name.as_str()
            )));
        }
        Input::Intent(_) => {}
    }
    model.dialog = Some(Dialog::Confirm {
        title,
        lines,
        name,
        typed,
        action,
    });
    Vec::new()
}
