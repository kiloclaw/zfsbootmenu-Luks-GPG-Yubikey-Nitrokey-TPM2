//! The checks of `runtime-rust-console` that end with the console: Ctrl-C
//! stops a program that floods it, and once it is killed, the supervisor
//! starts it again (ROADMAP.md, PM6), with the recovery shell on it, which
//! takes keys and finds the supervisor's messages in the kernel log.

use std::fs;
use std::path::Path;
use std::thread;

use crate::error::Result;
use crate::record::Test;
use crate::vm::Vm;

use super::super::colours::screen_colours;
use super::super::drm::Monitor;
use super::{Pace, last_screen, type_text, wait_screen};

/// Typed in the recovery shell: ends the console.
pub(super) const KILL: &str = "kill -9 $(cat /zfsbootmenu/console)\n";

/// What the supervisor shows before the recovery shell that follows the
/// start-up program, which ended with the console's terminal.
pub(super) const SHELL_AGAIN: &str = "type 'exit' to start it again";

/// Typed in that shell: a line on the serial console, which the test reads,
/// made by the shell, so that keys that reached something else (the menu's
/// command line, which a kernel prints) fail; it names the shell's
/// terminal, the pseudo-terminal of the console that started again.
pub(super) const BACK: &str = "echo zbm-back-$(readlink /proc/self/fd/0) > /dev/ttyS0\n";

/// What [`BACK`] prints from a shell on the console.
pub(super) const BACK_LINE: &str = r"zbm-back-/dev/pts/\d+\b";

/// Typed in the shell then: the supervisor's lines of the kernel log about
/// the console, on the serial console. The whole log can take longer than
/// the test waits to cross the serial line.
pub(super) const LOG: &str = "dmesg | grep on.the.display > /dev/ttyS0\n";

/// Typed in the recovery shell: floods the console, until Ctrl-C, with a
/// program the image has (it has no `yes`).
pub(super) const FLOOD: &str = "od -v /dev/zero\n";

/// Typed after Ctrl-C: a line on the serial console, which the test reads.
pub(super) const FLOOD_DONE: &str = "echo zbm-flood-ok > /dev/ttyS0\n";

/// Typed before the flood: the terminal's modes and the signals a program
/// started from the shell inherits ignored or blocked, on the serial
/// console, for the notes when Ctrl-C does not stop the flood.
pub(super) const STATE: [&str; 3] = [
    "stty -a > /dev/ttyS0\n",
    "grep Sig /proc/self/status > /dev/ttyS0\n",
    "echo zbm-state-end > /dev/ttyS0\n",
];

/// Typed in the shell last: the console's line of the kernel log that says
/// the menu said hello on the client channel (ROADMAP.md, UI6).
pub(super) const CELLS_LOG: &str = "dmesg | grep draws.on.its.cells > /dev/ttyS0\n";

/// That line: the menu drew on the console's cells, not on its terminal
/// (`zbm_console::run::GREETED`).
pub(super) const CELLS: &str = r"the menu draws on its cells \(client channel version \d+\)";

/// Typed in the recovery shell: what the console gave up (ROADMAP.md,
/// SE11), to the serial console; then the calls its filter refused, which
/// the kernel logs as audit records of type 1326, and how often a console
/// said it was confined. In the characters the test can type: `.` stands
/// for `=` and `: `.
pub(super) const STATUS_LOG: &str = "cat /proc/$(cat /zfsbootmenu/console)/status > /dev/ttyS0\n";
pub(super) const COUNTS_LOG: &str = "echo zbmtest-denied $(dmesg | grep -c type.1326) \
    zbmtest-confined $(dmesg | grep -c console..confined) > /dev/ttyS0\n";

/// The lines of the console's status, in their order, and of the counts.
pub(super) const CAPABILITIES: &str = r"CapEff:\s+([0-9a-f]+)";
pub(super) const NO_NEW_PRIVILEGES: &str = r"NoNewPrivs:\s+(\d+)";
pub(super) const SECCOMP: &str = r"Seccomp:\s+(\d+)";
pub(super) const COUNTS: &str = r"zbmtest-denied (\d+) zbmtest-confined (\d+)";

/// The capabilities the console keeps: `CAP_IPC_LOCK` and
/// `CAP_SYS_TTY_CONFIG`.
pub(super) const CONSOLE_CAPABILITIES: &str = "0000000004004000";

/// The supervisor's message when the console ended.
pub(super) const ENDED: &str = r"console on the display ended";

/// The supervisor's message when it started the console again.
pub(super) const RESTARTED: &str = r"console on the display started again";

/// Ctrl-C typed while a program floods the console stops it: the keys come
/// before the program's output.
pub(super) fn check_flood(
    test: &Test,
    vm: &mut Vm<'_>,
    monitor: &Monitor,
    pace: Pace,
) -> Result<()> {
    note_state(test, vm, monitor, pace)?;
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
    if !stopped {
        // Ctrl-\ sends SIGQUIT: whether it stops od tells a signal that
        // is ignored from a terminal that sends none.
        (monitor.key)(vm, "ctrl-backslash")?;
        thread::sleep(pace.pause * 2);
        type_text(vm, monitor, FLOOD_DONE, pace.pause)?;
        let quit = vm.expect(&["zbm-flood-ok"], pace.patience.min(60)).is_ok();
        test.note(&format!("virtio: after Ctrl-\\, the flood stopped: {quit}"));
    }
    Ok(())
}

/// Notes the shell's terminal modes and the signals its programs inherit
/// ignored or blocked.
fn note_state(test: &Test, vm: &mut Vm<'_>, monitor: &Monitor, pace: Pace) -> Result<()> {
    for line in STATE {
        type_text(vm, monitor, line, pace.pause)?;
    }
    if vm.expect(&["zbm-state-end"], pace.patience.min(30)).is_ok() {
        let state: Vec<String> = vm
            .before()
            .lines()
            .map(str::trim)
            .filter(|line| line.contains("isig") || line.starts_with("Sig"))
            .map(str::to_owned)
            .collect();
        test.note(&format!("virtio: before the flood: {}", state.join(" | ")));
    }
    Ok(())
}

/// The console killed from its shell: it starts again, the recovery shell
/// on it takes keys, and the supervisor's messages are in the kernel log.
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
    let again = wait_screen(mirror, SHELL_AGAIN, pace.patience.min(60))?.is_some();
    let back = again && {
        type_text(vm, monitor, BACK, pace.pause)?;
        vm.expect(&[BACK_LINE], pace.patience.min(30)).is_ok()
    };
    test.check(
        back,
        "virtio: once the console ended, it starts again, and the recovery shell on it takes keys",
    );
    type_text(vm, monitor, LOG, pace.pause)?;
    let logged = back
        && vm.expect(&[ENDED], pace.patience.min(30)).is_ok()
        && vm.expect(&[RESTARTED], pace.patience.min(30)).is_ok();
    test.check(
        logged,
        "virtio: the supervisor logged that the console ended and started again",
    );
    type_text(vm, monitor, CELLS_LOG, pace.pause)?;
    let drew = back && vm.expect(&[CELLS], pace.patience.min(30)).is_ok();
    test.check(
        drew,
        "virtio: the menu drew on the console's cells, over the client channel",
    );
    check_confined(test, vm, monitor, pace)
}

/// The console, started again, gave up what its profile says, and its
/// filter refused none of the calls it made since the boot: the hand-back
/// and the reattach, the layouts and the fonts ran confined.
fn check_confined(test: &Test, vm: &mut Vm<'_>, monitor: &Monitor, pace: Pace) -> Result<()> {
    let wait = pace.patience.min(30);
    let read = |vm: &mut Vm<'_>, pattern: &str| {
        vm.expect(&[pattern], wait)
            .map(|_| vm.group(1))
            .unwrap_or_default()
    };
    type_text(vm, monitor, STATUS_LOG, pace.pause)?;
    let capabilities = read(vm, CAPABILITIES);
    let privileges = read(vm, NO_NEW_PRIVILEGES);
    let seccomp = read(vm, SECCOMP);
    test.check(
        privileges == "1" && seccomp == "2" && capabilities == CONSOLE_CAPABILITIES,
        &format!(
            "virtio: the console is confined: no new privileges, a seccomp filter, two \
             capabilities (NoNewPrivs {privileges:?}, Seccomp {seccomp:?}, CapEff \
             {capabilities:?})"
        ),
    );
    type_text(vm, monitor, COUNTS_LOG, pace.pause)?;
    let counted = vm.expect(&[COUNTS], wait).is_ok();
    let (denied, confined) = if counted {
        (vm.group(1), vm.group(2))
    } else {
        (String::new(), String::new())
    };
    test.check(
        denied == "0" && confined.parse::<u32>().is_ok_and(|count| count >= 2),
        &format!(
            "virtio: each console said it was confined, and its filter refused no call it \
             made ({confined:?} confined, {denied:?} refused)"
        ),
    );
    Ok(())
}
