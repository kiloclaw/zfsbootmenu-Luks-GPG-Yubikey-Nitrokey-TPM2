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
//! - the shell's `logs`, which is `zbm`'s, through a pipe: it needs no
//!   terminal, exits 0, and prints the warning start-up logged at the
//!   boot, for the import policy the command line names and start-up does
//!   not know;
//! - the shell's `zbm help`, through a pipe: it exits 0 and prints the
//!   recovery shell's commands, `zfs-chroot` among them; and `zbm --help`
//!   prints `zbm`'s usage, not the menu's, as `/bin/zbm` gives `zbm` its
//!   own options;
//! - the shell's `help`, its alias of `zbm help`: through a pipe it prints
//!   the commands, `zfs-chroot` among them; on the terminal it opens the
//!   pager on the commands, and `q` leaves it, back at the prompt, `help`
//!   ending with 0;
//! - the shell's zreport, which is `zbm`'s, through a pipe: it exits 0
//!   and lists the built-in step `teardown.d/80-luks-close.sh` among its
//!   steps, to run, as nothing replaces or skips it on this machine;
//! - the shell's `zsnapshots tplain/ROOT/be`, which is `zbm`'s: the session
//!   serves the menu, which opens on the Snapshots tab of `tplain`'s
//!   environment; `!` and Enter, its recovery shell, end it, back at the
//!   prompt, `zsnapshots` ending with 0;
//! - the shell's `command -v fzf` fails: Rust images have no `fzf`;
//! - the shell's completion of `zsnapshots`, called as Tab calls it after
//!   `zsnapshots tplain/`: it offers `tplain`'s boot environment alone,
//!   which `zbm complete` lists, so the `.bashrc`'s completions run in the
//!   image's shell;
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
pub(crate) const CHROOT_QUESTION: &str = "screen:Entering chroot in";

/// The last of the chroot's notices, once its shell is found.
pub(crate) const CHROOT_NOTICES: &str = "screen:/tmp is mounted read/write";

/// What the test boot environment's `/etc/os-release` names.
const TEST_ENVIRONMENT: &str = "ZBM test boot environment";

/// What the scenario adds to the runtime images' command line: an import
/// policy start-up does not know, which it warns about at the boot, so that
/// the kernel log certainly holds one of its warnings. The policy stays
/// the default, host ID matching.
const UNKNOWN_POLICY: &str = "zbm.import_policy=zbmtest";

/// The warning start-up logs at the boot for [`UNKNOWN_POLICY`].
const BOOT_WARNING: &str = "ZFSBootMenu: unknown import policy 'zbmtest', defaulting to hostid";

/// `logs` through a pipe, which shows its own exit status (not as `RC=`,
/// which ends a command's output).
const LOGS_PIPED: &str = "{ logs; echo LOGS-STATUS=$?; } | cat";

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
    let append = format!("{} {UNKNOWN_POLICY}", super::ESCAPE_APPEND);
    let machine = Machine::boot(&image, &append)
        .with_disks(vec![super::disk(settings, 0)?, Disk::new(esp, "esp")]);
    let mut vm = Vm::start(test, "recovery", &machine)?;
    super::escape_countdown(&mut vm)?;
    menu_chroot(test, &mut vm, Duration::from_secs(2))?;
    tools_to_shell(test, &mut vm, Duration::from_secs(2))?;
    shell_tools(test, &mut vm)?;
    logs(test, &mut vm)?;
    help(test, &mut vm)?;
    zreport(test, &mut vm)?;
    zsnapshots(test, &mut vm)?;
    no_fzf(test, &mut vm)?;
    completion(test, &mut vm)?;
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

/// Runs `logs` through a pipe, which must exit 0 without a terminal and
/// print the warning start-up logged at the boot.
///
/// # Errors
///
/// When the command does not end in time.
fn logs(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("logs | cat: ZFSBootMenu's errors and warnings, without a terminal");
    let (_, said) = vm.sh_show(LOGS_PIPED)?;
    test.check(
        said.lines().any(|line| line.trim() == "LOGS-STATUS=0"),
        "logs exits 0 through a pipe",
    );
    test.check(
        said.lines().any(|line| line.trim() == BOOT_WARNING),
        "logs prints the warning start-up logged at the boot",
    );
    Ok(())
}

/// `zbm help` through a pipe, which shows its own exit status.
const HELP_PIPED: &str = "{ zbm help; echo HELP-STATUS=$?; } | cat";

/// The first line of `zbm`'s usage.
const ZBM_USAGE: &str = "Usage: zbm <command> [arguments]";

/// Runs `zbm help` through a pipe, which must exit 0 and list the
/// recovery shell's commands, then `zbm --help`, which must print `zbm`'s
/// usage; then the shell's `help`, through a pipe and in the pager.
///
/// # Errors
///
/// When a command does not end in time.
fn help(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("zbm help | cat: the recovery shell's commands, without a terminal");
    let (_, said) = vm.sh_show(HELP_PIPED)?;
    test.check(
        said.lines().any(|line| line.trim() == "HELP-STATUS=0"),
        "zbm help exits 0 through a pipe",
    );
    test.check(
        said.lines().any(|line| line.trim() == "zfs-chroot"),
        "zbm help lists zfs-chroot",
    );
    let (code, said) = vm.sh_show("zbm --help")?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == ZBM_USAGE),
        &format!("zbm --help prints zbm's usage, not the menu's ({code})"),
    );
    test.step("help | cat: the alias, through a pipe");
    let (_, said) = vm.sh_show(ALIAS_PIPED)?;
    test.check(
        said.lines().any(|line| line.trim() == "ALIAS-STATUS=0")
            && said.lines().any(|line| line.trim() == "zfs-chroot"),
        "the shell's help is zbm help: through a pipe it lists zfs-chroot",
    );
    pager(test, vm)
}

/// The shell's `help` through a pipe, which shows its own exit status.
const ALIAS_PIPED: &str = "{ help; echo ALIAS-STATUS=$?; } | cat";

/// The shell's `help` on the terminal, then its exit status once it ends.
const HELP_PAGED: &str = "help; echo PAGER-RC=$?";

/// The title of the pager on the commands, as its border shows it.
const PAGER_TITLE: &str = "screen:COMMANDS";

/// Runs the shell's `help` on the terminal, which must open the pager on
/// the commands; `q` must leave it, back at the prompt, `help` ending with
/// 0. Where no pager shows, Ctrl-C ends whatever runs instead.
///
/// # Errors
///
/// When a key cannot be sent.
fn pager(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("help: the pager on the terminal, then q");
    vm.sendline(HELP_PAGED)?;
    let shown = vm.expect1(PAGER_TITLE, 60).is_ok();
    test.check(shown, "the shell's help opens the pager on the commands");
    vm.send(if shown { "q" } else { "\x03" })?;
    let left = vm.expect1("PAGER-RC=0", 60).is_ok();
    test.check(
        left,
        "q leaves the pager, back at the prompt, help ending with 0",
    );
    Ok(())
}

/// zreport through a pipe, which shows its own exit status.
const ZREPORT_PIPED: &str = "{ zreport; echo ZREPORT-STATUS=$?; } | cat";

/// The built-in step the report must list, by stage and name.
const LUKS_CLOSE: &str = "teardown.d/80-luks-close.sh";

/// Runs zreport through a pipe, which must exit 0 and list the LUKS
/// close among the built-in steps, with no state after it: it runs.
///
/// # Errors
///
/// When the command does not end in time.
fn zreport(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("zreport | cat: the system report, with the built-in steps");
    let (_, said) = vm.sh_show(ZREPORT_PIPED)?;
    test.check(
        said.lines().any(|line| line.trim() == "ZREPORT-STATUS=0"),
        "zreport exits 0 through a pipe",
    );
    let steps = said.split(">> Built-in steps").nth(1).unwrap_or_default();
    test.check(
        steps
            .lines()
            .any(|line| line.contains(LUKS_CLOSE) && !line.contains(" (")),
        &format!("zreport lists the built-in step {LUKS_CLOSE}, which runs"),
    );
    Ok(())
}

/// The recovery shell's `zsnapshots` of `tplain`'s environment, followed by
/// its exit status once the menu ended.
const ZSNAPSHOTS: &str = "clear; zsnapshots tplain/ROOT/be; echo ZSNAPSHOTS-RC=$?";

/// The title of the Snapshots tab's column, as the menu draws it, in the
/// order its snapshots are sorted by.
const SNAPSHOTS_TITLE: &str = "screen:SNAPSHOTS BY";

/// Runs `zsnapshots` on `tplain`'s environment, which must open the menu
/// on its Snapshots tab; `!` and Enter, the recovery shell, must end it,
/// back at the prompt, `zsnapshots` ending with 0.
///
/// # Errors
///
/// When a key cannot be sent.
fn zsnapshots(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("zsnapshots tplain/ROOT/be: the menu on the environment's Snapshots tab");
    vm.sendline(ZSNAPSHOTS)?;
    let shown = vm.expect1(SNAPSHOTS_TITLE, 120).is_ok();
    test.check(
        shown && vm.console().screen().contains("tplain/ROOT/be"),
        "zsnapshots opens the menu on the Snapshots tab of tplain/ROOT/be",
    );
    crate::boot::ask_menu_for_shell(vm)?;
    let left = vm.expect1("ZSNAPSHOTS-RC=0", 120).is_ok();
    test.check(
        left,
        "! and Enter leave the menu, back at the prompt, zsnapshots ending with 0",
    );
    Ok(())
}

/// Asks the recovery shell for `fzf`, which a Rust image does not have.
///
/// # Errors
///
/// When the command does not end in time.
fn no_fzf(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("command -v fzf: Rust images have no fzf");
    let (code, _) = vm.sh_show("command -v fzf")?;
    test.check(
        code != 0,
        &format!("the image has no fzf: command -v fzf fails ({code})"),
    );
    Ok(())
}

/// The `.bashrc`'s completion, called as Tab calls it for the word
/// `tplain/` after `zsnapshots`, then what it offers, between `<` and `>`.
const COMPLETION: &str =
    "COMP_CWORD=1; _zbm_complete zsnapshots tplain/; echo \"<${COMPREPLY[*]}>\"";

/// What [`COMPLETION`] prints: `tplain`'s boot environment, alone.
const COMPLETED: &str = "<tplain/ROOT/be>";

/// Calls the recovery shell's completion of `zsnapshots`'s argument as Tab
/// does, which must offer `tplain`'s boot environment alone.
///
/// # Errors
///
/// When the command does not end in time.
fn completion(test: &Test, vm: &mut Vm<'_>) -> Result<()> {
    test.step("Tab after zsnapshots tplain/: the boot environments zbm complete lists");
    let (code, said) = vm.sh_show(COMPLETION)?;
    test.check(
        code == 0 && said.lines().any(|line| line.trim() == COMPLETED),
        &format!("Tab after zsnapshots tplain/ offers tplain/ROOT/be alone ({code})"),
    );
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
