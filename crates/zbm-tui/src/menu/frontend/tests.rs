use std::collections::VecDeque;

use ratatui::backend::TestBackend;
use zbm_display::keys::Key;
use zbm_testkit::fakes::FakeClock;
use zbm_testkit::{Shared, TempDir};

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::demo::{DemoOptions, FakeMachine};
use crate::menu::keymap::press;
use crate::menu::theme::Theme;
use crate::menu::world::{Action, Event, Snapshot};

/// Keys from a script, `None` for the time asked passing without one on
/// the clock, then time passing; what was done to the input; its size.
#[derive(Clone, Default)]
struct Scripted {
    keys: Shared<VecDeque<Option<KeyPress>>>,
    clock: FakeClock,
    modes: Shared<Vec<&'static str>>,
    size: Shared<Option<(u16, u16)>>,
}

impl Scripted {
    fn press(&self, keys: &[Key]) {
        let presses = keys.iter().map(|&key| Some(press(key, Modifiers::NONE)));
        self.keys.borrow_mut().extend(presses);
    }

    fn wait(&self, ticks: usize) {
        self.keys
            .borrow_mut()
            .extend(std::iter::repeat_n(None, ticks));
    }
}

impl Input for Scripted {
    fn key_or_wake(
        &mut self,
        timeout: Duration,
        _: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        let key = self.keys.borrow_mut().pop_front().flatten();
        if key.is_none() {
            self.clock.advance(timeout);
        }
        Ok(key)
    }

    fn release(&mut self) -> io::Result<()> {
        self.modes.borrow_mut().push("release");
        Ok(())
    }

    fn take(&mut self) -> io::Result<()> {
        self.modes.borrow_mut().push("take");
        Ok(())
    }

    fn size(&mut self) -> Option<(u16, u16)> {
        self.size.get()
    }
}

fn frontend(input: &Scripted) -> Frontend<TestBackend, Scripted> {
    Frontend::new(TestBackend::new(80, 25), input.clone())
        .unwrap()
        .with_clock(input.clock.clone())
}

fn shown(frontend: &Frontend<TestBackend, Scripted>) -> String {
    let buffer = frontend.terminal().backend().buffer();
    buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
#[allow(
    clippy::disallowed_methods,
    reason = "the keys of the usability sessions are logged to a real file"
)]
fn the_demo_runs_on_any_surface_and_its_keys_are_logged() {
    let input = Scripted::default();
    input.wait(20);
    input.press(&[Key::Down, Key::Enter]);
    let dir = TempDir::new("zbm-tui-frontend");
    let log = dir.join("keys");
    let mut session = Session::on(
        FakeMachine::new(&DemoOptions::default()),
        Theme::new(Capabilities::FULL),
    );

    let why = frontend(&input)
        .with_key_log(File::create(&log).unwrap())
        .run(&mut session)
        .unwrap();

    assert!(why.starts_with("Booting"), "{why}");
    // Start-up took its ticks, then the last frame stayed a moment.
    assert!(input.clock.now() >= TICK * 20 + LAST_FRAME);
    assert_eq!(*input.modes.borrow(), ["release"]);
    let logged = std::fs::read_to_string(&log).unwrap();
    let names: Vec<&str> = logged
        .lines()
        .filter_map(|line| line.split_once(' ').map(|(_, name)| name))
        .collect();
    assert_eq!(names, ["Down", "Enter"]);
}

/// A machine whose recovery shell takes the console, and that ends the
/// menu once it ran.
struct Shell {
    snapshot: Snapshot,
    ran: bool,
}

impl Machine for Shell {
    fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn tick(&mut self) -> Vec<Event> {
        if self.ran {
            vec![Event::Leaving(String::new())]
        } else {
            vec![Event::Changed(self.snapshot.clone())]
        }
    }

    fn apply(&mut self, action: Action) -> Vec<Event> {
        self.ran = action == Action::Shell;
        Vec::new()
    }

    fn needs_console(&self, action: &Action) -> bool {
        *action == Action::Shell
    }
}

#[test]
fn the_console_is_lent_to_what_takes_it_and_the_size_followed() {
    let input = Scripted::default();
    input.wait(1);
    // The recovery shell, the first of the recovery tools.
    input.press(&[Key::Char('!'), Key::Enter]);
    input.size.set(Some((100, 30)));
    input.wait(3);
    let mut session = Session::on(
        Shell {
            snapshot: Snapshot::default(),
            ran: false,
        },
        Theme::new(Capabilities::FULL),
    );
    let mut frontend = frontend(&input);
    frontend.draw(session.model()).unwrap();
    assert_eq!(frontend.terminal().backend().buffer().area.width, 100);
    assert!(!shown(&frontend).trim().is_empty());

    let why = frontend.run(&mut session).unwrap();

    assert_eq!(why, "");
    assert_eq!(*input.modes.borrow(), ["release", "take", "release"]);
}
