//! Dialogs: unlocking, choosing, confirming, errors.

use zbm_platform::secret::Secret;
use zbm_types::{ImportPolicy, PoolName};

use super::screens;
use crate::menu::keymap::{Input, Intent};
use crate::menu::model::{Choice, Dialog, Model, Then};
use crate::menu::world::{Action, Method, Outcome};

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
            what: "nothing to confirm".to_owned(),
            why: Vec::new(),
            next: Vec::new(),
        },
    }
}

/// The confirmation of a forced import.
pub(super) fn force_import(pool: &PoolName) -> Then {
    Then::Confirm {
        action: Action::Import(pool.clone(), ImportPolicy::Force),
        title: format!("Force the Import of {pool}"),
        lines: vec![
            format!("{pool} was last imported by another host."),
            "If that host is still running and using it, forcing the import can corrupt it."
                .to_owned(),
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
        Then::Open(screen) => model.screen = screen,
        Then::Focus(environment, screen) => screens::focus(model, environment, screen),
    }
    Vec::new()
}

/// The power choices.
pub(super) fn power(model: &mut Model) {
    let act = |label: &str, action: Action| Choice {
        label: label.to_owned(),
        then: Then::Act(action),
    };
    model.dialog = Some(choose(
        "Power",
        Vec::new(),
        vec![
            act("Power Off", Action::PowerOff),
            act("Reboot", Action::Reboot),
            act("Reboot into Firmware Setup", Action::FirmwareSetup),
        ],
    ));
}

/// The details of a pool, with what can be done to it.
pub(super) fn pool_details(model: &mut Model, name: &PoolName) {
    let Some(pool) = model.snapshot.pools.iter().find(|pool| &pool.name == name) else {
        return;
    };
    let mode = if pool.writable {
        "read/write"
    } else {
        "read-only"
    };
    let mut lines = vec![format!("{name}: {} · {mode}", pool.health.as_str())];
    if let Some(hostid) = &pool.foreign_hostid {
        lines.push(format!("last imported by host {hostid}"));
    }
    let mut choices = Vec::new();
    if pool.imported && !pool.writable {
        choices.push(Choice {
            label: "Import Read/Write".to_owned(),
            then: Then::Act(Action::ImportReadWrite(name.clone())),
        });
    }
    if !pool.imported {
        choices.push(Choice {
            label: "Import with Its Last Host ID".to_owned(),
            then: Then::Act(Action::Import(name.clone(), ImportPolicy::Hostid)),
        });
        choices.push(Choice {
            label: "Force the Import".to_owned(),
            then: force_import(name),
        });
    }
    if pool.checkpoint {
        choices.push(Choice {
            label: "Rewind to the Checkpoint".to_owned(),
            then: Then::Confirm {
                action: Action::RewindCheckpoint(name.clone()),
                title: format!("Rewind {name} to Its Checkpoint"),
                lines: vec![format!(
                    "Every change to {name} since its checkpoint is lost."
                )],
                name: name.to_string(),
            },
        });
    }
    model.dialog = Some(choose(&format!("Pool {name}"), lines, choices));
}

/// An input while a dialog is open.
pub(super) fn handle(model: &mut Model, input: Input) -> Vec<Action> {
    let Some(dialog) = model.dialog.take() else {
        return Vec::new();
    };
    match dialog {
        Dialog::Unlock {
            volume,
            method,
            tries_left,
            secret,
        } => unlock(model, input, (volume, method, tries_left, secret)),
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

/// The unlock dialog: the secret never leaves its [`Secret`].
fn unlock(
    model: &mut Model,
    input: Input,
    (volume, method, tries_left, mut secret): (String, Method, Option<u32>, Secret),
) -> Vec<Action> {
    let mut method = method;
    match input {
        Input::Type(c) => {
            let mut bytes = [0_u8; 4];
            secret.push_bytes(c.encode_utf8(&mut bytes).as_bytes());
        }
        Input::Erase => erase_char(&mut secret),
        Input::Clear => secret = Secret::new(),
        Input::Intent(Intent::Back) => return vec![Action::SkipUnlock(volume)],
        Input::Intent(Intent::Up | Intent::Down) => {
            method = other_method(model, &volume, method, input);
            secret = Secret::new();
        }
        Input::Intent(Intent::Primary) => {
            let secret = method.is_typed().then_some(secret);
            return vec![Action::Unlock {
                volume,
                method,
                secret,
            }];
        }
        Input::Intent(_) => {}
    }
    model.dialog = Some(Dialog::Unlock {
        volume,
        method,
        tries_left,
        secret,
    });
    Vec::new()
}

/// Removes the last character of a secret typed as UTF-8.
fn erase_char(secret: &mut Secret) {
    while let Some(byte) = secret.pop() {
        if byte & 0xC0 != 0x80 {
            break;
        }
    }
}

/// The method before or after `method` among those of `volume`.
fn other_method(model: &Model, volume: &str, method: Method, input: Input) -> Method {
    let methods = model
        .snapshot
        .volumes
        .iter()
        .find(|known| known.name == volume)
        .map(|known| known.methods.clone())
        .unwrap_or_default();
    let Some(position) = methods.iter().position(|known| *known == method) else {
        return method;
    };
    let next = if input == Input::Intent(Intent::Up) {
        position.checked_sub(1).unwrap_or(methods.len() - 1)
    } else {
        (position + 1) % methods.len()
    };
    methods.get(next).copied().unwrap_or(method)
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
            model.notification = Some(Outcome::Warning(format!("type {name} to confirm")));
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
