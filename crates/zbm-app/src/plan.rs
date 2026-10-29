//! The dry run of a change (ROADMAP.md, PM8): the change itself, run
//! against a ZFS port that plans what it would make ([`zbm_zfs::DryRun`])
//! and needs that plan what they would do ([`Planning`]), so that the plan
//! is what the change does, with the commands the adapters would run.

use std::sync::Mutex;

use zbm_platform::System;
use zbm_zfs::{DryRun, ZfsChange, ZfsCli};

use crate::App;
use crate::changes::{Changes, OnMachine, Planning};
use crate::model::{Action, Event, Outcome, Plan, Step, StepKind};

/// The step of `change`, with its command.
fn step(change: ZfsChange) -> Step {
    let command = Some(change.to_string());
    let kind = match change {
        ZfsChange::Export(pool) => StepKind::Export(pool),
        ZfsChange::Set {
            dataset, property, ..
        } => StepKind::SetProperty { dataset, property },
        ZfsChange::Inherit { dataset, property } => StepKind::InheritProperty { dataset, property },
        ZfsChange::SetPool {
            pool,
            property,
            value,
        } if property == "bootfs" => StepKind::SetDefault {
            pool,
            environment: value,
        },
        ZfsChange::SetPool { pool, property, .. } => StepKind::SetPoolProperty { pool, property },
        ZfsChange::Snapshot(snapshot) => StepKind::Snapshot(snapshot),
        ZfsChange::Rollback(snapshot) => StepKind::Rollback(snapshot),
        ZfsChange::Clone {
            snapshot, target, ..
        } => StepKind::Clone { snapshot, target },
        ZfsChange::Promote(dataset) => StepKind::Promote(dataset),
        ZfsChange::SendReceive {
            snapshot, target, ..
        } => StepKind::Copy { snapshot, target },
        ZfsChange::InheritKey(dataset) => StepKind::InheritKey(dataset),
    };
    Step { kind, command }
}

impl App<'_> {
    /// What the change `action` would do on the machine `console`, the
    /// front end's whole system, without doing it: nothing is changed,
    /// imported or unlocked. `None` for an action that changes nothing,
    /// such as the differences of a snapshot.
    pub fn plan(&self, action: &Action, console: System<'_>) -> Option<Plan> {
        if !action.is_change() || matches!(action, Action::Diff(..)) {
            return None;
        }
        let system = console.with_runner(self.context.runner());
        let state = zbm_state::State::new(system.files, &self.runtime.base);
        let steps = Mutex::new(Vec::new());
        let real = ZfsCli::new(system.runner);
        let planned = |change: ZfsChange| {
            if let Ok(mut steps) = steps.lock() {
                steps.push(step(change));
            }
        };
        let zfs = DryRun::new(&real, &planned);
        let machine = OnMachine {
            system: &system,
            state: &state,
            settings: &self.runtime.boot,
            asks: false,
        };
        let mut needs = Planning {
            machine,
            steps: &steps,
        };
        let events = Changes {
            zfs: &zfs,
            needs: &mut needs,
        }
        .perform(action)?;
        // Every change ends with how it ended.
        let finished = events.into_iter().rev().find_map(|event| match event {
            Event::Finished(outcome) => Some(outcome),
            _ => None,
        });
        let outcome = finished.unwrap_or_else(|| Outcome::Done(String::new()));
        Some(Plan {
            steps: steps.into_inner().unwrap_or_default(),
            outcome,
        })
    }

    /// The events of [`Action::Plan`] of `action`.
    pub(crate) fn planned(&self, action: &Action, console: System<'_>) -> Vec<Event> {
        match self.plan(action, console) {
            Some(plan) => vec![Event::Planned {
                action: Box::new(action.clone()),
                plan,
            }],
            None => self.alone(action),
        }
    }
}

#[cfg(test)]
mod tests;
