//! The snapshots view: the actions on a snapshot, with a typed confirmation
//! for those that lose data.

use zbm_types::{DatasetName, SnapshotName};

use super::dialogs;
use crate::menu::format::{age, size};
use crate::menu::keymap::Intent;
use crate::menu::model::{Choice, Make, Model, Naming, Then};
use crate::menu::world::{Action, SnapshotInfo};
use crate::text::text;

/// How many marked snapshots a diff compares.
const MARKS: usize = 2;

/// What `intent` does on the snapshots view.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    if intent == Intent::Sort {
        model.set_sort(model.sort.next());
        return Vec::new();
    }
    if intent == Intent::NewSnapshot {
        let Some(environment) = model.focus.clone() else {
            return Vec::new();
        };
        let then = new_snapshot(model, environment);
        return dialogs::follow(model, then);
    }
    let Some(snapshot) = model.selected_snapshot().cloned() else {
        return Vec::new();
    };
    let name = snapshot.name.clone();
    let then = match intent {
        Intent::Primary => {
            model.dialog = Some(actions(model, &snapshot));
            return Vec::new();
        }
        Intent::Mark => {
            mark(model, &name);
            return Vec::new();
        }
        Intent::Diff => Then::Act(diff(model, &name)),
        Intent::Boot => Then::Act(Action::BootSnapshot(name)),
        Intent::Rollback => rollback(model, &snapshot),
        Intent::Clone => clone(model, &name),
        Intent::Duplicate => duplicate(model, &snapshot),
        Intent::CloneAndPromote => promote(model, &name),
        Intent::Chroot => Then::Act(Action::Chroot(name.to_string())),
        _ => return Vec::new(),
    };
    dialogs::follow(model, then)
}

/// Marks or unmarks a snapshot for a diff; the oldest mark goes when a third
/// is set.
fn mark(model: &mut Model, name: &SnapshotName) {
    if let Some(position) = model.marked.iter().position(|marked| marked == name) {
        model.marked.remove(position);
        return;
    }
    if model.marked.len() == MARKS {
        model.marked.remove(0);
    }
    model.marked.push(name.clone());
}

/// The diff of the marked snapshots, or of the selection against now.
fn diff(model: &Model, selected: &SnapshotName) -> Action {
    match model.marked.as_slice() {
        [first, second] => Action::Diff(first.clone(), Some(second.clone())),
        [only] => Action::Diff(only.clone(), None),
        _ => Action::Diff(selected.clone(), None),
    }
}

/// The choice of actions for a snapshot.
fn actions(model: &Model, snapshot: &SnapshotInfo) -> crate::menu::model::Dialog {
    let name = &snapshot.name;
    let choice = |label: String, then: Then| Choice { label, then };
    let choices = vec![
        choice(
            text!("snapshot-boot-clone"),
            Then::Act(Action::BootSnapshot(name.clone())),
        ),
        choice(text!("snapshot-roll-back"), rollback(model, snapshot)),
        choice(text!("snapshot-clone"), clone(model, name)),
        choice(text!("snapshot-duplicate"), duplicate(model, snapshot)),
        choice(text!("snapshot-promote"), promote(model, name)),
        choice(
            text!("snapshot-diff"),
            Then::Act(Action::Diff(name.clone(), None)),
        ),
        choice(
            text!("snapshot-chroot"),
            Then::Act(Action::Chroot(name.to_string())),
        ),
    ];
    let lines = vec![format!(
        "{} · {} · {}",
        name,
        age(model.snapshot.now, snapshot.created),
        size(snapshot.used)
    )];
    dialogs::choose(
        &text!("snapshot-title", snapshot = name.name()),
        lines,
        choices,
    )
}

/// The confirmation of a rollback: what is lost, and the name to type.
fn rollback(model: &Model, snapshot: &SnapshotInfo) -> Then {
    let environment = snapshot.name.dataset();
    let newer: Vec<&SnapshotInfo> = model
        .focused()
        .map(|focused| {
            focused
                .snapshots
                .iter()
                .filter(|s| s.created > snapshot.created)
                .collect()
        })
        .unwrap_or_default();
    let environment_name = environment.to_string();
    let mut lines = vec![text!(
        "rollback",
        environment = environment_name.as_str(),
        snapshot = snapshot.name.name()
    )];
    if newer.is_empty() {
        lines.push(text!("rollback.none-destroyed"));
    } else {
        let used: u64 = newer.iter().map(|s| s.used).sum();
        lines.push(text!(
            "rollback.destroyed",
            count = newer.len(),
            size = size(used),
            newest = newer
                .iter()
                .max_by_key(|s| s.created)
                .map_or("", |s| s.name.name())
        ));
    }
    lines.push(text!("rollback.undo"));
    Then::Confirm {
        action: Action::Rollback(snapshot.name.clone()),
        title: text!("rollback.title", environment = environment_name),
        lines,
        name: environment.last_component().to_owned(),
    }
}

/// The name of a new snapshot of `environment`.
pub(super) fn new_snapshot(model: &Model, environment: DatasetName) -> Then {
    let shown = environment.to_string();
    Then::Name(Naming::new(
        Make::Snapshot(environment),
        model.snapshot.now,
        text!("name-snapshot.title", environment = shown.as_str()),
        vec![text!("name-snapshot", environment = shown)],
    ))
}

/// The name of a clone of `name`.
fn clone(model: &Model, name: &SnapshotName) -> Then {
    Then::Name(Naming::new(
        Make::Clone(name.clone()),
        model.snapshot.now,
        text!("name-clone.title", snapshot = name.name()),
        vec![text!("name-clone", snapshot = name.to_string())],
    ))
}

/// The name of an independent copy of `snapshot`.
fn duplicate(model: &Model, snapshot: &SnapshotInfo) -> Then {
    let name = &snapshot.name;
    Then::Name(Naming::new(
        Make::Duplicate(name.clone()),
        model.snapshot.now,
        text!("name-duplicate.title", snapshot = name.name()),
        vec![text!("name-duplicate", snapshot = name.to_string())],
    ))
}

/// The name of a clone of `name` that is promoted: what promoting does.
fn promote(model: &Model, name: &SnapshotName) -> Then {
    let environment = name.dataset();
    Then::Name(Naming::new(
        Make::Promote(name.clone()),
        model.snapshot.now,
        text!("promote.title", snapshot = name.name()),
        vec![
            text!(
                "promote",
                environment = environment.to_string(),
                snapshot = name.name()
            ),
            text!("promote.depends", environment = environment.to_string()),
        ],
    ))
}
