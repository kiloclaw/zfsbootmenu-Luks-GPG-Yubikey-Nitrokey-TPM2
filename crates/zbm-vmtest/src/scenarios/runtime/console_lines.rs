//! `runtime-rust-console-lines`: the menu as lines, and the console's text
//! for a Braille display (ROADMAP.md UI8, ADR 0011). Four boots of the
//! image of `runtime-rust-console`, with `tplain` (d0) and `tforeign` (d1),
//! the pool of another host, which `zbm.import_policy=force` imports beside
//! `tplain`:
//!
//! - `TERM=dumb` on the serial console: the menu, the workspace, prints its
//!   boot environments as a numbered list, and the number of `tforeign`'s,
//!   typed as a line, boots it.
//! - `zbm.menu=lines`, `TERM` as the kernel sets it: the same.
//! - `zbm.menu=classic` and `TERM=dumb`: the classic menu shows as lines by
//!   itself too (ADR 0016, point 10), and the same number boots `tforeign`.
//! - `zbm.console=drm`, the screen as usual: in the recovery shell, the
//!   text of `/dev/vcs1`, which the console writes through `/dev/vcsa1`,
//!   holds what the shell printed; the kernel draws none of it.

use std::thread;
use std::time::Duration;

use crate::boot::{Follow, follow_boot, prompt};
use crate::error::Result;
use crate::expect::SCREEN;
use crate::images::zbm_image;
use crate::qemu::Machine;
use crate::record::Test;
use crate::vm::Vm;

use super::console::{last_screen, machine_extra, wait_screen};

/// The command line of the boots of the menu as lines, before the boot's
/// own options: `tforeign` belongs to another host, and the default import
/// policy leaves it out once `tplain` is in.
const APPEND: &str = "console=ttyS0 loglevel=7 zbm.timeout=60 zbm.import_policy=force";

/// The boots of the menu as lines: their names and their options.
const LINE_BOOTS: [(&str, &str); 3] = [
    ("dumb", " TERM=dumb"),
    ("lines", " zbm.menu=lines"),
    ("classic", " zbm.menu=classic TERM=dumb"),
];

/// The command line of the boot on the console on the display, which
/// serves the serial console too.
const DRM_APPEND: &str = "console=tty1 console=ttyS0 loglevel=7 zbm.console=drm \
    zbm.console_mirror=/dev/ttyS1 zbm.timeout=60";

/// The line of the list that names `tforeign`'s boot environment, its
/// number in group 1.
const FOREIGN_ITEM: &str = r"(?m)^ +(\d+)\) tforeign/ROOT/be\b";

/// What the line view prints under a list.
const NUMBER_HINT: &str = "Type a number to choose it";

/// What the shell types: a line that prints `ZBMTEST-VCS-2-yes` when the
/// console's text holds `zbm-42-vcs`, which only the output of `echo`
/// spells, and `ZBMTEST-VCS-3-no` when it does not. Neither marker is in
/// the line itself, which the screen shows as it is typed.
const VCS_COMMAND: &str = "echo zbm-$((6*7))-vcs; sleep 2; \
    if grep -q zbm-42-vcs /dev/vcs1; then echo ZBMTEST-VCS-$((1+1))-yes; \
    else echo ZBMTEST-VCS-$((1+2))-no; fi\r";

/// What the shell types when `/dev/vcs1` lacks the text: what the console
/// logged about the text device, and the start of the terminal's text, for
/// the serial log.
const VCS_DIAGNOSIS: &str = "dmesg 2>&1 | grep -i -e vcsa -e 'screen readers' | tail -5; \
    head -c 480 /dev/vcs1 | od -A d -c | head -8\r";

/// What the shell prints after [`VCS_COMMAND`].
const VCS_RESULT: &str = r"ZBMTEST-VCS-(2-yes|3-no)";

/// How long a step may take, in seconds.
const PATIENCE: u64 = 1200;

/// Runs the three boots.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = zbm_image(settings, &super::console::spec())?;
    for (name, extra) in LINE_BOOTS {
        test.step(&format!("{name}: the menu as lines"));
        let disks = [super::disk(settings, 0)?, super::disk(settings, 1)?];
        let machine = Machine::boot(&image, &format!("{APPEND}{extra}")).with_disks(disks);
        let mut vm = Vm::start(test, name, &machine)?;
        if choose_foreign(&mut vm, name, Duration::from_secs(2))? {
            // Up to ZFSBootMenu's teardown: the boot environment's own
            // initramfs leaves the pool of another host out, as after
            // `runtime-rust-force`.
            let follow = Follow {
                timeout: Some(PATIENCE),
                no_be: true,
                ..Follow::default()
            };
            follow_boot(&mut vm, follow)?;
            let booted = booted(&mut vm);
            check_booted(test, name, &booted);
        }
        vm.kill();
    }
    test.step("drm: the console's text in /dev/vcsa1");
    let mirror = test.dir().join("mirror-vcsa.txt");
    let mut machine = Machine::boot(&image, DRM_APPEND).with_disks([super::disk(settings, 0)?]);
    machine.extra.extend(machine_extra("virtio", &mirror));
    let mut vm = Vm::start(test, "virtio", &machine)?;
    check_vcsa(&mut vm, &mirror, Duration::from_secs(1))?;
    vm.kill();
    Ok(())
}

/// Waits for the list of the line view and types the number of
/// `tforeign`'s boot environment; whether it could.
fn choose_foreign(vm: &mut Vm<'_>, name: &str, pause: Duration) -> Result<bool> {
    let test = vm.test();
    let listed = vm.expect(&[FOREIGN_ITEM], PATIENCE).is_ok();
    test.check(
        listed,
        &format!("{name}: the menu prints the boot environments as a numbered list"),
    );
    if !listed {
        return Ok(false);
    }
    let number = vm.group(1);
    let hinted = vm.expect(&[NUMBER_HINT], 60).is_ok();
    test.check(
        hinted,
        &format!("{name}: the line view says what can be typed"),
    );
    thread::sleep(pause);
    test.step(&format!("{name}: typing {number}, tforeign's number"));
    vm.send(&format!("{number}\r"))?;
    Ok(true)
}

/// What ZFSBootMenu says it boots, after its teardown: the boot
/// environment in group 1.
const BOOTED: &str = r"Booting \S+ for (\S+)";

/// The boot environment ZFSBootMenu says it boots; empty when it says
/// none within a minute.
fn booted(vm: &mut Vm<'_>) -> String {
    match vm.expect(&[BOOTED], 60) {
        Ok(_) => vm.group(1),
        Err(_) => String::new(),
    }
}

/// ZFSBootMenu booted `booted`, the boot environment of `tforeign`.
fn check_booted(test: &Test, name: &str, booted: &str) {
    test.check(
        booted.starts_with("tforeign/"),
        &format!("{name}: the number typed boots tforeign's boot environment ({booted})"),
    );
}

/// The countdown escaped, the menu opens the recovery shell, whose output
/// the console writes into `/dev/vcsa1`.
fn check_vcsa(vm: &mut Vm<'_>, mirror: &std::path::Path, pause: Duration) -> Result<()> {
    let test = vm.test();
    let countdown = vm.expect(&[super::MENU_COUNTDOWN], PATIENCE).is_ok();
    test.check(countdown, "drm: the countdown shows");
    thread::sleep(pause * 2);
    vm.send("\x1b")?;
    let menu = vm.expect(&[prompt::NEW_MENU], 60).is_ok();
    test.check(menu, "drm: Escape opens the menu");
    thread::sleep(pause * 2);
    vm.send("!")?;
    thread::sleep(pause);
    vm.send("\r")?;
    let shell = wait_screen(mirror, "emergency shell", 60)?;
    test.check(shell.is_some(), "drm: the menu opens the recovery shell");
    thread::sleep(pause * 3);
    vm.send(VCS_COMMAND)?;
    let result = vm.expect(&[&format!("{SCREEN}{VCS_RESULT}")], 60).is_ok();
    let found = result.then(|| vm.group(1));
    test.check(
        found.as_deref() == Some("2-yes"),
        &format!("drm: /dev/vcs1 holds the text the shell printed ({found:?})"),
    );
    if found.is_none() {
        test.note(&format!("the console shows: {}", last_screen(mirror)));
    }
    if found.as_deref() != Some("2-yes") {
        vm.send(VCS_DIAGNOSIS)?;
        thread::sleep(pause * 3);
        test.note(&format!("after the diagnosis: {}", last_screen(mirror)));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
