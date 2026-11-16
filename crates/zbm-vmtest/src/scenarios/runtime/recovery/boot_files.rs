//! The kernel search and the boot of a filesystem whose `/boot` links out
//! of it (ROADMAP.md SB20), in `runtime-rust-recovery`: `runtime-setup`'s
//! [`CRAFTED`], unmounted, whose `/boot` holds `vmlinuz-3` with a `.kcl`
//! that is an absolute link to `/etc/zfsbootmenu.conf`, of which it has its
//! own ([`CRAFTED_OWN`]); `vmlinuz-4` with `initramfs-4.img` an absolute
//! link to `/etc/hostid`, which it does not have; `vmlinuz-6` with a FIFO
//! as its `.kcl`; and a kernel and initramfs named `vmlinuz-9`, a newline,
//! then `tplain`, `x` and `y` after tabs, which sort last.
//!
//! - `zkexec` of `vmlinuz-4` and `initramfs-4.img` ([`ZKEXEC_LINKED`])
//!   searches [`CRAFTED`], which writes its kernel list and command line
//!   cache, and ends: the FIFO does not hold it. Its boot mounts
//!   [`CRAFTED`] and is refused at the initramfs, which leads nowhere
//!   inside it: Enter answers the failed boot's prompt, it ends with 1, and
//!   the kernel log says why ([`REFUSAL_LOGGED`]). The prompt and the
//!   status alone do not tell the refusal from a load that fails, as this
//!   boot failed before SB20;
//! - every line of the kernel list names [`CRAFTED`], and they are those of
//!   `vmlinuz-3` and `vmlinuz-6` alone: none of `vmlinuz-4`, and none that
//!   the newline name makes, naming `tplain`'s root dataset;
//! - the library's kernel selection of [`CRAFTED`] takes its `vmlinuz-6`,
//!   where it took the last piece of the newline name before SB20;
//! - `vmlinuz-3`'s cached command line is [`CRAFTED`]'s own
//!   `/etc/zfsbootmenu.conf`, not the image's, and the FIFO is not cached;
//! - a guard, which held before SB20 too: nothing is left mounted on
//!   `tplain`'s mount point in ZFSBootMenu's state or below [`CRAFTED`]'s
//!   path.

use super::links::{CRAFTED_STATE, NOT_LOADED, ZKEXEC_ENDED};
use crate::error::Result;
use crate::record::Test;
use crate::scenarios::runtime::setup::{CRAFTED, CRAFTED_OWN};
use crate::vm::Vm;

/// `zkexec` of [`CRAFTED`]'s `vmlinuz-4` and its initramfs, a link to
/// `/etc/hostid`, followed by its exit status once it ended.
const ZKEXEC_LINKED: &str = "zkexec tplain/crafted vmlinuz-4 initramfs-4.img; echo ZKEXEC-RC=$?";

/// What counts the kernel log's lines that say why the boot of
/// [`ZKEXEC_LINKED`] was refused: its initramfs, resolved inside
/// [`CRAFTED`], is missing there.
const REFUSAL_LOGGED: &str = "dmesg | grep -c 'initramfs-4.img.*is missing in the environment'";

/// What prints [`CRAFTED`]'s kernel list with its tabs as `^I` and each
/// line's end as `$`.
const KERNEL_LIST: &str = "cat -A /zfsbootmenu/environments/tplain/crafted/kernels";

/// The lines of [`KERNEL_LIST`]: those of `vmlinuz-3` and `vmlinuz-6`, in
/// version order.
const LISTED: [&str; 2] = [
    "tplain/crafted^I/boot/vmlinuz-3^I/boot/initramfs-3.img$",
    "tplain/crafted^I/boot/vmlinuz-6^I/boot/initramfs-6.img$",
];

/// The start of each line of [`KERNEL_LIST`]: [`CRAFTED`] and a tab.
const OWN_LINE: &str = "tplain/crafted^I";

/// The library's kernel selection of [`CRAFTED`], its tabs shown as `^I`.
const SELECTED: &str = "select_kernel tplain/crafted | cat -A";

/// The start of what [`SELECTED`] prints: [`CRAFTED`]'s `vmlinuz-6`.
const SELECTED_LINE: &str = "tplain/crafted^I/boot/vmlinuz-6^I";

/// What prints `vmlinuz-3`'s cached command line.
const CACHED_KCL: &str = "cat /zfsbootmenu/environments/tplain/crafted/kcl/boot:vmlinuz-3.kcl";

/// What tells whether `vmlinuz-6`'s command line, a FIFO, was cached.
const FIFO_KCL: &str = "test -e /zfsbootmenu/environments/tplain/crafted/kcl/boot:vmlinuz-6.kcl";

/// What lists the mounts on `tplain`'s mount point in ZFSBootMenu's state
/// and below [`CRAFTED`]'s path there.
const LEFT_MOUNTED: &str =
    "grep -E ' /zfsbootmenu/environments/tplain/(mnt|crafted/)' /proc/self/mounts";

/// The search and the refused boot of [`CRAFTED`] by `zkexec`, then what
/// they left in ZFSBootMenu's state.
///
/// # Errors
///
/// When a command does not end in time, or `zkexec` does not end.
pub(super) fn crafted_boot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step(
        "zkexec tplain/crafted vmlinuz-4 initramfs-4.img: its initramfs links to /etc/hostid",
    );
    refused_boot(test, vm)?;
    kernel_list(test, vm)?;
    command_lines(test, vm)?;
    let (code, _) = vm.sh_show(LEFT_MOUNTED)?;
    test.check(
        code == 1,
        &format!(
            "a guard: nothing is mounted on tplain's mount point or below {CRAFTED_STATE} \
             ({code})"
        ),
    );
    Ok(())
}

/// [`ZKEXEC_LINKED`]: its search ends, its boot is refused at the
/// initramfs, whose prompt Enter answers, it ends with 1, and the kernel
/// log says why.
///
/// # Errors
///
/// When `zkexec` neither prompts nor ends in time, once that failed a
/// check, or a command does not end in time.
fn refused_boot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    vm.sendline(ZKEXEC_LINKED)?;
    let first = vm.expect(&[NOT_LOADED, ZKEXEC_ENDED], 300);
    test.check(
        first.is_ok(),
        "zkexec's search of tplain/crafted ends: the FIFO vmlinuz-6.kcl does not hold it",
    );
    let prompted = first? == 0;
    test.check(
        prompted,
        &format!("zkexec mounts {CRAFTED} for its boot and fails at its files: the prompt"),
    );
    if prompted {
        test.step("Enter: the failed boot's prompt");
        vm.send("\r")?;
        vm.expect1(ZKEXEC_ENDED, 120)?;
    }
    let status = vm.group(1);
    test.check(
        status == "1",
        &format!("zkexec {CRAFTED} ends with 1 ({status})"),
    );
    let (code, said) = vm.sh_show(REFUSAL_LOGGED)?;
    let count = count(&said);
    test.check(
        code == 0 && count >= 1,
        &format!(
            "the kernel log says why: initramfs-4.img is missing in the environment ({count})"
        ),
    );
    Ok(())
}

/// [`CRAFTED`]'s kernel list, as the search of [`ZKEXEC_LINKED`] wrote
/// it, and the library's kernel selection of it.
///
/// # Errors
///
/// When a command does not end in time.
fn kernel_list(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = vm.sh_show(KERNEL_LIST)?;
    let lines = listed(&said);
    test.check(
        code == 0 && !lines.is_empty() && lines.iter().all(|line| line.starts_with(OWN_LINE)),
        &format!("every line of {CRAFTED}'s kernel list names {CRAFTED} ({code})"),
    );
    test.check(
        lines == LISTED,
        &format!(
            "{CRAFTED}'s kernel list holds vmlinuz-3 and vmlinuz-6 alone: not vmlinuz-4, whose \
             initramfs is missing inside it, nor the newline name ({})",
            lines.len()
        ),
    );
    let (_, said) = vm.sh_show(SELECTED)?;
    test.check(
        listed(&said)
            .first()
            .is_some_and(|line| line.starts_with(SELECTED_LINE)),
        &format!("the library's select_kernel {CRAFTED} takes its vmlinuz-6"),
    );
    Ok(())
}

/// The command line cache of [`CRAFTED`], as the search of
/// [`ZKEXEC_LINKED`] wrote it.
///
/// # Errors
///
/// When a command does not end in time.
fn command_lines(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = vm.sh_show(CACHED_KCL)?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == CRAFTED_OWN),
        &format!(
            "vmlinuz-3's cached command line is {CRAFTED}'s own /etc/zfsbootmenu.conf, \
             {CRAFTED_OWN} ({code})"
        ),
    );
    let (code, _) = vm.sh_show(FIFO_KCL)?;
    test.check(
        code == 1,
        &format!("the FIFO vmlinuz-6.kcl is not read: no command line is cached for it ({code})"),
    );
    Ok(())
}

/// The lines of `said` that `cat -A` ended with `$`, trimmed.
fn listed(said: &str) -> Vec<&str> {
    said.lines()
        .map(str::trim)
        .filter(|line| line.ends_with('$'))
        .collect()
}

/// The number `grep -c` printed in `said`, its last line that is one: 0
/// when there is none.
fn count(said: &str) -> u32 {
    said.lines()
        .rev()
        .find_map(|line| line.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
