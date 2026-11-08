//! The menu tests (ADR 0016, point 16): the core tasks of the menu, each
//! driven through its keys on the serial console and on the display, on
//! pools of the group's own, as the steps of `steps` say.
//!
//! - `setup` (`setup`): on the legacy image, as `runtime-setup` does, the
//!   pools of the group, whose disks it keeps as pristine copies:
//!   - `tmenu` (d0): the test boot environment with a second kernel, files
//!     to restore, and snapshots with a clone, a hold and a bookmark;
//!   - `tother` (d1): a checkpoint, then a filesystem made after it;
//!   - `tforeign` (d2): left imported by the host `deadbeef`;
//!   - `tlocked` (d3): in a LUKS2 volume, with the passphrase `pw`.
//! - `snapshots` (`snapshots`): each change of a snapshot, a diff and a
//!   restore, destroy, and what ZFS would refuse;
//! - `pools` (`pools`): an import of each kind, a rewind, an export and the
//!   unlock of a volume;
//! - `boot` (`boot`): the palette's entries, a boot with another kernel and
//!   an edited command line, and the boot of a clone;
//! - `display` (`display`): the tasks of the three, on the console on the
//!   display, with no recovery shell;
//! - `keys` (`keys`): keys pressed by their place on six keyboard layouts,
//!   on the console on the display, and the views they bring.
//!
//! Each test boots on fresh copies of the pristine disks, with
//! `zbm.prefer=tmenu!!`, which leaves the other pools to the menu: one that
//! fails costs itself, and each can run again alone after `menu-setup`,
//! within the month (UTC) the setup ran: the timeline folds the snapshots
//! of an earlier month under its header, so that a test asks for the setup
//! again then. What the tasks did is checked on the serial console, in the
//! recovery shell (`checks`), which the palette opens.

use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::boot::{dialogs, prompt};
use crate::error::{Error, Result};
use crate::images::Image;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::{Settings, ZbmRuntime};
use crate::vm::Vm;

use super::display::Pace;
use super::drive::{Driver, Key, Serial};

mod boot;
mod checks;
mod display;
mod keys;
mod pools;
mod setup;
mod snapshots;
mod steps;

/// The group of the tests, which names their disks and their lock.
const GROUP: &str = "menu";

/// How many disks the group has: d0 to d3.
const DISKS: usize = 4;

/// The command line of the tests: the menu on the serial console, a
/// countdown long enough to stop, and `tmenu` alone imported at start-up.
const APPEND: &str = "console=ttyS0 loglevel=7 zbm.timeout=30 zbm.prefer=tmenu!!";

/// The countdown of `tmenu`'s boot environment, on the screen.
const COUNTDOWN: &str = r"screen:Booting \S*tmenu/ROOT/be\S* in";

/// The workspace's tab bar with its Boot tab current.
const TABS: &str = r"\[1 Boot\]";

/// What the kernel says as the machine powers off.
const POWERED_OFF: &str = "reboot: Power down";

/// How long the menu may take to show again after the shell, in seconds.
const MENU: u64 = 600;

/// How long the machine may take to power off, in seconds.
const POWER: u64 = 180;

/// The pace of the serial console: the pause after Escape, the patience
/// of a boot.
const SERIAL: Pace = Pace {
    pause: Duration::from_secs(1),
    patience: 1200,
};

/// The UUID of the LUKS volume on d3, fixed so that its mapping's name is
/// known: `luks-<uuid>`.
macro_rules! uuid {
    () => {
        "0e8a5e1d-7c3b-4f2a-9d6e-1b2c3d4e5f60"
    };
}
use uuid;

/// The UUID of the LUKS volume.
const UUID: &str = uuid!();

/// The name of the LUKS volume's mapping, as the menu lists it.
const VOLUME: &str = concat!("luks-", uuid!());

/// Runs `setup`, `snapshots`, `pools`, `boot`, `display` or `keys`.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub fn run(test: &Test, scenario: &str) -> Result<()> {
    if scenario == "setup" {
        return setup::run(test);
    }
    if let Some(why) = missing_setup(test.settings(), SystemTime::now()) {
        test.check(false, why);
        return Ok(());
    }
    match scenario {
        "snapshots" => snapshots::run(test),
        "pools" => pools::run(test),
        "boot" => boot::run(test),
        "display" => display::run(test),
        "keys" => keys::run(test),
        _ => {
            test.check(false, &format!("unknown scenario {scenario}"));
            Ok(())
        }
    }
}

/// The pristine copy of the disk `d<index>`, which `menu-setup` keeps.
fn pristine(settings: &Settings, index: usize) -> PathBuf {
    settings.disks(GROUP).join(format!("pristine-{index}.img"))
}

/// Why a test cannot run on the pristine disks at `now`, if it cannot:
/// `menu-setup` did not keep them all, or kept them in an earlier month
/// (UTC, as the menu counts), whose snapshots the timeline folds under the
/// month's header, where no row of them shows.
fn missing_setup(settings: &Settings, now: SystemTime) -> Option<&'static str> {
    let kept: Option<Vec<SystemTime>> = (0..DISKS)
        .map(|index| {
            fs::metadata(pristine(settings, index))
                .and_then(|meta| meta.modified())
                .ok()
        })
        .collect();
    let Some(kept) = kept else {
        return Some("run menu-setup first");
    };
    if kept.iter().any(|&time| month(time) != month(now)) {
        return Some("run menu-setup again: its snapshots are of an earlier month");
    }
    None
}

/// The year and the month, from 1, of `time` in UTC.
fn month(time: SystemTime) -> (i64, i64) {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let days = i64::try_from(seconds / 86_400).unwrap_or_default();
    // The civil date of a day number (H. Hinnant's days_from_civil, read
    // back), in years that begin on 1 March, of eras of 400 years.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year + year / 4 - year / 100);
    let from_march = (5 * of_year + 2) / 153;
    let month = if from_march < 10 {
        from_march + 3
    } else {
        from_march - 9
    };
    (era * 400 + year + i64::from(month <= 2), month)
}

/// The disks d0 to d3, fresh copies of the pristine ones.
fn fresh_disks(settings: &Settings) -> Result<Vec<Disk>> {
    (0..DISKS)
        .map(|index| {
            let path = settings.disks(GROUP).join(format!("disk{index}.img"));
            crate::disks::copy_sparse(&pristine(settings, index), &path)?;
            Ok(Disk::new(path, format!("d{index}")))
        })
        .collect()
}

/// Boots the Rust image on fresh copies of the disks, and `more`, with
/// `append`; `label` names the boot.
fn boot_on_fresh<'t>(test: &'t Test, label: &str, append: &str, more: &[Disk]) -> Result<Vm<'t>> {
    let settings = test.settings();
    let image = super::runtime::image(settings, "auto", ZbmRuntime::Rust, None)?;
    let machine = on_fresh(settings, &image, append, more)?;
    Vm::start(test, label, &machine)
}

/// The machine that boots `image` with `append`, on fresh copies of the
/// disks and `more`.
fn on_fresh(settings: &Settings, image: &Image, append: &str, more: &[Disk]) -> Result<Machine> {
    let mut disks = fresh_disks(settings)?;
    disks.extend(more.iter().cloned());
    Ok(Machine::boot(image, append).with_disks(disks))
}

/// Waits for the menu: answers start-up's dialog of the LUKS volume empty,
/// when it shows, which leaves the volume to the menu, then stops the
/// countdown, and checks that the menu is the workspace.
fn start(test: &Test, serial: &mut Serial, vm: &mut Vm<'_>) -> Result<()> {
    while vm.expect(&[dialogs::LUKS, COUNTDOWN], serial.pace.patience)? == 0 {
        test.step("start-up asks for the passphrase of the LUKS volume -> skipped");
        dialogs::reply(vm, "")?;
    }
    let pause = serial.pace.pause;
    stop_countdown(test, serial, vm, pause)
}

/// Stops the countdown, after `pause`, and checks that the menu is the
/// workspace.
fn stop_countdown(
    test: &Test,
    driver: &mut dyn Driver,
    vm: &mut Vm<'_>,
    pause: Duration,
) -> Result<()> {
    thread::sleep(pause);
    driver.press(vm, Key::Escape)?;
    let tabs = driver.screen(vm, TABS, MENU)?;
    test.check(
        tabs.is_some(),
        "the countdown stops, and the menu is the workspace: its tab bar shows the Boot tab",
    );
    match tabs {
        Some(_) => Ok(()),
        None => Err(Error::Setup("the menu did not show".into())),
    }
}

/// Opens the recovery shell from the palette, and prepares it.
fn shell(test: &Test, serial: &mut Serial, vm: &mut Vm<'_>) -> Result<()> {
    steps::run(test, serial, vm, steps::SHELL)?;
    vm.shell_ready()
}

/// Ends the recovery shell: the menu shows again, and Escape stops its
/// countdown if it counts down again.
fn leave_shell(test: &Test, serial: &mut Serial, vm: &mut Vm<'_>) -> Result<()> {
    test.step("exit: the menu again");
    vm.sendline("exit")?;
    vm.expect1(prompt::NEW_MENU, MENU)?;
    serial.press(vm, Key::Escape)
}

/// Powers the machine off from the palette.
fn power_off(test: &Test, driver: &mut dyn Driver, vm: &mut Vm<'_>) -> Result<()> {
    steps::run(test, driver, vm, steps::POWER_OFF)?;
    let said = vm.expect1(POWERED_OFF, POWER).is_ok();
    test.check(said, "the palette's Power Off powers the machine off");
    if vm.expect_eof(POWER).is_err() {
        vm.kill();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
