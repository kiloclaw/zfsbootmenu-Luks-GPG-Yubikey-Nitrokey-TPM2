//! `menu-display`: the tasks of `menu-snapshots`, `menu-pools` and
//! `menu-boot` on the console on the display (`zbm.console=drm`), through
//! their step tables. The image is the console's, shared with
//! `runtime-rust-console*`; keys are pressed on QEMU's keyboard through its
//! monitor, and the screens read from the console's mirror, on the second
//! serial port.
//!
//! Each boot starts on fresh copies of the pristine disks:
//!
//! - `snapshots`: the snapshot tables in turn, then Power Off;
//! - `pools`: the pool table, then Power Off;
//! - `kernel`, with the FAT disk that keeps the boot's logs: the palette's
//!   entries, then the second kernel with the command line edited, which
//!   the boot environment prints on the serial console, and the boot's
//!   record names;
//! - `clone`: the boot of a clone of `five`;
//! - `reboot`: the palette's Reboot, which resets the machine at once
//!   (SysRq-b) and so ends it, as QEMU runs with `-no-reboot`.
//!
//! It opens no recovery shell: the menu runs it on the console's
//! pseudo-terminal, which the test cannot read. Where the serial tests open
//! it between two tables, this one goes back to the Boot tab; what the
//! tasks did to the disks is theirs to check. This one checks what the menu
//! shows after each step: the outcomes, the tabs, the review pages, and
//! what was typed in them.

use std::fs;
use std::time::Duration;

use crate::boot::{Boot, dialogs, follow_be};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::expect::{SCREEN, regex};
use crate::images::{Image, zbm_image};
use crate::qemu::Disk;
use crate::record::Test;
use crate::scenarios::display::{
    Monitor, Pace, last_screen, machine_extra, mirror_path, sendkey, spec,
};
use crate::scenarios::runtime::colours::screendump;
use crate::scenarios::runtime::saved::saved_record;
use crate::vm::Vm;

use super::super::drive::{Display, Driver, Key};
use super::{COUNTDOWN, GROUP, POWER, boot, steps};

/// The command line of the boots: the menu on the display, its mirror on
/// the second serial port, the kernel's messages on the first, a countdown
/// long enough to stop, and `tmenu` alone imported at start-up.
pub(super) const APPEND: &str = "console=ttyS0 console=tty1 loglevel=7 zbm.console=drm \
    zbm.console_serial=off zbm.console_mirror=/dev/ttyS1 zbm.timeout=30 zbm.prefer=tmenu!!";

/// What the kernel says as the menu resets the machine with SysRq-b,
/// which restarts it without the kernel's usual "Restarting system". The
/// kernel may restart before its console prints it: it prints from a
/// thread of its own, which the reset does not wait for.
const RESETTING: &str = "sysrq: Resetting";

/// What the kernel says when the machine ends some other way than SysRq-b:
/// its own reboot or power-off, SysRq-o, or a panic.
const NOT_A_RESET: &str =
    "reboot: Restarting system|reboot: Power down|sysrq: Power Off|Kernel panic";

/// The pace of the display: a key each 200 milliseconds, and the patience
/// of a boot.
const PACE: Pace = Pace {
    pause: Duration::from_secs(1),
    patience: 1200,
};

/// How a boot drives the menu on the display, with what it holds.
pub(super) type Driving<'d> = &'d dyn Fn(&Test, &mut Display<'_>, &mut Vm<'_>) -> Result<()>;

/// Boots five times.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = zbm_image(settings, &spec())?;
    let dir = settings.disks(GROUP);
    fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let logs = vfat_disk(&dir.join("display-logs.img"), boot::LABEL, &[], "64M", 512)?;
    let saving = format!("{APPEND} zbm.save_logs=LABEL={}", boot::LABEL);
    let fat = [Disk::new(logs.clone(), "logs")];
    let monitor = Monitor {
        dump: screendump,
        key: sendkey,
    };
    let on = |label: &str, append: &str, more: &[Disk], drive: Driving<'_>| {
        on_display(test, &image, &monitor, (label, append, more), drive)
    };
    on("snapshots", APPEND, &[], &snapshots)?;
    on("pools", APPEND, &[], &pools)?;
    on("kernel", &saving, &fat, &kernel)?;
    boot::check_record(test, &saved_record(&logs)?);
    on("clone", APPEND, &[], &clone)?;
    on("reboot", APPEND, &[], &reboot)
}

/// Boots `image` with `append` on fresh disks and `more`, as the boot
/// `label`, with the display and its mirror, and drives the menu with
/// `drive`. When that fails, a screenshot is kept (`<label>-failed.ppm`)
/// and the mirror's last screen noted.
pub(super) fn on_display(
    test: &Test,
    image: &Image,
    monitor: &Monitor,
    (label, append, more): (&str, &str, &[Disk]),
    drive: Driving<'_>,
) -> Result<()> {
    let mirror = mirror_path(test, label);
    let mut machine = super::on_fresh(test.settings(), image, append, more)?;
    machine.extra.extend(machine_extra("virtio", &mirror));
    let mut vm = Vm::start(test, label, &machine)?;
    let mut display = Display {
        monitor,
        mirror: &mirror,
        pace: PACE,
    };
    let driven = drive(test, &mut display, &mut vm);
    if driven.is_err() {
        let shot = test.dir().join(format!("{label}-failed.ppm"));
        if let Err(error) = (monitor.dump)(&vm, &shot) {
            test.note(&format!("{label}: no screenshot: {error}"));
        }
        test.note(&format!(
            "{label}: the console shows: {}",
            last_screen(&mirror)
        ));
    }
    vm.kill();
    driven
}

/// Waits for the menu on the display: answers start-up's dialog of the
/// LUKS volume empty when it shows, which leaves the volume to the menu,
/// then stops the countdown, and checks that the menu is the workspace.
/// The group has one volume, so that the dialog shows once at most. Each
/// wait has the boot's patience once: when neither shows, the start fails.
pub(super) fn start(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>) -> Result<()> {
    let luks = dialogs::LUKS.trim_start_matches(SCREEN);
    let countdown = COUNTDOWN.trim_start_matches(SCREEN);
    let either = format!("(?:{luks})|(?:{countdown})");
    let patience = display.pace.patience;
    let mut shown = display.screen(vm, &either, patience)?;
    let asks = shown
        .as_ref()
        .is_some_and(|screen| regex(luks).is_ok_and(|luks| luks.is_match(screen)));
    if asks {
        test.step("start-up asks for the passphrase of the LUKS volume -> skipped");
        display.press(vm, Key::Enter)?;
        shown = display.screen(vm, countdown, patience)?;
    }
    if shown.is_none() {
        test.check(false, "the countdown shows on the display");
        return Err(Error::Setup("the countdown did not show".into()));
    }
    let pause = display.pace.pause;
    super::stop_countdown(test, display, vm, pause)
}

/// The tables of `menu-snapshots`, each from the Boot tab, as after the
/// serial test's shell; then Power Off.
fn snapshots(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>) -> Result<()> {
    start(test, display, vm)?;
    steps::run(test, display, vm, steps::SNAPSHOTS_CHANGES)?;
    steps::run(test, display, vm, steps::BOOT_TAB)?;
    steps::run(test, display, vm, steps::SNAPSHOTS_DESTROY)?;
    steps::run(test, display, vm, steps::BOOT_TAB)?;
    steps::run(test, display, vm, steps::SNAPSHOTS_REFUSED)?;
    super::power_off(test, display, vm)
}

/// The table of `menu-pools`, then Power Off.
fn pools(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>) -> Result<()> {
    start(test, display, vm)?;
    steps::run(test, display, vm, steps::POOLS)?;
    super::power_off(test, display, vm)
}

/// The palette's entries, then the second kernel with the command line
/// edited, which the boot environment holds.
fn kernel(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>) -> Result<()> {
    start(test, display, vm)?;
    steps::run(test, display, vm, steps::BOOT_TOUR)?;
    steps::run(test, display, vm, steps::BOOT_KERNEL)?;
    boot::check_edited(test, &be_report(test, vm)?);
    Ok(())
}

/// The boot of a clone of `five`, which is the boot environment's root.
fn clone(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>) -> Result<()> {
    start(test, display, vm)?;
    steps::run(test, display, vm, steps::BOOT_CLONE)?;
    boot::check_clone(test, &be_report(test, vm)?);
    Ok(())
}

/// The palette's Reboot, which resets the machine: QEMU ends, after the
/// kernel says so, or with nothing said, where the reset came before the
/// console printed it; but not after the kernel said that it ended the
/// machine another way ([`NOT_A_RESET`]).
fn reboot(test: &Test, display: &mut Display<'_>, vm: &mut Vm<'_>) -> Result<()> {
    start(test, display, vm)?;
    steps::run(test, display, vm, steps::REBOOT)?;
    let ended = match vm.expect(&[RESETTING, NOT_A_RESET], POWER) {
        Ok(0) => vm.expect_eof(POWER).is_ok(),
        Err(Error::Eof { .. }) => {
            test.note("the machine ended with no line of the kernel: a reset by SysRq-b");
            true
        }
        Ok(_) | Err(_) => false,
    };
    test.check(ended, "the palette's Reboot resets the machine");
    if !ended {
        vm.kill();
    }
    Ok(())
}

/// The report of the boot environment the menu booted, on the serial
/// console, once it is done; the machine then powers off. A panic, an
/// emergency shell or a prompt of the boot environment ends the wait at
/// once, and is noted; the report is then empty.
fn be_report(test: &Test, vm: &mut Vm<'_>) -> Result<Boot> {
    let mut report = Boot::default();
    follow_be(vm, &mut report, boot::BE_PATIENCE)?;
    for event in &report.prompts {
        test.note(&format!("the boot environment stopped: {event}"));
    }
    Ok(report)
}

#[cfg(test)]
pub(super) mod tests;
