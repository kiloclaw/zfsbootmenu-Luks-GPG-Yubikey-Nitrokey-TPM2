//! The checks of `runtime-rust-console` that end with the console: Ctrl-C
//! stops a program that floods it, and once it is killed, the runtime goes
//! on on the kernel console, whose shell takes keys and finds the
//! supervisor's message in the kernel log.

use std::fs;
use std::path::Path;
use std::thread;

use crate::error::Result;
use crate::record::Test;
use crate::vm::Vm;

use super::super::colours::screen_colours;
use super::super::drm::Monitor;
use super::{Pace, last_screen, type_text};

/// Typed in the recovery shell: ends the console.
pub(super) const KILL: &str = "kill -9 $(cat /zfsbootmenu/console)\n";

/// Typed on the kernel console afterwards: a line on the serial console,
/// which the test reads, made by the shell, so that keys that reached
/// something else (the menu's command line, which a kernel prints) fail;
/// it names the shell's terminal, which must not be the console's
/// pseudo-terminal.
pub(super) const BACK: &str = "echo zbm-back-$(readlink /proc/self/fd/0) > /dev/ttyS0\n";

/// What [`BACK`] prints from a shell on the kernel console.
pub(super) const BACK_LINE: &str = r"zbm-back-/dev/(tty\d+|console)\b";

/// Typed on the kernel console then: the supervisor's line of the kernel
/// log, on the serial console. The whole log can take longer than the test
/// waits to cross the serial line.
pub(super) const LOG: &str = "dmesg | grep display.ended > /dev/ttyS0\n";

/// Typed in the recovery shell: floods the console, until Ctrl-C, with a
/// program the image has (it has no `yes`).
pub(super) const FLOOD: &str = "od -v /dev/zero\n";

/// Typed after Ctrl-C: a line on the serial console, which the test reads.
pub(super) const FLOOD_DONE: &str = "echo zbm-flood-ok > /dev/ttyS0\n";

/// The supervisor's message when the console ended.
pub(super) const ENDED: &str = r"console on the display ended";

/// Ctrl-C typed while a program floods the console stops it: the keys come
/// before the program's output.
pub(super) fn check_flood(
    test: &Test,
    vm: &mut Vm<'_>,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    type_text(vm, monitor, FLOOD, pace.pause)?;
    thread::sleep(pace.pause * 3);
    (monitor.key)(vm, "ctrl-c")?;
    thread::sleep(pace.pause * 2);
    type_text(vm, monitor, FLOOD_DONE, pace.pause)?;
    let stopped = vm.expect(&["zbm-flood-ok"], pace.patience.min(60)).is_ok();
    test.check(
        stopped,
        "virtio: Ctrl-C typed while od floods the console stops it",
    );
    Ok(())
}

/// The console killed from its shell: the kernel console takes keys again,
/// and the supervisor's message is in the kernel log.
pub(super) fn check_fallback(
    test: &Test,
    vm: &mut Vm<'_>,
    mirror: &Path,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    type_text(vm, monitor, KILL, pace.pause)?;
    thread::sleep(pace.pause * 5);
    let path = test.dir().join("virtio-fallback.ppm");
    (monitor.dump)(vm, &path)?;
    let colours = fs::read(&path).ok().and_then(|ppm| screen_colours(&ppm));
    let screen = last_screen(mirror);
    test.note(&format!(
        "virtio: after the kill, {colours:?} colours on the display; the console showed: {screen}"
    ));
    type_text(vm, monitor, BACK, pace.pause)?;
    let back = vm.expect(&[BACK_LINE], pace.patience.min(30)).is_ok();
    test.check(
        back,
        "virtio: once the console ended, a shell on the kernel console takes keys",
    );
    type_text(vm, monitor, LOG, pace.pause)?;
    let ended = back && vm.expect(&[ENDED], pace.patience.min(30)).is_ok();
    test.check(
        ended,
        "virtio: the supervisor logged that the console ended",
    );
    Ok(())
}
