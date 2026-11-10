//! The snapshots view: the actions on a snapshot, with a review page, or
//! under the classic map a typed confirmation, for those that lose data.
//! The workspace destroys snapshots too: the marked ones, or the selected
//! one.

use zbm_types::{DatasetName, SnapshotName};

use super::action_of::action_of;
use super::dialogs;
use crate::menu::format::{age, size};
use crate::menu::keymap::{self, Context, Intent, Map};
use crate::menu::model::{Choice, Make, Model, Naming, Then};
use crate::menu::world::{Action, Outcome, SnapshotInfo};
use crate::text::text;

/// What `intent` does on the snapshots view: the order, the folds and the
/// marks change; `Enter` opens the actions of the selected snapshot;
/// another key does what [`action_of`] says, or says why it does nothing.
pub(super) fn intent(model: &mut Model, intent: Intent) -> Vec<Action> {
    if intent == Intent::Sort {
        model.set_sort(model.sort.next());
        return Vec::new();
    }
    // `Enter` on a folded group of the timeline opens it, as `z` does.
    if intent == Intent::Fold || (intent == Intent::Primary && model.folded_group().is_some()) {
        model.fold();
        return Vec::new();
    }
    if let Some(snapshot) = model.selected_snapshot().cloned() {
        match intent {
            Intent::Primary => {
                actions(model, &snapshot);
                return Vec::new();
            }
            Intent::Mark => {
                mark(model, &snapshot.name);
                return Vec::new();
            }
            _ => {}
        }
    }
    if let Some(then) = action_of(model, Context::Snapshots, intent) {
        return dialogs::follow(model, then);
    }
    match diff_of(model) {
        Err(count) if intent == Intent::Diff => {
            let said = if count > 2 {
                text!("diff-marks", count = count)
            } else {
                text!("diff-unread")
            };
            model.notification = Some(Outcome::Warning(said));
            Vec::new()
        }
        _ => on_folded(model, intent),
    }
}

/// Marks or unmarks a snapshot; as many as the person likes.
fn mark(model: &mut Model, name: &SnapshotName) {
    if !model.marked.remove(name) {
        model.marked.insert(name.clone());
    }
}

/// The diff of the two marked snapshots, the older against the newer, or
/// of the one marked or else the selection against now; none with
/// neither. Three marks or more are no diff, nor are marks while the
/// column's list is not read: then the count of the marks, which the
/// status line says.
pub(super) fn diff_of(model: &Model) -> Result<Option<Action>, usize> {
    let count = model.marked.len();
    match model.marked_by_age().as_deref() {
        _ if count > 2 => Err(count),
        Some([older, newer]) => Ok(Some(Action::Diff((*older).clone(), Some((*newer).clone())))),
        Some([only]) => Ok(Some(Action::Diff((*only).clone(), None))),
        Some(_) => Ok((model.selected_snapshot()).map(|s| Action::Diff(s.name.clone(), None))),
        None => Err(count),
    }
}

/// What a snapshot's `intent` does with no snapshot selected: on the
/// header of a folded group of the timeline, the status line says to open
/// it first; elsewhere nothing.
fn on_folded(model: &mut Model, intent: Intent) -> Vec<Action> {
    let context = model.context();
    if model.folded_group().is_some() && keymap::needs_snapshot(context, intent, model) {
        model.notification = Some(Outcome::Warning(text!("timeline-open-group")));
    }
    Vec::new()
}

/// Opens the choice of the actions the machine offers for a snapshot; the
/// workspace's destroys it too.
fn actions(model: &mut Model, snapshot: &SnapshotInfo) {
    let name = &snapshot.name;
    let choice = |label: String, then: Then| Choice { label, then };
    let mut choices = vec![
        choice(text!("snapshot-boot-clone"), boot_clone(model, name)),
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
            Then::Act(Action::Chroot(name.clone().into())),
        ),
    ];
    if model.map == Map::Workspace {
        choices.push(choice(text!("snapshot-destroy"), destroy(vec![name])));
    }
    // A middle dot, or `|` in ASCII, as over a serial line.
    let dot = model.theme.dot();
    let lines = vec![format!(
        "{name}{dot}{}{dot}{}",
        age(model.snapshot.now, snapshot.created),
        size(snapshot.used)
    )];
    let title = text!("snapshot-title", snapshot = name.name());
    dialogs::choose_offered(model, &title, lines, choices);
}

/// Asks the machine for the snapshots of the focused environment, which
/// its column shows once they come; never while the countdown runs, as
/// the boot it ends with would wait for them, and Escape would cancel
/// their load instead of stopping it; nor once the environment is gone,
/// as after its pool's export; nor, under the workspace, before start-up
/// is over: the start-up view has no machine to answer, and the menu's
/// first snapshot asks for them ([`Model::needs_load`]).
pub(super) fn load(model: &mut Model) -> Vec<Action> {
    let Some(environment) = model.focused().map(|e| e.name.clone()) else {
        return Vec::new();
    };
    let starting = model.map == Map::Workspace && !model.started;
    if model.countdown.is_some() || starting {
        return Vec::new();
    }
    model.reading = Some(environment.clone());
    model.unread = None;
    vec![Action::LoadSnapshots(environment)]
}

/// A rollback: under the workspace its review page, which the machine
/// plans; under the classic map its confirmation: what is lost, from the
/// snapshots read with the last snapshot of the machine or after it, and
/// the name to type.
pub(super) fn rollback(model: &Model, snapshot: &SnapshotInfo) -> Then {
    if model.map == Map::Workspace {
        return Then::Review(Action::Rollback(snapshot.name.clone()));
    }
    let environment = snapshot.name.dataset();
    let newer: Vec<&SnapshotInfo> = model
        .loaded_snapshots(environment)
        .unwrap_or_default()
        .iter()
        .filter(|s| s.created > snapshot.created)
        .collect();
    let environment_name = environment.to_string();
    let mut lines = vec![text!(
        "rollback",
        environment = environment_name.as_str(),
        snapshot = snapshot.name.name()
    )];
    if newer.is_empty() {
        lines.push(text!("rollback.none-destroyed"));
    } else {
        // Sizes come from the pool: their sum stops at the largest.
        let used = newer
            .iter()
            .fold(0, |sum: u64, s| sum.saturating_add(s.used));
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

/// The destroy of `snapshots`, the oldest first, after its review page,
/// which the workspace alone has.
pub(super) fn destroy(snapshots: Vec<&SnapshotName>) -> Then {
    Then::Review(Action::Destroy(snapshots.into_iter().cloned().collect()))
}

/// The destroy of the marked snapshots, the oldest first; in the order of
/// their names while one is not in the column's list, which the machine
/// then reads.
pub(super) fn destroy_marked(model: &Model) -> Then {
    destroy(
        model
            .marked_by_age()
            .unwrap_or_else(|| model.marked.iter().collect()),
    )
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
pub(super) fn clone(model: &Model, name: &SnapshotName) -> Then {
    Then::Name(Naming::new(
        Make::Clone(name.clone()),
        model.snapshot.now,
        text!("name-clone.title", snapshot = name.name()),
        vec![text!("name-clone", snapshot = name.to_string())],
    ))
}

/// The name of the clone of `name` that boots once it is made.
pub(super) fn boot_clone(model: &Model, name: &SnapshotName) -> Then {
    Then::Name(Naming::new(
        Make::BootClone(name.clone()),
        model.snapshot.now,
        text!("name-boot-clone.title", snapshot = name.name()),
        vec![text!("name-boot-clone", snapshot = name.to_string())],
    ))
}

/// The name of an independent copy of `snapshot`.
pub(super) fn duplicate(model: &Model, snapshot: &SnapshotInfo) -> Then {
    let name = &snapshot.name;
    Then::Name(Naming::new(
        Make::Duplicate(name.clone()),
        model.snapshot.now,
        text!("name-duplicate.title", snapshot = name.name()),
        vec![text!("name-duplicate", snapshot = name.to_string())],
    ))
}

/// The name of a clone of `name` that is promoted: what promoting does.
pub(super) fn promote(model: &Model, name: &SnapshotName) -> Then {
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
