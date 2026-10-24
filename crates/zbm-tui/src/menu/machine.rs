//! What the menu runs on: a [`Machine`] answers the menu's actions with
//! events. [`demo::FakeMachine`](super::demo::FakeMachine) is the fake
//! machine of `--demo` and of the tests; [`RealMachine`] is the machine the
//! menu boots, through `zbm-app` (REFACTOR-ROADMAP.md, R5 step 3).

mod real;

pub use real::RealMachine;

use super::world::{Action, Event, Snapshot};

/// The machine the menu runs on.
pub trait Machine {
    /// The machine as the menu first reads it.
    fn snapshot(&self) -> &Snapshot;

    /// Time passed: what happened meanwhile.
    fn tick(&mut self) -> Vec<Event>;

    /// Performs `action`; what happened comes back as events, the last of
    /// them saying how the action ended.
    fn apply(&mut self, action: Action) -> Vec<Event>;

    /// The user closed the dialog start-up waited on: ask again.
    fn reask(&mut self) {}

    /// Whether `action` takes the console, such as a boot that may ask for a
    /// passphrase or the recovery shell. The front end then hands the
    /// terminal over before [`Machine::apply`] performs it, and takes it back
    /// afterwards.
    fn needs_console(&self, _action: &Action) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
