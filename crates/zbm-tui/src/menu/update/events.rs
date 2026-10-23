//! What the machine's events do to the menu.

use crate::menu::model::{Choice, Dialog, DiffView, Model, Screen, Then};
use crate::menu::world::{Action, Event, Outcome, Target};
use zbm_types::ImportPolicy;

/// Applies an event.
pub(super) fn apply(model: &mut Model, event: Event) {
    match event {
        Event::Changed(snapshot) => {
            model.snapshot = snapshot;
            model.refresh();
            start(model);
        }
        Event::UnlockRequested {
            volume,
            method,
            tries_left,
        } => {
            model.dialog = Some(Dialog::Unlock {
                volume,
                method,
                tries_left,
                secret: zbm_platform::secret::Secret::new(),
            });
        }
        Event::ImportProblem { pool, reason } => import_problem(model, &pool, reason),
        Event::DiffReady { from, to, changes } => {
            let kept = model
                .diff
                .as_ref()
                .filter(|diff| diff.from == from)
                .map(|diff| diff.list.selected);
            let mut view = DiffView::new(from, to, changes);
            let last = view.list.matches.len().saturating_sub(1);
            view.list.selected = kept.unwrap_or(0).min(last);
            model.diff = Some(view);
            model.screen = Screen::Diff;
        }
        Event::Finished(outcome) => {
            super::show_failure(model, &outcome);
            model.notification = Some(outcome);
        }
        Event::Leaving(why) => model.leaving = Some(why),
    }
}

/// Opens the main view and arms the countdown once start-up is over.
fn start(model: &mut Model) {
    if model.started || !model.is_started() || model.dialog.is_some() {
        return;
    }
    model.started = true;
    model.screen = Screen::Main;
    model.countdown = model.timeout_ms();
    // What the countdown boots: the fallback of a default that used up its
    // tries, else the environment the running system asked for once.
    let fallback = model.snapshot.fallback.as_ref().map(|f| f.target.clone());
    let target = match fallback {
        Some(Target::Environment(name)) => Some(name),
        Some(Target::Snapshot(_)) => None,
        None => model.snapshot.next_boot.clone(),
    };
    if let Some(name) = target {
        model.select_environment(&name);
    }
    if model.snapshot.environments.is_empty() {
        model.countdown = None;
        model.notification = Some(Outcome::Warning(
            "no boot environment found: see the pools, or open the recovery shell".to_owned(),
        ));
    }
}

/// The choices of a pool that did not import.
fn import_problem(model: &mut Model, pool: &zbm_types::PoolName, reason: String) {
    let act = |label: &str, then: Then| Choice {
        label: label.to_owned(),
        then,
    };
    let choices = vec![
        act(
            "Retry",
            Then::Act(Action::Import(pool.clone(), ImportPolicy::Strict)),
        ),
        act(
            "Import with Its Last Host ID",
            Then::Act(Action::Import(pool.clone(), ImportPolicy::Hostid)),
        ),
        act("Force the Import", super::dialogs::force_import(pool)),
        act("Open the Recovery Shell", Then::Act(Action::Shell)),
    ];
    model.dialog = Some(super::dialogs::choose(
        &format!("{pool} Did Not Import"),
        vec![reason],
        choices,
    ));
}
