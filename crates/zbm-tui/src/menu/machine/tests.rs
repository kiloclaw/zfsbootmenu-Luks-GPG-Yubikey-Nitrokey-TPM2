use std::cell::Cell;

use zbm_app::{App, BootError, Recovery};
use zbm_display::keys::Key;
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
    session.key(Key::Char('!'));
    session.key(Key::Enter);
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

/// The demo machine, on which taking a snapshot runs for a minute of ticks
/// of a second each, as a slow `zfs` would, while the menu goes on.
struct Slow {
    inner: crate::menu::demo::FakeMachine,
    /// The snapshot being taken, and the ticks it still takes.
    running: Option<(Action, u32)>,
    /// What [`Machine::poll`] gives next.
    outbox: Vec<Event>,
}

impl Machine for Slow {
    fn snapshot(&self) -> &Snapshot {
        self.inner.snapshot()
    }

    fn tick(&mut self) -> Vec<Event> {
        match self.running.take() {
            Some((action, 1)) => return self.inner.apply(action),
            Some((action, left)) => self.running = Some((action, left - 1)),
            None => {}
        }
        self.inner.tick()
    }

    fn poll(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.outbox)
    }

    fn apply(&mut self, action: Action) -> Vec<Event> {
        if matches!(action, Action::CreateSnapshot(_)) {
            self.running = Some((action, 60));
            return Vec::new();
        }
        self.inner.apply(action)
    }

    fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    fn cancel(&mut self) {
        if self.running.take().is_some() {
            self.outbox.push(Event::Finished(Outcome::Cancelled));
        }
    }
}

/// The menu as drawn at the console's size.
fn drawn(session: &Session<Slow>) -> String {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 25)).unwrap();
    terminal
        .draw(|frame| crate::menu::view::view(session.model(), frame))
        .unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn the_menu_draws_and_counts_while_an_action_runs_and_escape_cancels_it() {
    let options = crate::menu::demo::DemoOptions::default();
    let machine = Slow {
        inner: crate::menu::demo::FakeMachine::new(&options),
        running: None,
        outbox: Vec::new(),
    };
    let mut session = Session::on(machine, theme());
    session.start();
    let snapshots = session.model().snapshot.environments[0].snapshots.len();
    session.key(Key::Char('s'));
    session.key(Key::Char('n'));
    session.key(Key::Enter);
    assert!(matches!(
        session.actions.last(),
        Some(Action::CreateSnapshot(_))
    ));

    // Half a minute of fake time: the menu draws each second, counting.
    for second in 1..=30_u64 {
        session.tick(1000);
        assert_eq!(session.model().working, Some(second * 1000));
        let screen = drawn(&session);
        assert!(
            screen.contains(&format!("Working for {second} s")),
            "{screen}"
        );
    }
    // Keys still move the menu; Escape cancels the action.
    session.key(Key::Down);
    session.key(Key::Escape);
    session.poll();

    assert_eq!(session.model().working, None);
    assert_eq!(session.model().notification, Some(Outcome::Cancelled));
    assert!(drawn(&session).contains("Cancelled"));
    for _ in 0..60 {
        session.tick(1000);
    }
    let taken = session.model().snapshot.environments[0].snapshots.len();
    assert_eq!(taken, snapshots, "the cancelled snapshot was taken");
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the service's worker runs on its own thread, in real time"
)]
fn with_a_service_the_real_machine_performs_quiet_actions_on_its_worker() {
    // Unanswered, a command succeeds without output: no pool, no
    // environment, and `tank` writable.
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let settings = zbm_app::BootSettings::default();
    let app = App::new(Context::new(fake.system()))
        .with_base(std::path::Path::new("/zfsbootmenu"))
        .with_boot(&settings);
    let mut shells = Shells::default();
    let stop = || false;
    let service = zbm_app::service::MachineService::new(app, fake.system());

    let events = std::thread::scope(|scope| {
        let machine = RealMachine::new(app, fake.system(), &mut shells, &stop, Countdown::Run)
            .unwrap()
            .with_service(service.start(scope).unwrap());
        let mut machine = machine;
        let _ = machine.tick();
        assert!(machine.waker().is_some());
        let take = Action::CreateSnapshot("tank/ROOT/be@new".parse().unwrap());
        assert!(machine.apply(take).is_empty());
        let mut events = Vec::new();
        while machine.is_busy() {
            std::thread::sleep(std::time::Duration::from_millis(1));
            events.extend(machine.poll());
        }
        events
    });

    assert!(
        matches!(
            events.as_slice(),
            [Event::Changed(snapshot), .., Event::Finished(_)] if snapshot.timeout == Timeout::Wait
        ),
        "{events:?}"
    );
    assert!(
        fake.runner
            .lines()
            .contains(&"zfs snapshot tank/ROOT/be@new".to_owned())
    );
}
