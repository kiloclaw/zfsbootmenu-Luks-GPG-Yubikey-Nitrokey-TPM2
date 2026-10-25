use std::cell::Cell;

use ratatui::crossterm::event::KeyCode;
use zbm_app::{App, BootError, Recovery};
use zbm_platform::Context;
use zbm_testkit::fakes::FakeSystem;

use super::real::Countdown;
use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::session::Session;
use crate::menu::theme::Theme;
use crate::menu::world::{Outcome, Timeout};

/// A machine whose `Shell` takes the console, and that records what it was
/// asked to do.
struct Recorded {
    snapshot: Snapshot,
    applied: Vec<Action>,
}

impl Machine for Recorded {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn tick(&mut self) -> Vec<Event> {
        vec![Event::Changed(self.snapshot.clone())]
    }

    fn apply(&mut self, action: Action) -> Vec<Event> {
        self.applied.push(action);
        vec![Event::Finished(Outcome::Done("done".to_owned()))]
    }

    fn needs_console(&self, action: &Action) -> bool {
        action.needs_console()
    }
}

fn theme() -> Theme {
    Theme::new(Capabilities::FULL)
}

#[test]
fn actions_that_take_the_console_wait_for_the_front_end() {
    let machine = Recorded {
        snapshot: Snapshot::default(),
        applied: Vec::new(),
    };
    let mut session = Session::on(machine, theme());
    session.tick(0);
    // The recovery shell: the first of the recovery tools.
    session.key(KeyCode::Char('!'));
    session.key(KeyCode::Enter);
    let Some(action) = session.console_action() else {
        panic!("no console action: {:?}", session.actions);
    };
    assert_eq!(action, Action::Shell);
    assert!(session.console_action().is_none());
    session.perform(action);
    assert_eq!(session.actions.last(), Some(&Action::Shell));
}

/// A recovery that counts its shells.
#[derive(Default)]
struct Shells(usize);

impl Recovery for Shells {
    fn unlock_late(&mut self) -> Result<(), BootError> {
        Err(BootError::NoCryptsetup)
    }

    fn emergency_shell(&mut self, _message: &str) {
        self.0 += 1;
    }
}

/// A machine with one pool and no boot environments.
fn fake() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    fake
}

#[test]
fn the_real_machine_opens_the_menu_and_reads_itself_again_after_an_action() {
    let fake = fake();
    let app = App::new(Context::new(fake.system()));
    let mut shells = Shells::default();
    let stopped = Cell::new(false);
    let stop = || stopped.get();
    let mut machine =
        RealMachine::new(app, fake.system(), &mut shells, &stop, Countdown::Run).unwrap();
    assert_eq!(machine.snapshot().timeout, Timeout::Wait);
    assert!(matches!(machine.tick().as_slice(), [Event::Changed(_)]));
    assert!(machine.tick().is_empty());

    assert!(machine.needs_console(&Action::Reboot));
    assert!(!machine.needs_console(&Action::Shell));
    assert!(!machine.needs_console(&Action::ResetBootTries("tank/ROOT/be".parse().unwrap())));
    let events = machine.apply(Action::Shell);
    assert!(matches!(events.as_slice(), [Event::Leaving(why)] if why.is_empty()));
    // The fake has no /proc: the reset fails, and the menu goes on.
    let events = machine.apply(Action::Reboot);
    assert!(
        matches!(
            events.as_slice(),
            [Event::Changed(_), Event::Finished(Outcome::Failed { .. })]
        ),
        "{events:?}"
    );
    let events = machine.apply(Action::ResetBootTries("tank/ROOT/be".parse().unwrap()));
    assert!(matches!(
        events.as_slice(),
        [Event::Changed(_), Event::Finished(Outcome::Failed { .. })]
    ));

    stopped.set(true);
    assert!(matches!(machine.tick().as_slice(), [Event::Leaving(_)]));
    assert!(matches!(
        machine.apply(Action::Reboot).last(),
        Some(Event::Leaving(_))
    ));
    drop(machine);
    assert_eq!(shells.0, 0);
}

#[test]
fn a_machine_that_cannot_be_read_is_an_error() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 1, "");
    fake.runner.respond_program("zfs", 1, "");
    let app = App::new(Context::new(fake.system()));
    let mut shells = Shells::default();
    let stop = || false;
    assert!(RealMachine::new(app, fake.system(), &mut shells, &stop, Countdown::Run).is_err());
}
