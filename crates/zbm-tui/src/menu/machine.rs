//! What the menu runs on: a [`Machine`] answers the menu's actions with
//! events. [`demo::FakeMachine`](super::demo::FakeMachine) is the fake
//! machine of `--demo` and of the tests; [`RealMachine`] is the machine the
//! menu boots, through `zbm-app` (ROADMAP.md, R5 step 3).

mod real;

use std::os::fd::BorrowedFd;

pub use real::{Countdown, RealMachine};

use super::world::{Action, Event, Snapshot};

/// The machine the menu runs on.
pub trait Machine {
    /// The machine as the menu first reads it.
    fn snapshot(&self) -> &Snapshot;

    /// Time passed: what happened meanwhile.
    fn tick(&mut self) -> Vec<Event>;

    /// What the machine sent since, without time passing: the events of
    /// an action it performs on its own.
    fn poll(&mut self) -> Vec<Event> {
        Vec::new()
    }

    /// Performs `action`; what happened comes back as events, the last of
    /// them saying how the action ended. A machine that performs it on its
    /// own returns at once, busy ([`Machine::is_busy`]), and the events come
    /// with [`Machine::poll`] meanwhile.
    fn apply(&mut self, action: Action) -> Vec<Event>;

    /// Whether an action it performs on its own has not ended.
    fn is_busy(&self) -> bool {
        false
    }

    /// Cancels the actions it performs on its own: they end as cancelled.
    fn cancel(&mut self) {}

    /// Readable when [`Machine::poll`] has something, for the front end's
    /// `poll`.
    fn waker(&self) -> Option<BorrowedFd<'_>> {
        None
    }

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
