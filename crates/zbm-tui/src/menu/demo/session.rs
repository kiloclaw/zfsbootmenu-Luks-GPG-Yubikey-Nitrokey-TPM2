//! The menu on the fake machine: what `--demo` runs.

use super::{DemoOptions, FakeMachine};
use crate::menu::machine::Machine;
use crate::menu::session::Session;
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Event, Snapshot};

impl Session<FakeMachine> {
    /// The menu on the demo machine `options` describe, at power-on.
    pub fn new(options: &DemoOptions, theme: Theme) -> Session<FakeMachine> {
        Session::on(FakeMachine::new(options), theme)
    }
}

impl Machine for FakeMachine {
    fn snapshot(&self) -> &Snapshot {
        FakeMachine::snapshot(self)
    }

    fn tick(&mut self) -> Vec<Event> {
        FakeMachine::tick(self)
    }

    fn apply(&mut self, action: Action) -> Vec<Event> {
        FakeMachine::apply(self, action)
    }

    fn reask(&mut self) {
        FakeMachine::reask(self);
    }
}
