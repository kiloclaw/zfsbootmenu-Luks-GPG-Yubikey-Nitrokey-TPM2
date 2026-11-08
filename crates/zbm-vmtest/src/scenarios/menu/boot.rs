//! `menu-boot`: the palette's entries, and the boots the menu starts.
//!
//! - The first boot, with `zbm.save_logs` naming a FAT disk, goes through
//!   the palette's entries, checks those it does not offer here, then boots
//!   the second kernel, `0.0.1-zbmtest`, with a command line edited for
//!   this boot. The boot environment prints its command line, and the
//!   boot's record on the FAT disk names the kernel: both copies of the
//!   kernel are the same build, so it prints the same release.
//! - The second, on fresh disks, boots a clone of `five` from the palette:
//!   the boot environment's root is the clone.
//! - The third powers the machine off from the palette.

use crate::boot::{Boot, Follow, follow_boot};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::qemu::Disk;
use crate::record::Test;
use crate::vm::Vm;

use super::super::drive::Serial;
use super::super::runtime::saved::saved_record;
use super::{APPEND, GROUP, SERIAL, steps};

/// The label of the FAT disk that keeps the boots' logs.
pub(super) const LABEL: &str = "ZBMMENU";

/// What the first boot edits on the command line.
const EDITED: &str = "zbmtest.edited=1";

/// The kernel the first boot boots, in its boot environment.
const KERNEL: &str = "/boot/vmlinuz-0.0.1-zbmtest";

/// The clone the second boot boots.
const CLONE: &str = "tmenu/ROOT/booted";

/// How long a boot environment may take to report, in seconds.
pub(super) const BE_PATIENCE: u64 = 1200;

/// Boots three times.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let dir = settings.disks(GROUP);
    std::fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let logs = vfat_disk(&dir.join("logs.img"), LABEL, &[], "64M", 512)?;
    let saving = format!("{APPEND} zbm.save_logs=LABEL={LABEL}");
    let fat = [Disk::new(logs.clone(), "logs")];
    let mut serial = Serial { pace: SERIAL };

    let mut vm = super::boot_on_fresh(test, "kernel", &saving, &fat)?;
    let boot = first(test, &mut vm, &mut serial)?;
    drop(vm);
    check_edited(test, &boot);
    check_record(test, &saved_record(&logs)?);

    let mut vm = super::boot_on_fresh(test, "clone", APPEND, &[])?;
    let boot = second(test, &mut vm, &mut serial)?;
    drop(vm);
    check_clone(test, &boot);

    let mut vm = super::boot_on_fresh(test, "power", APPEND, &[])?;
    super::start(test, &mut serial, &mut vm)?;
    super::power_off(test, &mut serial, &mut vm)
}

/// Follows a boot the menu started into the boot environment.
fn follow(vm: &mut Vm<'_>) -> Result<Boot> {
    let follow = Follow {
        timeout: Some(BE_PATIENCE),
        ..Follow::default()
    };
    follow_boot(vm, follow)
}

/// The first boot: the palette's entries, then the second kernel with the
/// command line edited.
fn first(test: &Test, vm: &mut Vm<'_>, serial: &mut Serial) -> Result<Boot> {
    super::start(test, serial, vm)?;
    steps::run(test, serial, vm, steps::BOOT_TOUR)?;
    steps::run(test, serial, vm, steps::BOOT_KERNEL)?;
    follow(vm)
}

/// The second boot: a clone of `five`, from the palette.
fn second(test: &Test, vm: &mut Vm<'_>, serial: &mut Serial) -> Result<Boot> {
    super::start(test, serial, vm)?;
    steps::run(test, serial, vm, steps::BOOT_CLONE)?;
    follow(vm)
}

/// Checks that the boot environment got the command line edited.
pub(super) fn check_edited(test: &Test, boot: &Boot) {
    let line = boot.be_line("BE-CMDLINE");
    test.note(&format!("BE-CMDLINE: {line}"));
    test.check(
        line.split_whitespace().any(|word| word == EDITED),
        "the boot environment's command line holds the edit",
    );
}

/// Checks the record of the first boot, `boot.json`: the second kernel,
/// and the command line edited.
pub(super) fn check_record(test: &Test, text: &str) {
    test.note(&format!("boot.json: {text}"));
    let record: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let field = |name: &str| record.get(name).cloned().unwrap_or_default();
    test.check(
        field("kernel") == KERNEL,
        "the record names the kernel 0.0.1-zbmtest",
    );
    let line = field("command_line");
    test.check(
        field("command_line_edited") == true
            && line.as_str().is_some_and(|line| line.contains(EDITED)),
        "the record holds the command line, edited",
    );
}

/// Checks that the boot environment's root is the clone.
pub(super) fn check_clone(test: &Test, boot: &Boot) {
    let root = boot.be_line("BE-ROOT");
    test.check(
        root.split_whitespace().next() == Some(CLONE),
        &format!("the clone of five boots: its root is {CLONE} ({root})"),
    );
}

#[cfg(test)]
mod tests;
