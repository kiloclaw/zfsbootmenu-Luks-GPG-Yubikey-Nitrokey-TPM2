//! `runtime-rust-console-serial`: the console on the display serves the
//! serial console too (ROADMAP.md UI9). The image of `runtime-rust-console`
//! boots with the serial console last (`console=tty1 console=ttyS0`): the
//! console starts on the display and serves `ttyS0`, 80x25 (the defaults
//! of `zbm.columns` and `zbm.lines`), the screen the test reads; the text
//! of the display's screen goes to its mirror on `ttyS1`.
//!
//! - The countdown and the menu show on the serial console's screen.
//! - Escape, `!` and Enter typed on the serial console open the menu, then
//!   the recovery shell.
//! - A command typed on the serial console sees its size, 80x25, and the
//!   display shows that screen: its mirror has no wider line. Typed on
//!   QEMU's keyboard, it sees the display's size, wider; typed on the
//!   serial console again, the serial console's again.
//! - Ctrl+L typed on the serial console clears its screen and writes all
//!   of it again.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::boot::prompt;
use crate::error::{Error, Result};
use crate::expect::{SCREEN, regex};
use crate::images::zbm_image;
use crate::qemu::Machine;
use crate::record::Test;
use crate::vm::Vm;

use super::MENU_COUNTDOWN;
use super::console::{Pace, key_names, last_screen, machine_extra, wait_screen};

/// The command line: the serial console last, the console on the display
/// with its mirror.
const APPEND: &str = "console=tty1 console=ttyS0 loglevel=7 zbm.console=drm \
    zbm.console_mirror=/dev/ttyS1 zbm.timeout=60";

/// The serial console's size, columns and rows: the defaults of
/// `zbm.columns` and `zbm.lines`.
const SERIAL_SIZE: (u32, u32) = (80, 25);

/// What clears the serial console's screen, before all of it is written.
const CLEAR: &[u8] = b"\x1b[2J";

/// How long the serial console may take to be written again after Ctrl+L.
const REPAINT_PATIENCE: Duration = Duration::from_secs(30);

/// The pace of the VM.
const PACE: Pace = Pace {
    pause: Duration::from_secs(1),
    patience: 1200,
};

/// Boots the image and checks the serial console the console serves.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let image = zbm_image(test.settings(), &super::console::spec())?;
    let mirror = test.dir().join("mirror-serial.txt");
    let mut machine = Machine::boot(&image, APPEND).with_disks([super::disk(test.settings(), 0)?]);
    machine.extra.extend(machine_extra("virtio", &mirror));
    let mut vm = Vm::start(test, "virtio", &machine)?;
    let serial = Serial {
        test,
        mirror: &mirror,
        key: sendkey,
        pace: PACE,
    };
    serial.check(&mut vm)?;
    vm.kill();
    Ok(())
}

/// Presses `key` on the machine's keyboard for 50 milliseconds.
fn sendkey(vm: &Vm<'_>, key: &str) -> Result<()> {
    vm.send_key(key)
}

/// Where a command is typed.
#[derive(Clone, Copy, Debug)]
enum Keyboard {
    /// On the serial console.
    Serial,
    /// On QEMU's keyboard, which the display's console reads.
    Display,
}

/// The size, columns and rows, in `screen` after
/// `echo zbm-NAME-$(stty size)`.
fn size_in(screen: &str, name: &str) -> Option<(u32, u32)> {
    let captures = regex(&format!(r"(?m)^zbm-{name}-(\d+) (\d+)\s*$"))
        .ok()?
        .captures(screen)?;
    let number = |group: usize| captures.get(group)?.as_str().parse().ok();
    Some((number(2)?, number(1)?))
}

/// The number of characters of the widest line of `screen`, without the
/// blanks at its end.
fn widest(screen: &str) -> usize {
    screen
        .lines()
        .map(|line| line.trim_end().chars().count())
        .max()
        .unwrap_or(0)
}

/// How many times the serial console's screen was cleared, in its log at
/// `path`.
fn clears(path: &Path) -> usize {
    let log = fs::read(path).unwrap_or_default();
    log.windows(CLEAR.len())
        .filter(|window| *window == CLEAR)
        .count()
}

/// The test, the console's mirror, how keys are pressed on QEMU's keyboard,
/// and its pace.
struct Serial<'t> {
    test: &'t Test,
    mirror: &'t Path,
    key: fn(&Vm<'_>, &str) -> Result<()>,
    pace: Pace,
}

impl Serial<'_> {
    /// Checks the menu, the recovery shell, the sizes and the repaint, on
    /// the serial console.
    fn check(&self, vm: &mut Vm<'_>) -> Result<()> {
        self.check_menu(vm)?;
        self.check_serial_size(vm, "serial")?;
        self.check_display_size(vm)?;
        self.check_repaint(vm)?;
        self.check_serial_size(vm, "again")
    }

    /// Notes the console's last screen when `is_seen` is false.
    fn note_unless(&self, is_seen: bool) {
        if !is_seen {
            self.test
                .note(&format!("the console shows: {}", last_screen(self.mirror)));
        }
    }

    /// The countdown shows on the serial console; Escape typed there opens
    /// the menu, on its screen and the display's, and the menu the
    /// recovery shell.
    fn check_menu(&self, vm: &mut Vm<'_>) -> Result<()> {
        let pause = self.pace.pause;
        let countdown = vm.expect(&[MENU_COUNTDOWN], self.pace.patience).is_ok();
        self.test.check(
            countdown,
            "the countdown shows on the serial console's screen",
        );
        thread::sleep(pause * 2);
        vm.send("\x1b")?;
        let menu = vm.expect(&[prompt::NEW_MENU], 60).is_ok();
        self.test.check(
            menu,
            "Escape typed on the serial console opens the menu, on its screen",
        );
        let panel = prompt::NEW_MENU.trim_start_matches(SCREEN);
        let shown = wait_screen(self.mirror, panel, 30)?;
        self.test
            .check(shown.is_some(), "the display shows the menu too");
        self.note_unless(shown.is_some());
        thread::sleep(pause * 2);
        vm.send("!")?;
        thread::sleep(pause);
        vm.send("\r")?;
        let shell = wait_screen(self.mirror, "emergency shell", 60)?;
        self.test.check(
            shell.is_some(),
            "typed on the serial console, the menu opens the recovery shell",
        );
        self.note_unless(shell.is_some());
        thread::sleep(pause * 3);
        Ok(())
    }

    /// Types `echo zbm-NAME-$(stty size)` on `keyboard`; the size the
    /// command saw, columns and rows, and the display's screen that shows
    /// it.
    fn size(
        &self,
        vm: &mut Vm<'_>,
        name: &str,
        keyboard: Keyboard,
    ) -> Result<Option<((u32, u32), String)>> {
        let command = format!("echo zbm-{name}-$(stty size)\n");
        match keyboard {
            Keyboard::Serial => vm.send(&command.replace('\n', "\r"))?,
            Keyboard::Display => self.type_text(vm, &command)?,
        }
        let pattern = format!(r"(?m)^zbm-{name}-\d+ \d+\s*$");
        let screen = wait_screen(self.mirror, &pattern, 60)?;
        Ok(screen.and_then(|screen| Some((size_in(&screen, name)?, screen))))
    }

    /// Types `text` on QEMU's keyboard, a key each fifth of a pause.
    fn type_text(&self, vm: &Vm<'_>, text: &str) -> Result<()> {
        let keys = key_names(text).ok_or_else(|| Error::Setup(format!("no keys type {text:?}")))?;
        for key in keys {
            (self.key)(vm, &key)?;
            thread::sleep(self.pace.pause / 5);
        }
        Ok(())
    }

    /// Typed on the serial console, a command sees its size, and the
    /// display shows no wider line.
    fn check_serial_size(&self, vm: &mut Vm<'_>, name: &str) -> Result<()> {
        let seen = self.size(vm, name, Keyboard::Serial)?;
        let size = seen.as_ref().map(|(size, _)| *size);
        self.test.check(
            size == Some(SERIAL_SIZE),
            &format!(
                "{name}: typed on the serial console, a command sees its size, 80x25: {size:?}"
            ),
        );
        let width = seen.as_ref().map(|(_, screen)| widest(screen));
        self.test.check(
            width.is_some_and(|width| width <= 80),
            &format!("{name}: the display shows the serial console's screen: {width:?} columns"),
        );
        self.note_unless(seen.is_some());
        Ok(())
    }

    /// Typed on QEMU's keyboard, a command sees the display's size, wider
    /// than the serial console.
    fn check_display_size(&self, vm: &mut Vm<'_>) -> Result<()> {
        let seen = self.size(vm, "display", Keyboard::Display)?;
        let size = seen.as_ref().map(|(size, _)| *size);
        self.test.check(
            size.is_some_and(|(columns, _)| columns > SERIAL_SIZE.0),
            &format!("typed on the keyboard, a command sees the display's size: {size:?}"),
        );
        self.note_unless(seen.is_some());
        Ok(())
    }

    /// Ctrl+L typed on the serial console clears its screen, and writes all
    /// of it again.
    fn check_repaint(&self, vm: &mut Vm<'_>) -> Result<()> {
        let log = vm.serial_log();
        let before = clears(&log);
        vm.send("\x0c")?;
        let start = Instant::now();
        let mut after = clears(&log);
        while after <= before && start.elapsed() < REPAINT_PATIENCE {
            thread::sleep(Duration::from_millis(200));
            after = clears(&log);
        }
        self.test.check(
            after > before,
            "Ctrl+L typed on the serial console writes its whole screen again",
        );
        thread::sleep(self.pace.pause);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
