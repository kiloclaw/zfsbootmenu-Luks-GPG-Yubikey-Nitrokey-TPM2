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
use crate::menu::keymap::{Modifiers, press};
use crate::menu::machine::Countdown;

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
struct NoShell;

impl Recovery for NoShell {
    fn unlock_late(&self) -> Result<(), BootError> {
        Err(BootError::NoCryptsetup)
    }

    fn emergency_shell(&self, _: &str) {}
}

#[test]
fn the_menu_goes_on_with_its_model_from_one_round_to_the_next() {
    let fake = FakeSystem::new();
    fake.runner.respond_program("zpool", 0, "");
    fake.runner.respond_program("zfs", 0, "");
    let input = Scripted::default();
    let frontend = Frontend::new(TestBackend::new(80, 25), input.clone())
        .unwrap()
        .with_clock(input.clock.clone());
    let theme = Theme::new(Capabilities::FULL);
    let mut screen = SessionScreen::new(frontend, None, theme);
    // Each round ends once time passes: another instance took over.
    let stop = || true;
    let round = |screen: &mut SessionScreen<TestBackend, Scripted>| {
        let runtime = Runtime::default();
        let app = App::new(Context::new(fake.system()), &runtime);
        let machine =
            RealMachine::new(&app, fake.system(), &NoShell, &stop, Countdown::Skip).unwrap();
        screen.run(machine).unwrap()
    };

    input
        .keys
        .borrow_mut()
        .extend([Some(press(Key::Down, Modifiers::NONE)), None]);
    assert_eq!(round(&mut screen), "");
    assert_eq!(screen.model().map(|model| model.keys), Some(1));
    input.keys.borrow_mut().push_back(None);
    assert_eq!(round(&mut screen), "");

    // The same model, its key presses counted on.
    assert_eq!(screen.model().map(|model| model.keys), Some(1));
    assert_eq!(
        *input.modes.borrow(),
        ["take", "release", "take", "release"]
    );
}
