//! The chroot of a filesystem whose mount points link out of it (ROADMAP.md
//! SB16), in `runtime-rust-recovery`: `runtime-setup`'s
//! [`CRAFTED`], whose `tmp` leads to the image's `/etc` and whose `dev`
//! climbs to the image's `/etc/zfs`.
//!
//! - the recovery shell's `zfs-chroot` of it, read-only after Enter at the
//!   question, says that `/tmp` and `/dev` are not mounted, as links, and
//!   not that `/tmp` is mounted; its shell opens all the same;
//! - in the chroot, the image's `/proc/1/mountinfo` has nothing mounted
//!   on `/etc` or `/etc/zfs`, `/proc` and `/sys` are mounted, and the
//!   shell is the filesystem's `/bin/sh`, its link to `/bin/busybox`
//!   followed inside the root;
//! - the shell mounts a `tmpfs` over its own `/`, then `exit` ends
//!   `zfs-chroot` with 0;
//! - after it, nothing is mounted below the filesystem's path in
//!   ZFSBootMenu's state, or on `/etc` or `/etc/zfs`, and the filesystem,
//!   mounted read-only on `/tmp/c` meanwhile, still has its two links.

use super::CHROOT_QUESTION;
use crate::error::Result;
use crate::expect::SCREEN;
use crate::record::Test;
use crate::scenarios::runtime::setup::{CRAFTED, DEV_LINK, TMP_LINK};
use crate::vm::Vm;

/// The recovery shell's chroot of [`CRAFTED`], followed by its exit status
/// once its shell ended.
const CRAFTED_CHROOT: &str = "zfs-chroot tplain/crafted; echo ZFS-CHROOT-RC=$?";

/// The chroot's notice that `/tmp`, a link, is not mounted.
const CRAFTED_REFUSED: &str = "screen:/tmp is not mounted: it is a link to /etc";

/// The chroot's notice that `/dev`, a link, is not mounted.
const DEV_REFUSED: &str = "/dev is not mounted: it is a link to ../../../../../../../etc/zfs";

/// The notice of a `/tmp` mounted, which this chroot does not give.
const TMP_MOUNTED: &str = "/tmp is mounted read/write";

/// The chroot shell's prompt, once the notices are said.
const CRAFTED_PROMPT: &str = "screen:tplain/crafted / > ";

/// [`CRAFTED_CHROOT`]'s exit status on the screen: `zfs-chroot` has ended,
/// and the recovery shell is back. Its command line, which names `$?`,
/// does not match.
const CHROOT_ENDED: &str = "screen:ZFS-CHROOT-RC=[0-9]";

/// What counts the mounts on the image's `/etc` and `/etc/zfs`, the mount
/// point being the fifth field of the image's init's `mountinfo`. The
/// filesystem has no program but busybox, which runs each one.
const ON_ETC: &str = "busybox grep -cE '^([^ ]+ ){4}/etc(/zfs)? ' /proc/1/mountinfo";

/// What counts the chroot's `/proc` and `/sys` mounts.
const KERNEL_FS: &str = "busybox grep -cE ' /(proc|sys) ' /proc/mounts";

/// What prints the arguments of the chroot's shell, separated by spaces.
pub(super) const SHELL_CMDLINE: &str = r"busybox tr '\0' ' ' < /proc/$$/cmdline";

/// What mounts a `tmpfs` over the chroot's own `/`.
const OVERMOUNT: &str = "busybox mount -t tmpfs x /";

/// Where ZFSBootMenu's state puts [`CRAFTED`], its mount point below it.
const CRAFTED_STATE: &str = "/zfsbootmenu/environments/tplain/crafted";

/// The recovery shell's chroot of [`CRAFTED`]: the notices, the checks in
/// its shell, which then mounts over its own `/`, and what is left once
/// it ended. Enter at the question, if it is asked, keeps it read-only.
/// The notices are looked for on the screen once the shell's prompt shows
/// below them, so that a chroot that mounts `/tmp` all the same is checked
/// as well; nothing runs in a shell that did not open.
///
/// # Errors
///
/// When a step cannot be done, or the prompt does not show and
/// `zfs-chroot` has not ended: its shell may still be open, and the steps
/// after it would run in the chroot.
pub(super) fn crafted_chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("zfs-chroot tplain/crafted: its tmp and dev link out of it");
    vm.sendline(CRAFTED_CHROOT)?;
    let first = vm.expect(&[CHROOT_QUESTION, CRAFTED_REFUSED, CRAFTED_PROMPT], 120);
    if matches!(first, Ok(0)) {
        test.step("Enter: the chroot, read-only");
        vm.send("\r")?;
    }
    let opened = vm.expect1(CRAFTED_PROMPT, 120).is_ok();
    let screen = vm.console().screen();
    let refused = on_screen(CRAFTED_REFUSED);
    test.check(
        screen.contains(refused),
        &format!("the chroot says {refused}"),
    );
    test.check(
        screen.contains(DEV_REFUSED),
        &format!("the chroot says {DEV_REFUSED}"),
    );
    test.check(opened, "the chroot's shell opens without /tmp and /dev");
    test.check(
        opened && !screen.contains(TMP_MOUNTED),
        &format!("the chroot does not say {TMP_MOUNTED}"),
    );
    if !opened {
        return vm.expect1(CHROOT_ENDED, 5);
    }
    in_the_crafted_chroot(test, vm)?;
    after_the_crafted_chroot(test, vm)
}

/// `pattern` without [`SCREEN`].
fn on_screen(pattern: &str) -> &str {
    pattern.strip_prefix(SCREEN).unwrap_or(pattern)
}

/// In the chroot's shell: no mount on the image's `/etc`, the kernel's
/// file systems, the shell's name, then a `tmpfs` over `/` and `exit`.
///
/// # Errors
///
/// When a command does not end in time.
fn in_the_crafted_chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = vm.sh_show(ON_ETC)?;
    test.check(
        code == 1 && has_answer(&said, "0"),
        &format!("in the chroot, nothing is mounted on the image's /etc or /etc/zfs ({code})"),
    );
    let (code, said) = vm.sh_show(KERNEL_FS)?;
    test.check(
        code == 0 && has_answer(&said, "2"),
        &format!("in the chroot, /proc and /sys are mounted ({code})"),
    );
    let (code, said) = vm.sh_show(SHELL_CMDLINE)?;
    test.check(
        code == 0 && is_sh(&said),
        &format!("in the chroot, the shell is /bin/sh, found inside the root ({code})"),
    );
    test.step("mount -t tmpfs x /: the shell mounts over its own root");
    let (code, _) = vm.sh_show(OVERMOUNT)?;
    test.check(
        code == 0,
        &format!("in the chroot, a tmpfs mounts over its own / ({code})"),
    );
    test.step("exit: back to the recovery shell");
    vm.sendline("exit")?;
    let ended = vm.expect1("ZFS-CHROOT-RC=0", 120).is_ok();
    test.check(
        ended,
        "zfs-chroot tplain/crafted ends with 0 once its shell ended",
    );
    Ok(())
}

/// In the recovery shell, after the chroot: nothing it mounted is left,
/// and the filesystem, mounted read-only on `/tmp/c` meanwhile, still has
/// its links.
///
/// # Errors
///
/// When a command does not end in time, or the filesystem cannot be
/// mounted or unmounted.
fn after_the_crafted_chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("after the chroot: its mounts gone, the filesystem as it was");
    let (code, _) = vm.sh_show(&format!(
        "grep -E ' {CRAFTED_STATE}/| /etc(/zfs)? ' /proc/self/mounts"
    ))?;
    test.check(
        code == 1,
        &format!(
            "after the chroot, nothing is mounted below {CRAFTED_STATE}, or on /etc or /etc/zfs \
             ({code})"
        ),
    );
    vm.must(&format!(
        "mkdir -p /tmp/c && mount -t zfs -o ro,zfsutil {CRAFTED} /tmp/c"
    ))?;
    let (code, said) = vm.sh_show("readlink /tmp/c/tmp /tmp/c/dev")?;
    let links: Vec<&str> = said.lines().map(str::trim).collect();
    test.check(
        code == 0 && links.contains(&TMP_LINK),
        &format!("{CRAFTED}'s tmp is still its link to {TMP_LINK} ({code})"),
    );
    test.check(
        code == 0 && links.contains(&DEV_LINK),
        &format!("{CRAFTED}'s dev is still its link to {DEV_LINK} ({code})"),
    );
    vm.must("umount /tmp/c").map(drop)
}

/// Whether a line of `said`, the chroot shell's prompt left out, is
/// `answer`.
fn has_answer(said: &str, answer: &str) -> bool {
    said.lines().any(|line| after_prompt(line).trim() == answer)
}

/// Whether a line of `said`, the chroot shell's prompt left out, is what
/// [`SHELL_CMDLINE`] prints for `/bin/sh`: the shell's name, and its
/// arguments if it has any.
pub(super) fn is_sh(said: &str) -> bool {
    said.lines().any(|line| {
        let line = after_prompt(line).trim();
        line == "/bin/sh" || line.starts_with("/bin/sh ")
    })
}

/// `line` after the chroot shell's prompt, which ends with `> `: the whole
/// line where it has none.
fn after_prompt(line: &str) -> &str {
    line.rsplit_once("> ").map_or(line, |(_, after)| after)
}

#[cfg(test)]
mod tests;
