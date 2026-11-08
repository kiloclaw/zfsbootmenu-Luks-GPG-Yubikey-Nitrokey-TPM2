//! Dialogs: choosing, confirming, errors; unlocking is in `unlock`, the
//! review page of a change in `review`.

use zbm_types::{Answer, ImportPolicy, PoolName};

use super::screens;
use crate::menu::keymap::{Input, Intent, Map};
use crate::menu::model::{
    Asking, Choice, Dialog, Make, Model, Naming, Then, host_text, is_name_char,
};
use crate::menu::world::{Access, Action, ImportOptions, Outcome, Pool};
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

/// Opens a choice dialog of the `choices` the machine offers; with none,
/// the status line says so.
pub(super) fn choose_offered(
    model: &mut Model,
    title: &str,
    lines: Vec<String>,
    choices: Vec<Choice>,
) {
    let choices = super::offers::offered(model, choices);
    if choices.is_empty() {
        super::offers::not_offered(model);
        return;
    }
    model.dialog = Some(choose(title, lines, choices));
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

/// A pool as an import names it: by its name, and by its identifier when
/// another pool has that name.
#[derive(Clone, Copy)]
pub(super) struct Named<'p> {
    /// Its name.
    pub name: &'p PoolName,
    /// Its identifier, when the name does not tell it from another.
    pub id: Option<u64>,
}

impl<'p> From<&'p PoolName> for Named<'p> {
    /// A pool its name tells apart.
    fn from(name: &'p PoolName) -> Named<'p> {
        Named { name, id: None }
    }
}

impl<'p> Named<'p> {
    /// `pool`, as the menu of `model` names it to import it.
    pub(super) fn of(model: &Model, pool: &'p Pool) -> Named<'p> {
        Named {
            name: &pool.name,
            id: model.import_id(pool),
        }
    }

    /// The import of the pool with `options`, which name it by its
    /// identifier when it has one.
    fn import(self, options: ImportOptions) -> Action {
        Action::Import(
            self.name.clone(),
            ImportOptions {
                id: self.id,
                ..options
            },
        )
    }
}

/// The import of `pool` with `access` and `policy`, without a rewind. An
/// imported pool is imported again under `zbm.import_policy`, whatever
/// `policy` says.
pub(super) fn import<'p>(
    pool: impl Into<Named<'p>>,
    access: Access,
    policy: ImportPolicy,
) -> Action {
    pool.into().import(ImportOptions {
        access,
        policy,
        ..ImportOptions::default()
    })
}

/// The rewind of `pool` to its checkpoint, which imports it read/write,
/// as the legacy menu's rewind did: its review page under the workspace's
/// map `map`, the classic map's confirmation, with `lines`, else.
pub(super) fn rewind<'p>(map: Map, pool: impl Into<Named<'p>>, lines: Vec<String>) -> Then {
    let pool = pool.into();
    let options = ImportOptions {
        access: Access::ReadWrite,
        policy: ImportPolicy::Strict,
        rewind: true,
        id: None,
    };
    let action = pool.import(options);
    if map == Map::Workspace {
        return Then::Review(action);
    }
    Then::Confirm {
        action,
        title: text!("rewind.title", pool = pool.name.to_string()),
        lines,
        name: pool.name.to_string(),
    }
}

/// A forced import: its review page under the workspace's map `map`, the
/// classic map's confirmation else.
pub(super) fn force_import<'p>(map: Map, pool: impl Into<Named<'p>>) -> Then {
    let pool = pool.into();
    let name = pool.name.to_string();
    let action = import(pool, Access::ReadOnly, ImportPolicy::Force);
    if map == Map::Workspace {
        return Then::Review(action);
    }
    Then::Confirm {
        action,
        title: text!("force-import.title", pool = name.as_str()),
        lines: vec![
            text!("force-import", pool = name.as_str()),
            text!("force-import.risk"),
        ],
        name,
    }
}

/// Does what `then` says: asks for an action, opens a confirmation, a
/// review page, which asks for the plan of its change, or a screen; or
/// does what a key of the view does, if the machine offers it.
pub fn follow(model: &mut Model, then: Then) -> Vec<Action> {
    match then {
        Then::Act(action) => return vec![action],
        Then::Confirm { .. } => model.dialog = Some(confirm(then)),
        Then::Review(action) => return super::review::open(model, action),
        Then::Name(naming) => model.dialog = Some(Dialog::Name(naming)),
        Then::BootClone(snapshot) => {
            return follow(model, super::snapshots::boot_clone(model, &snapshot));
        }
        Then::Open(screen) => {
            model.screen = screen;
            model.scroll = 0;
        }
        Then::Focus(environment, screen) => return screens::focus(model, environment, screen),
        Then::Intent(intent) => return super::handle(model, Input::Intent(intent)),
    }
    Vec::new()
}

/// The power choices.
pub(super) fn power(model: &mut Model) {
    let act = |label: String, action: Action| Choice {
        label,
        then: Then::Act(action),
    };
    let choices = vec![
        act(text!("power-off"), Action::PowerOff),
        act(text!("power-reboot"), Action::Reboot),
        act(text!("power-firmware-setup"), Action::FirmwareSetup),
    ];
    choose_offered(model, &text!("power-title"), Vec::new(), choices);
}

/// The details of `pool`, with what the machine offers to do to it; one
/// named like an imported pool that is another says why it is not
/// imported.
pub(super) fn pool_details(model: &mut Model, pool: &Pool) {
    let name = &pool.name;
    let mode = crate::menu::model::pool_mode(pool);
    let shown = crate::menu::model::pool_name(pool, model.import_id(pool));
    let dot = model.theme.dot();
    let mut lines = vec![format!("{shown}: {}{dot}{mode}", pool.health.as_str())];
    if let Some(host) = &pool.host {
        lines.push(match &host.hostid {
            Some(hostid) => text!("pool-last-host", hostid = hostid.to_string()),
            None => host_text(host),
        });
    }
    let named = Named::of(model, pool);
    let mut choices = Vec::new();
    if pool.imported {
        choices.push(toggle(named, pool.writable));
        choices.push(Choice {
            label: text!("pool-export"),
            then: Then::Act(Action::Export(name.clone())),
        });
    } else if model.is_shadowed(pool) {
        lines.push(text!("pool-shadowed", pool = name.to_string()));
    } else {
        choices.push(Choice {
            label: text!("pool-import-last-host"),
            then: Then::Act(import(named, Access::ReadOnly, ImportPolicy::Hostid)),
        });
        choices.push(Choice {
            label: text!("pool-force-import"),
            then: force_import(model.map, named),
        });
    }
    if pool.checkpoint == Some(true) {
        let lines = vec![text!("rewind", pool = name.to_string())];
        choices.push(Choice {
            label: text!("pool-rewind"),
            then: rewind(model.map, named, lines),
        });
    }
    let choices = super::offers::offered(model, choices);
    model.dialog = Some(choose(
        &text!("pool-title", pool = name.to_string()),
        lines,
        choices,
    ));
}

/// The import of `pool`, imported, with the other access: read-only when
/// it is `writable`, read/write when not, as the legacy menu's `w` toggled
/// it.
pub(super) fn toggle<'p>(pool: impl Into<Named<'p>>, writable: bool) -> Choice {
    let (label, access) = if writable {
        (text!("pool-import-read-only"), Access::ReadOnly)
    } else {
        (text!("pool-import-read-write"), Access::ReadWrite)
    };
    Choice {
        label,
        then: Then::Act(import(pool, access, ImportPolicy::Strict)),
    }
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
        Dialog::Review(reviewing) => super::review::handle(model, input, reviewing),
        Dialog::Name(naming) => named(model, input, naming),
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

/// Whether the menu knows `name` as an environment, or as a snapshot of
/// those read: a snapshot of a column never opened is not known, and zfs
/// refuses its name, saying why.
fn is_taken(model: &Model, name: &str) -> bool {
    let is_snapshot = name.parse::<zbm_types::SnapshotName>().is_ok_and(|name| {
        model
            .snapshot
            .loaded
            .iter()
            .flat_map(|list| list.snapshots.iter())
            .any(|snapshot| snapshot.name == name)
    });
    is_snapshot
        || model
            .snapshot
            .environments
            .iter()
            .any(|environment| environment.name.as_str() == name)
}

/// The name dialog: only the characters a name may have are typed, and
/// the action runs when the name is new.
fn named(model: &mut Model, input: Input, mut naming: Naming) -> Vec<Action> {
    match input {
        Input::Type(c) if is_name_char(c) => naming.typed.push(c),
        Input::Erase => {
            naming.typed.pop();
        }
        Input::Clear => naming.typed.clear(),
        Input::Intent(Intent::Back) => return Vec::new(),
        Input::Intent(Intent::Primary) => {
            let full = naming.full_name();
            match naming.action() {
                Some(_) if is_taken(model, &full) => {
                    model.notification =
                        Some(Outcome::Warning(text!("name-taken", name = full.as_str())));
                }
                Some(action) => return made(model, naming, action),
                None => {
                    model.notification = Some(Outcome::Warning(text!("name-empty")));
                }
            }
        }
        Input::Type(_) | Input::Intent(_) => {}
    }
    model.dialog = Some(Dialog::Name(naming));
    Vec::new()
}

/// What `action`, the making of `naming`, does once named: a promotion,
/// which leaves the environment depending on the clone, asks for the
/// environment's name to be typed; the others go ahead.
fn made(model: &mut Model, naming: Naming, action: Action) -> Vec<Action> {
    let Make::Promote(snapshot) = &naming.make else {
        return vec![action];
    };
    let name = snapshot.dataset().last_component().to_owned();
    model.dialog = Some(confirm(Then::Confirm {
        action,
        title: naming.title,
        lines: naming.lines,
        name,
    }));
    Vec::new()
}
