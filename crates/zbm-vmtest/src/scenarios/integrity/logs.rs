//! The kernel log of the integrity scenarios' boots. ZFSBootMenu keeps it
//! off the serial console: a boot that reaches its boot environment saves
//! it on a FAT disk (`zbm.save_logs`), and a boot that stays in the menu
//! has it read with `dmesg` in the recovery shell.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::boot::ask_menu_for_shell;
use crate::disks::vfat_disk;
use crate::error::Result;
use crate::record::Test;
use crate::vm::Vm;

/// The label of the FAT disk on which a boot that reaches its boot
/// environment saves its logs (`zbm.save_logs`).
pub(super) const LOGS_LABEL: &str = "ZBMINTLOGS";

/// What reads ZFSBootMenu's lines of the kernel log that the checks look
/// for, in the recovery shell: parts of [`HELD`](super::HELD),
/// [`WAITS`](super::WAITS) and [`REFUSED`](super::REFUSED), which the
/// command itself does not hold whole.
pub(super) const KERNEL_LOG: &str =
    "dmesg | grep -F -e 'host ID alone' -e 'unlocked itself' -e 'lockdown is on'";

/// The menu's dialog after a boot that failed, on the screen.
const FAILED_BOOT: &str = "screen:unable to boot";

/// An empty FAT disk for the logs of the boot `scenario`.
pub(super) fn logs_disk(test: &Test, scenario: &str) -> Result<PathBuf> {
    let path = test.dir().join(format!("{scenario}-logs.img"));
    vfat_disk(&path, LOGS_LABEL, &[], "64M", 512)
}

/// The kernel log the boot `scenario` saved on `logs`, once its machine is
/// gone; empty when it saved none.
pub(super) fn saved_log(test: &Test, scenario: &str, logs: &Path) -> Result<String> {
    let saved = crate::scenarios::runtime::saved::saved_kernel_log(logs)?;
    if saved.is_empty() {
        test.note(&format!(
            "{scenario}: no kernel log saved on {}",
            logs.display()
        ));
    }
    Ok(saved)
}

/// ZFSBootMenu's lines of the kernel log that the checks look for, read in
/// the recovery shell, opened from the menu, which shows.
pub(super) fn kernel_log_in_shell(vm: &mut Vm<'_>) -> Result<String> {
    ask_menu_for_shell(vm)?;
    vm.shell_ready()?;
    Ok(vm.sh_show(KERNEL_LOG)?.1)
}

/// Once the kernel was not loaded: the menu shows why the boot failed,
/// which Enter closes; the lines of the kernel log, read in the recovery
/// shell.
pub(super) fn after_refusal(vm: &mut Vm<'_>) -> Result<String> {
    // The prompt that says so waits 10 seconds, then the menu shows.
    if vm.expect1(FAILED_BOOT, 120).is_ok() {
        vm.test().step("the menu says the boot failed -> Enter");
        thread::sleep(Duration::from_secs(1));
        vm.send("\r")?;
    }
    kernel_log_in_shell(vm)
}
