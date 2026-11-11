//! `runtime-rust-recovery`: the recovery tools of the Rust runtime as
//! `zbm`'s applets (ADR 0018). One boot of the image of `runtime-rust`, with
//! `tplain` (d0), on the group's machine, which boots from a BIOS:
//!
//! - the menu's chroot, from the Recovery Tools: the pool is imported
//!   read-only, so it asks first, and Enter keeps it read-only; the shell
//!   in `tplain`'s environment reads its `/etc/os-release`, and `exit` is
//!   back at the menu, on its Recovery Tools, which the chroot left open.
//!   The environment is not encrypted: no key is loaded;
//! - the menu's Recovery Tools list no Firmware Setup, which a BIOS cannot
//!   open, but Reboot and Power Off;
//! - in the recovery shell, `firmware-setup` says why and exits 1;
//!   `reboot --help` and `shutdown --help` print the usages of `zbm`'s
//!   `reboot` and `poweroff`, through the image's links. Neither is run,
//!   since either would end the machine;
//! - the shell's `zfs-chroot tplain/ROOT/be`, which is `zbm`'s: it asks as
//!   the menu's chroot does, Enter keeps it read-only, its shell reads the
//!   environment's `/etc/os-release`, and `exit` is back in the recovery
//!   shell, `zfs-chroot` ending with 0;
//! - the shell's `mount_esp /dev/disk/by-id/virtio-esp`, which is `zbm`'s:
//!   it mounts the machine's FAT disk `esp` read/write at
//!   `/mnt/virtio-esp`, prints that, and leaves it mounted; asked again, it
//!   prints where the disk is mounted already;
//! - last, the shell's `zkexec tplain/ROOT/be vmlinuz-KVER
//!   initramfs-KVER.img`, which is `zbm`'s: it boots the environment's
//!   kernel by its files through the menu's boot, which the boot
//!   environment's report shows.

use std::thread;
use std::time::Duration;

use crate::boot::{Follow, check_be, follow_boot, prompt};
use crate::disks::vfat_disk;
use crate::error::{Error, Result};
use crate::images::be_kver;
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The title of the menu's Recovery Tools.
const TOOLS: &str = "screen:RECOVERY TOOLS";

/// What `firmware-setup` says on a machine that boots from a BIOS.
const NO_FIRMWARE_SETUP: [&str; 2] = [
    "firmware-setup: unable to reboot into the firmware setup",
    "the firmware does not say that it can open its setup",
];

/// How long a step may take, in seconds.
const PATIENCE: u64 = 1200;

/// The chroot's question, which counts down.
const CHROOT_QUESTION: &str = "screen:Entering chroot in";

/// The last of the chroot's notices, once its shell is found.
const CHROOT_NOTICES: &str = "screen:/tmp is mounted read/write";

/// What the test boot environment's `/etc/os-release` names.
const TEST_ENVIRONMENT: &str = "ZBM test boot environment";

/// Boots, looks at the Recovery Tools, then runs the shell's tools.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let kver = be_kver(settings)?;
    let image = super::image(settings, "auto", ZbmRuntime::Rust, None)?;
    let dir = settings.disks("runtime");
    std::fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let esp = vfat_disk(&dir.join("esp.img"), "ESP", &[], "64M", 512)?;
    let machine = Machine::boot(&image, super::ESCAPE_APPEND)
        .with_disks(vec![super::disk(settings, 0)?, Disk::new(esp, "esp")]);
    let mut vm = Vm::start(test, "recovery", &machine)?;
    super::escape_countdown(&mut vm)?;
    menu_chroot(test, &mut vm, Duration::from_secs(2))?;
    tools_to_shell(test, &mut vm, Duration::from_secs(2))?;
    shell_tools(test, &mut vm)?;
    vm.sendline(SHELL_CHROOT)?;
    in_the_shell_s_chroot(test, &mut vm)?;
    mount_esp(test, &mut vm)?;
    boot_by_files(test, &mut vm, &kver)?;
    vm.kill();
    Ok(())
}

/// Opens the menu's chroot of `tplain`'s environment from the Recovery
/// Tools, the second of them; Enter at the question keeps it read-only.
/// Reads the environment's `/etc/os-release` in the chroot's shell, then
/// leaves it for the menu, which still shows its Recovery Tools with the
/// chroot selected: Up selects the Recovery Shell again and Escape goes
/// back to the boot environments. `pause` is how long to leave the menu
/// between two keys.
fn menu_chroot(test: &Test, vm: &mut Vm<'_>, pause: Duration) -> Result<()> {
    vm.expect1(prompt::NEW_MENU, PATIENCE)?;
    test.step("menu -> !, Down, Enter: the chroot from the Recovery Tools");
    thread::sleep(pause);
    vm.send("!")?;
    let shown = vm.expect1(TOOLS, 60).is_ok();
    test.check(
        shown && vm.console().screen().contains("Chroot"),
        "the menu's Recovery Tools offer the chroot",
    );
    thread::sleep(pause);
    vm.send("\x1b[B")?;
    thread::sleep(pause);
    vm.send("\r")?;
    let asked = vm.expect1(CHROOT_QUESTION, 120).is_ok();
    test.check(
        asked && vm.console().screen().contains("Enter r/o chroot"),
        "a chroot on a pool imported read-only asks first",
    );
    test.step("Enter: a read-only chroot");
    vm.send("\r")?;
    let noticed = vm.expect1(CHROOT_NOTICES, 120).is_ok();
    test.check(
        noticed
            && vm
                .console()
                .screen()
                .contains("tplain/ROOT/be is mounted read-only"),
        "Enter at the question mounts the environment read-only",
    );
    thread::sleep(pause);
    let (code, said) = vm.sh_show("cat /etc/os-release")?;
    test.check(
        code == 0 && said.contains(TEST_ENVIRONMENT),
        &format!("the chroot's shell reads the environment's os-release ({code})"),
    );
    test.step("exit: back to the menu's Recovery Tools");
    vm.sendline("exit")?;
    let back = vm.expect1(TOOLS, 120).is_ok();
    test.check(
        back,
        "the menu is back on its Recovery Tools once the chroot's shell ends",
    );
    test.step("Up, Escape: the Recovery Shell selected, then the boot environments");
    thread::sleep(pause);
    vm.send("\x1b[A")?;
    thread::sleep(pause);
    vm.send("\x1b")
}

/// Opens the menu's Recovery Tools, checks what they list on a machine
/// that boots from a BIOS, then opens the recovery shell, the first of
/// them; `pause` is how long to leave the menu between two keys.
fn tools_to_shell(test: &Test, vm: &mut Vm<'_>, pause: Duration) -> Result<()> {
    vm.expect1(prompt::NEW_MENU, PATIENCE)?;
    test.step("menu -> !: the Recovery Tools");
    thread::sleep(pause);
    vm.send("!")?;
    let shown = vm.expect1(TOOLS, 60).is_ok();
    let screen = vm.console().screen();
    test.check(
        shown && screen.contains("Reboot") && screen.contains("Power Off"),
        "the menu's Recovery Tools list Reboot and Power Off",
    );
    test.check(
        shown && !screen.contains("Firmware Setup"),
        "on a BIOS, the menu's Recovery Tools list no Firmware Setup",
    );
    test.step("Enter: the Recovery Shell");
    thread::sleep(pause);
    vm.send("\r")?;
    vm.shell_ready()
}

/// Runs `firmware-setup`, which must say why it cannot, and asks `reboot`
/// and `shutdown` for their usages, which `zbm` prints.
fn shell_tools(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let (code, said) = vm.sh_show("firmware-setup")?;
    test.check(
        code == 1,
        &format!("on a BIOS, firmware-setup exits 1 ({code})"),
    );
    test.check(
        NO_FIRMWARE_SETUP.iter().all(|line| said.contains(line)),
        "firmware-setup says why the machine cannot reboot into its setup",
    );
    for (name, usage) in [("reboot", "Usage: reboot"), ("shutdown", "Usage: poweroff")] {
        let (code, said) = vm.sh_show(&format!("{name} --help"))?;
        test.check(
            code == 0 && said.contains(usage),
            &format!("{name} is zbm's: --help prints {usage:?} ({code})"),
        );
    }
    Ok(())
}

/// The recovery shell's chroot of `tplain`'s environment, followed by its
/// exit status once its shell ended.
pub(crate) const SHELL_CHROOT: &str = "zfs-chroot tplain/ROOT/be; echo ZFS-CHROOT-RC=$?";

/// The recovery shell's chroot, once [`SHELL_CHROOT`] started and, in a
/// locked image, the PIN was typed: the question, Enter for a read-only
/// chroot, the environment's `/etc/os-release` read in its shell, and
/// `exit`, after which `zfs-chroot` ends with 0 in the recovery shell.
///
/// # Errors
///
/// When a step cannot be done.
pub(crate) fn in_the_shell_s_chroot(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    let asked = vm.expect1(CHROOT_QUESTION, 120).is_ok();
    test.check(
        asked && vm.console().screen().contains("Enter r/o chroot"),
        "the shell's zfs-chroot asks first on a pool imported read-only",
    );
    test.step("Enter: the shell's chroot, read-only");
    vm.send("\r")?;
    let noticed = vm.expect1(CHROOT_NOTICES, 120).is_ok();
    test.check(
        noticed
            && vm
                .console()
                .screen()
                .contains("tplain/ROOT/be is mounted read-only"),
        "the shell's zfs-chroot mounts the environment read-only after Enter",
    );
    let (code, said) = vm.sh_show("cat /etc/os-release")?;
    test.check(
        code == 0 && said.contains(TEST_ENVIRONMENT),
        &format!("the shell's chroot reads the environment's os-release ({code})"),
    );
    test.step("exit: back to the recovery shell");
    vm.sendline("exit")?;
    let ended = vm.expect1("ZFS-CHROOT-RC=0", 120).is_ok();
    test.check(
        ended,
        "zfs-chroot ends with 0 in the recovery shell once its shell ended",
    );
    Ok(())
}

/// The recovery shell's `mount_esp` of the machine's FAT disk `esp`.
const MOUNT_ESP: &str = "mount_esp /dev/disk/by-id/virtio-esp";

/// Where `mount_esp` mounts that disk: `/mnt/` and the last part of the
/// path, as the legacy script named it.
const ESP_MOUNTED: &str = "/mnt/virtio-esp";

/// Runs `mount_esp` on the FAT disk `esp`, which must print where it
/// mounted it and leave it mounted read/write there, then runs it again,
/// which must print where it is mounted already.
///
/// # Errors
///
/// When a command does not end in time.
fn mount_esp(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("mount_esp: the FAT disk esp, by its link");
    let (code, said) = vm.sh_show(MOUNT_ESP)?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == ESP_MOUNTED),
        &format!("mount_esp mounts the disk and prints {ESP_MOUNTED} ({code})"),
    );
    let (code, _) = vm.sh_show(&format!("grep ' {ESP_MOUNTED} vfat rw,' /proc/self/mounts"))?;
    test.check(
        code == 0,
        &format!("the disk stays mounted read/write at {ESP_MOUNTED} ({code})"),
    );
    let (code, said) = vm.sh_show(MOUNT_ESP)?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == ESP_MOUNTED),
        &format!("mount_esp again prints where the disk is mounted already ({code})"),
    );
    Ok(())
}

/// The recovery shell's `zkexec` of `tplain`'s environment, whose kernel
/// is of version `kver`: the boot is followed to the boot environment,
/// whose report must show `tplain`'s root and pool.
///
/// # Errors
///
/// When an awaited message does not come.
fn boot_by_files(test: &Test, vm: &mut Vm<'_>, kver: &str) -> Result<()> {
    test.step("zkexec: tplain's environment, by its kernel's files");
    vm.sendline(&format!(
        "zkexec tplain/ROOT/be vmlinuz-{kver} initramfs-{kver}.img"
    ))?;
    let follow = Follow {
        timeout: Some(PATIENCE),
        ..Follow::default()
    };
    let boot = follow_boot(vm, follow)?;
    check_be(test, &boot, "tplain", &["tplain"], &[]);
    Ok(())
}

#[cfg(test)]
mod tests;
