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
//!   mounted read-only on `/tmp/c` meanwhile, still has its links `tmp`
//!   and `dev`.
//!
//! Then the kernel searches of its child [`CRAFTED_MNT`] while it is
//! mounted (ROADMAP.md SB19), its `mnt` leading to the image's `/etc` and
//! its `kernels` to [`KERNELS_LINK`]:
//!
//! - the hook library's mount of [`CRAFTED`] ([`MOUNT_CRAFTED`]) mounts
//!   it on its mount point in ZFSBootMenu's state and prints it;
//! - the library's kernel search of the child ([`FIND_MNT`]) finds no
//!   kernel, and ends with 1;
//! - `zkexec` of the child by files it does not have searches it, mounts
//!   it for its boot and fails at loading them: Enter answers the failed
//!   boot's prompt, and it ends with 1;
//! - [`KERNELS_LINK`] does not exist, the child has a directory of its
//!   own, its name's `mnt` escaped, nothing is mounted on the image's
//!   `/etc`, and once [`CRAFTED`] is unmounted, nothing is mounted below
//!   its path in ZFSBootMenu's state.

use super::CHROOT_QUESTION;
use crate::error::Result;
use crate::expect::SCREEN;
use crate::record::Test;
use crate::scenarios::runtime::setup::{CRAFTED, CRAFTED_MNT, DEV_LINK, KERNELS_LINK, TMP_LINK};
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

/// The hook library's mount of [`CRAFTED`], which prints where it mounted
/// it.
const MOUNT_CRAFTED: &str = "mount_zfs tplain/crafted";

/// Where [`MOUNT_CRAFTED`] mounts [`CRAFTED`]: its mount point in
/// ZFSBootMenu's state, where its links `mnt` and `kernels` show.
const CRAFTED_MOUNTED: &str = "/zfsbootmenu/environments/tplain/crafted/mnt";

/// The library's kernel search of [`CRAFTED_MNT`].
const FIND_MNT: &str = "find_be_kernels tplain/crafted/mnt";

/// `zkexec` of [`CRAFTED_MNT`] by files it does not have, followed by its
/// exit status once it ended.
const ZKEXEC_MNT: &str = "zkexec tplain/crafted/mnt x x; echo ZKEXEC-RC=$?";

/// What the prompt of a failed boot says when the kernel was not loaded.
const NOT_LOADED: &str = "Unable to load kernel or initramfs into memory";

/// [`ZKEXEC_MNT`]'s exit status, its first group, whole: only its line's
/// end ends it. Its command line, which names `$?`, does not match.
const ZKEXEC_ENDED: &str = r"ZKEXEC-RC=([0-9]+)\r*\n";

/// [`CRAFTED_MNT`]'s own directory in ZFSBootMenu's state: its name's
/// `mnt` escaped, beside [`CRAFTED`]'s mount point.
const MNT_STATE: &str = "/zfsbootmenu/environments/tplain/crafted/+mnt";

/// What counts the mounts on the image's `/etc`.
const ON_IMAGE_ETC: &str = "grep -c ' /etc ' /proc/self/mounts";

/// The kernel searches of [`CRAFTED_MNT`] while [`CRAFTED`], whose `mnt`
/// and `kernels` link out of it, is mounted: the library's, then
/// `zkexec`'s, whose boot fails at loading the files it names; then what
/// they left, and [`CRAFTED`] unmounted.
///
/// # Errors
///
/// When a command does not end in time, or `zkexec` does not end.
pub(super) fn crafted_mnt(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("tplain/crafted/mnt searched while tplain/crafted, with its links, is mounted");
    let (code, said) = vm.sh_show(MOUNT_CRAFTED)?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == CRAFTED_MOUNTED),
        &format!("mount_zfs {CRAFTED} mounts it and prints {CRAFTED_MOUNTED} ({code})"),
    );
    let (code, _) = vm.sh_show(FIND_MNT)?;
    test.check(
        code == 1,
        &format!("find_be_kernels {CRAFTED_MNT} finds no kernel and ends with 1 ({code})"),
    );
    boot_of_crafted_mnt(test, vm)?;
    after_the_searches(test, vm)
}

/// `zkexec` of [`CRAFTED_MNT`]: its boot mounts it and fails at loading
/// the files it names, whose prompt Enter answers; then it ends with 1.
///
/// # Errors
///
/// When a key cannot be sent, or `zkexec` does not end.
fn boot_of_crafted_mnt(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("zkexec tplain/crafted/mnt x x: its search, then a boot that cannot load x");
    vm.sendline(ZKEXEC_MNT)?;
    let prompted = vm.expect(&[NOT_LOADED, ZKEXEC_ENDED], 300)? == 0;
    test.check(
        prompted,
        &format!("zkexec mounts {CRAFTED_MNT} for its boot and fails only at loading x"),
    );
    if prompted {
        test.step("Enter: the failed boot's prompt");
        vm.send("\r")?;
        vm.expect1(ZKEXEC_ENDED, 120)?;
    }
    let status = vm.group(1);
    test.check(
        status == "1",
        &format!("zkexec {CRAFTED_MNT} ends with 1 ({status})"),
    );
    Ok(())
}

/// In the recovery shell, after the searches: no kernel list written
/// through [`CRAFTED`]'s `kernels`, [`CRAFTED_MNT`]'s own directory,
/// nothing on the image's `/etc`; then [`CRAFTED`] unmounted, and nothing
/// left mounted below its path.
///
/// # Errors
///
/// When a command does not end in time.
fn after_the_searches(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, _) = vm.sh_show(&format!("test -e {KERNELS_LINK}"))?;
    test.check(
        code == 1,
        &format!(
            "no kernel list went through {CRAFTED}'s kernels: {KERNELS_LINK} is absent ({code})"
        ),
    );
    let (code, _) = vm.sh_show(&format!("test -d '{MNT_STATE}'"))?;
    test.check(
        code == 0,
        &format!("{CRAFTED_MNT} has its own directory, {MNT_STATE} ({code})"),
    );
    let (code, said) = vm.sh_show(ON_IMAGE_ETC)?;
    test.check(
        code == 1 && has_answer(&said, "0"),
        &format!("after the searches, nothing is mounted on the image's /etc ({code})"),
    );
    vm.sh_show(&format!("umount {CRAFTED_MOUNTED}"))?;
    let (code, _) = vm.sh_show(&format!("grep ' {CRAFTED_STATE}/' /proc/self/mounts"))?;
    test.check(
        code == 1,
        &format!("once {CRAFTED} is unmounted, nothing is mounted below {CRAFTED_STATE} ({code})"),
    );
    Ok(())
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
