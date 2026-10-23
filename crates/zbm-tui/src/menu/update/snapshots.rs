//! The snapshots view: the actions on a snapshot, with a typed confirmation
//! for those that lose data.

use zbm_types::SnapshotName;

use super::dialogs;
use crate::menu::keymap::Intent;
use crate::menu::model::{Choice, Model, Then};
use crate::menu::world::{Action, SnapshotInfo};

/// How many marked snapshots a diff compares.
const MARKS: usize = 2;

/// What `intent` does on the snapshots view.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    if intent == Intent::Sort {
        model.sort = model.sort.next();
        model.refresh();
        return Vec::new();
    }
    if intent == Intent::NewSnapshot {
        return model
            .focus
            .clone()
            .map(Action::CreateSnapshot)
            .into_iter()
            .collect();
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
        Intent::Clone => Then::Act(Action::Clone(name)),
        Intent::Duplicate => Then::Act(Action::Duplicate(name)),
        Intent::CloneAndPromote => promote(&name),
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
    let choice = |label: &str, then: Then| Choice {
        label: label.to_owned(),
        then,
    };
    let choices = vec![
        choice(
            "Boot a Temporary Clone",
            Then::Act(Action::BootSnapshot(name.clone())),
        ),
        choice("Roll Back to It", rollback(model, snapshot)),
        choice(
            "Clone as a New Boot Environment",
            Then::Act(Action::Clone(name.clone())),
        ),
        choice(
            "Duplicate as an Independent Copy",
            Then::Act(Action::Duplicate(name.clone())),
        ),
        choice("Clone and Promote", promote(name)),
        choice(
            "Show Differences to Now",
            Then::Act(Action::Diff(name.clone(), None)),
        ),
        choice(
            "Chroot into It, Read-Only",
            Then::Act(Action::Chroot(name.to_string())),
        ),
    ];
    let lines = vec![format!(
        "{} · {} · {}",
        name,
        crate::menu::format::age(model.snapshot.now, snapshot.created),
        crate::menu::format::size(snapshot.used)
    )];
    dialogs::choose(&format!("Snapshot {}", name.name()), lines, choices)
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
    let mut lines = vec![format!(
        "{environment} goes back to {}: every change since is lost.",
        snapshot.name.name()
    )];
    if newer.is_empty() {
        lines.push("No newer snapshot is destroyed.".to_owned());
    } else {
        let size: u64 = newer.iter().map(|s| s.used).sum();
        lines.push(format!(
            "{} newer snapshots are destroyed ({}), the newest {}.",
            newer.len(),
            crate::menu::format::size(size),
            newer
                .iter()
                .max_by_key(|s| s.created)
                .map_or("", |s| s.name.name())
        ));
    }
    lines.push(format!(
        "A safety snapshot {environment}@before-rollback is taken first: roll back to it to undo."
    ));
    Then::Confirm {
        action: Action::Rollback {
            snapshot: snapshot.name.clone(),
            safety: true,
        },
        title: format!("Roll Back {environment}"),
        lines,
        name: environment.last_component().to_owned(),
    }
}

/// The confirmation of a clone and promote.
fn promote(name: &SnapshotName) -> Then {
    let environment = name.dataset();
    Then::Confirm {
        action: Action::CloneAndPromote(name.clone()),
        title: format!("Clone and Promote {}", name.name()),
        lines: vec![
            format!(
                "The clone takes over the snapshots of {environment} up to {}.",
                name.name()
            ),
            format!("{environment} then depends on the clone, and cannot be destroyed without it."),
        ],
        name: environment.last_component().to_owned(),
    }
}
