//! What the machine's events do to the menu.

use crate::menu::model::{Choice, Dialog, DiffView, Model, Prompt, Screen, Then};
use crate::menu::world::{Action, Event, Outcome, Stage, StageState, Target, UnlockOutcome};
use crate::text::text;
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
            notice,
        } => {
            let prompt = Prompt::new(volume, method, tries_left, notice);
            model.dialog = Some(Dialog::Unlock(prompt));
        }
        Event::ResealOffered(volume) => {
            if let Some(then) = super::unlock::reseal(model, &volume) {
                model.dialog = Some(super::dialogs::confirm(then));
            }
        }
        Event::ImportProblem { pool, reason } => import_problem(model, &pool, reason),
        Event::DiffReady {
            from,
            to,
            changes,
            packages,
        } => {
            let kept = model
                .diff
                .as_ref()
                .filter(|diff| diff.from == from)
                .map(|diff| (diff.list.selected, diff.segment));
            let mut view = DiffView::new((from, to), changes, packages);
            let last = view.list.matches.len().saturating_sub(1);
            let (selected, segment) = kept.unwrap_or_default();
            view.list.selected = selected.min(last);
            view.segment = segment;
            model.diff = Some(view);
            model.screen = Screen::Diff;
        }
        Event::Finished(outcome) => {
            super::show_failure(model, &outcome);
            model.notification = Some(outcome);
        }
        Event::Leaving(why) => model.leaving = Some(why),
        Event::StageStarted(name) => stage(model, name, StageState::Running),
        Event::StageFinished { stage: name, state } => stage(model, name, state),
        Event::UnlockFinished { volume, outcome } => {
            model.notification = Some(unlocked(&volume, outcome));
        }
    }
}

/// The notification of how unlocking `volume` ended.
fn unlocked(volume: &str, outcome: UnlockOutcome) -> Outcome {
    match outcome {
        UnlockOutcome::Opened => Outcome::Done(text!("unlock-opened", volume = volume)),
        UnlockOutcome::Skipped => Outcome::Warning(text!("unlock-skipped", volume = volume)),
        UnlockOutcome::Failed(why) => Outcome::Failed {
            what: text!("unlock-failed", volume = volume),
            why: vec![why],
            next: Vec::new(),
        },
    }
}

/// Records how far the start-up stage `name` got, adding it when the
/// snapshot did not list it, and opens the main view once start-up is over.
fn stage(model: &mut Model, name: String, reached: StageState) {
    let startup = &mut model.snapshot.startup;
    match startup.iter_mut().find(|known| known.name == name) {
        Some(known) => known.state = reached,
        None => startup.push(Stage {
            name,
            state: reached,
        }),
    }
    start(model);
}

/// Opens the main view and arms the countdown once start-up is over and no
/// dialog waits for the user.
pub(super) fn start(model: &mut Model) {
    if model.started || !model.is_started() || model.dialog.is_some() {
        return;
    }
    model.started = true;
    model.screen = Screen::Main;
    model.countdown = model.timeout_ms();
    // What the countdown boots: the fallback of a default that used up its
    // tries, else the environment the running system asked for once, else
    // the one start-up chose.
    let fallback = model.snapshot.fallback.as_ref().map(|f| f.target.clone());
    let target = match fallback {
        Some(Target::Environment(name)) => Some(name),
        Some(Target::Snapshot(_)) => None,
        None => model
            .snapshot
            .next_boot
            .clone()
            .or_else(|| model.snapshot.bootfs.clone()),
    };
    if let Some(name) = target {
        model.select_environment(&name);
    }
    if model.snapshot.environments.is_empty() {
        model.countdown = None;
        model.notification = Some(Outcome::Warning(text!("environments-none.notice")));
    }
}

/// The choices of a pool that did not import.
fn import_problem(model: &mut Model, pool: &zbm_types::PoolName, reason: String) {
    let act = |label: String, then: Then| Choice { label, then };
    let choices = vec![
        act(
            text!("import-retry"),
            Then::Act(Action::Import(pool.clone(), ImportPolicy::Strict)),
        ),
        act(
            text!("pool-import-last-host"),
            Then::Act(Action::Import(pool.clone(), ImportPolicy::Hostid)),
        ),
        act(
            text!("pool-force-import"),
            super::dialogs::force_import(pool),
        ),
        act(text!("import-shell"), Then::Act(Action::Shell)),
    ];
    model.dialog = Some(super::dialogs::choose(
        &text!("import-failed.title", pool = pool.to_string()),
        vec![reason],
        choices,
    ));
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::menu::caps::Capabilities;
    use crate::menu::theme::Theme;
    use crate::menu::world::Snapshot;

    #[test]
    fn stage_and_unlock_events_update_the_menu() {
        let mut model = Model::new(Snapshot::default(), Theme::new(Capabilities::FULL));

        apply(&mut model, Event::StageStarted("Importing pools".into()));
        assert_eq!(model.snapshot.startup[0].state, StageState::Running);
        let finished = Event::StageFinished {
            stage: "Importing pools".into(),
            state: StageState::Done,
        };
        apply(&mut model, finished);
        assert_eq!(model.snapshot.startup.len(), 1);
        assert_eq!(model.snapshot.startup[0].state, StageState::Done);

        let opened = Event::UnlockFinished {
            volume: "zroot-a".into(),
            outcome: UnlockOutcome::Opened,
        };
        apply(&mut model, opened);
        assert_eq!(
            model.notification,
            Some(Outcome::Done("zroot-a is open".into()))
        );
        assert_eq!(
            unlocked("b", UnlockOutcome::Skipped),
            Outcome::Warning("b stays locked".into())
        );
        assert!(matches!(
            unlocked("c", UnlockOutcome::Failed("wrong".into())),
            Outcome::Failed { why, .. } if why == ["wrong"]
        ));
    }
}
