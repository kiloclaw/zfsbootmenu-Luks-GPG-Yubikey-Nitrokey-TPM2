use std::collections::VecDeque;
use std::os::fd::BorrowedFd;
use std::time::Duration;

use ratatui::backend::TestBackend;
use zbm_app::{App, BootError, Recovery, Runtime};
use zbm_display::keys::{Key, KeyPress};
use zbm_platform::Context;
use zbm_testkit::Shared;
use zbm_testkit::fakes::{FakeClock, FakeSystem};

use super::*;
use crate::menu::caps::Capabilities;
use crate::menu::keymap::{Map, Modifiers, press};
use crate::menu::machine::Countdown;
use crate::menu::world::Snapshot;

/// Keys from a script, `None` for the time asked passing; then time passing.
#[derive(Clone, Default)]
struct Scripted {
    keys: Shared<VecDeque<Option<KeyPress>>>,
    clock: FakeClock,
    modes: Shared<Vec<&'static str>>,
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
}

/// No recovery shell.
pub(crate) struct NoShell;

impl Recovery for NoShell {
    fn unlock_late(&self) -> Result<(), BootError> {
        Err(BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, _: &str) {}
}

/// A machine whose `zpool` and `zfs` find nothing.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    fake
}

/// The screen on scripted keys, going on from `model` when given, else
/// with a new menu with the key map `map`.
fn screen(
    input: &Scripted,
    model: Option<Model>,
    map: Map,
) -> SessionScreen<TestBackend, Scripted> {
    let frontend = Frontend::new(TestBackend::new(80, 25), input.clone())
        .unwrap()
        .with_clock(input.clock.clone());
    SessionScreen::new(frontend, model, Theme::new(Capabilities::FULL), map)
}

/// One round of the menu on `screen`, on the machine `fake`, which ends
/// once time passes: another instance took over.
fn round(fake: &FakeSystem, screen: &mut SessionScreen<TestBackend, Scripted>) -> String {
    let stop = || true;
    let runtime = Runtime::default();
    let app = App::new(Context::new(fake.system()), &runtime);
    let machine = RealMachine::new(&app, fake.system(), &NoShell, &stop, Countdown::Skip).unwrap();
    screen.run(machine).unwrap()
}

#[test]
fn the_menu_goes_on_with_its_model_from_one_round_to_the_next() {
    let fake = machine();
    let input = Scripted::default();
    let mut screen = screen(&input, None, Map::Classic);

    input
        .keys
        .borrow_mut()
        .extend([Some(press(Key::Down, Modifiers::NONE)), None]);
    assert_eq!(round(&fake, &mut screen), "");
    assert_eq!(screen.model().map(|model| model.keys), Some(1));
    input.keys.borrow_mut().push_back(None);
    assert_eq!(round(&fake, &mut screen), "");

    // The same model, its key presses counted on.
    assert_eq!(screen.model().map(|model| model.keys), Some(1));
    assert_eq!(
        *input.modes.borrow(),
        ["take", "release", "take", "release"]
    );
}

#[test]
fn a_new_menu_reads_keys_with_the_screen_s_map_from_one_round_to_the_next() {
    let fake = machine();
    let input = Scripted::default();
    let mut screen = screen(&input, None, Map::Workspace);

    assert_eq!(round(&fake, &mut screen), "");
    assert_eq!(screen.model().map(|model| model.map), Some(Map::Workspace));
    // The next round begins on the same model, its map kept.
    assert_eq!(round(&fake, &mut screen), "");
    assert_eq!(screen.model().map(|model| model.map), Some(Map::Workspace));
}

#[test]
fn a_menu_that_goes_on_from_a_model_keeps_the_model_s_map() {
    let fake = machine();
    let input = Scripted::default();
    let mut model = Model::new(Snapshot::default(), Theme::new(Capabilities::FULL));
    model.map = Map::Workspace;
    let mut screen = screen(&input, Some(model), Map::Classic);

    assert_eq!(round(&fake, &mut screen), "");
    assert_eq!(screen.model().map(|model| model.map), Some(Map::Workspace));
}
