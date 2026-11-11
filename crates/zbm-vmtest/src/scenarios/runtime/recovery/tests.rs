use pretty_assertions::assert_eq;

use super::*;
use crate::testing::{Step, checks, cleanup, scripted, settings};

/// The Recovery Tools as the menu shows them on a machine that boots from
/// a BIOS.
const BIOS_TOOLS: &str = "\x1b[H\x1b[2JRECOVERY TOOLS\r\n Recovery Shell\r\n Log\r\n \
    Pools and Volumes\r\n Reboot\r\n Power Off\r\n";

/// The same with the Firmware Setup, as on UEFI.
const UEFI_TOOLS: &str = "\x1b[H\x1b[2JRECOVERY TOOLS\r\n Recovery Shell\r\n Log\r\n \
    Pools and Volumes\r\n Firmware Setup\r\n Reboot\r\n Power Off\r\n";

/// The script from the menu to the recovery shell, with the Recovery
/// Tools that `tools` shows.
fn to_shell(tools: &'static str) -> Vec<Step> {
    vec![
        Step::Say("\x1b[H\x1b[2JBOOT ENVIRONMENTS\r\n"),
        Step::Keys("!"),
        Step::Say(tools),
        Step::Keys("\r"),
        Step::Say("zfsbootmenu ~ > "),
        Step::Hear("stty -echo cols 250; export PS1='zbm# ' PS2=''"),
        Step::Hear("true; echo RC=$?"),
        Step::Say("RC=0\r\n"),
    ]
}

/// Plays `script` against [`tools_to_shell`]; the checks it recorded.
fn tools_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    tools_to_shell(&test, &mut vm, Duration::ZERO).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn on_a_bios_the_recovery_tools_list_no_firmware_setup() {
    let found = tools_checks("runtime-recovery-bios", to_shell(BIOS_TOOLS));

    assert_eq!(
        found,
        [
            "ok   - the menu's Recovery Tools list Reboot and Power Off",
            "ok   - on a BIOS, the menu's Recovery Tools list no Firmware Setup",
        ]
    );
}

#[test]
fn recovery_tools_that_list_firmware_setup_fail_on_a_bios() {
    let found = tools_checks("runtime-recovery-uefi", to_shell(UEFI_TOOLS));

    assert_eq!(
        found,
        [
            "ok   - the menu's Recovery Tools list Reboot and Power Off",
            "FAIL - on a BIOS, the menu's Recovery Tools list no Firmware Setup",
        ]
    );
}

/// The shell's answers to [`shell_tools`]: `firmware-setup` says
/// `firmware`, and ends with `code`.
fn shell_script(firmware: &'static str) -> Vec<Step> {
    vec![
        Step::Hear("firmware-setup; echo RC=$?"),
        Step::Say(firmware),
        Step::Hear("reboot --help; echo RC=$?"),
        Step::Say("Usage: reboot\r\n\r\nExports the pools imported read/write.\r\nRC=0\r\n"),
        Step::Hear("shutdown --help; echo RC=$?"),
        Step::Say("Usage: poweroff\r\n       shutdown\r\nRC=0\r\n"),
    ]
}

/// Plays `script` against [`shell_tools`]; the checks it recorded.
fn shell_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    shell_tools(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn in_the_shell_firmware_setup_says_why_and_exits_1() {
    let found = shell_checks(
        "runtime-recovery-shell",
        shell_script(
            "firmware-setup: unable to reboot into the firmware setup\r\n  \
             the firmware does not say that it can open its setup\r\nRC=1\r\n",
        ),
    );

    assert_eq!(
        found,
        [
            "ok   - on a BIOS, firmware-setup exits 1 (1)",
            "ok   - firmware-setup says why the machine cannot reboot into its setup",
            "ok   - reboot is zbm's: --help prints \"Usage: reboot\" (0)",
            "ok   - shutdown is zbm's: --help prints \"Usage: poweroff\" (0)",
        ]
    );
}

#[test]
fn a_firmware_setup_that_says_nothing_fails() {
    let found = shell_checks("runtime-recovery-silent", shell_script("RC=0\r\n"));

    assert_eq!(
        found[..2],
        [
            "FAIL - on a BIOS, firmware-setup exits 1 (0)",
            "FAIL - firmware-setup says why the machine cannot reboot into its setup",
        ]
    );
}

/// The Recovery Tools with the chroot, which every image offers.
const CHROOT_TOOLS: &str = "\x1b[H\x1b[2JRECOVERY TOOLS\r\n Recovery Shell\r\n \
    Chroot into the selected environment\r\n Log\r\n Pools and Volumes\r\n Reboot\r\n \
    Power Off\r\n";

/// The script of the menu's chroot, whose question shows `question`, whose
/// notices are `notices`, and after which the menu shows `back`.
fn chroot_script(question: &'static str, notices: &'static str, back: &'static str) -> Vec<Step> {
    vec![
        Step::Say("\x1b[H\x1b[2JBOOT ENVIRONMENTS\r\n"),
        Step::Keys("!"),
        Step::Say(CHROOT_TOOLS),
        Step::Keys("\x1b[B"),
        Step::Keys("\r"),
        Step::Say(question),
        Step::Keys("\r"),
        Step::Say(notices),
        Step::Hear("cat /etc/os-release; echo RC=$?"),
        Step::Say("NAME=\"ZBM test boot environment\"\r\nRC=0\r\n"),
        Step::Hear("exit"),
        Step::Say(back),
        Step::Keys("\x1b[A"),
        Step::Keys("\x1b"),
    ]
}

/// The question as the console shows it.
const QUESTION: &str = "\x1b[H\x1b[2J[ESCAPE] Enter r/w chroot\r\n[RETURN] Enter r/o chroot\r\n\
    \r\nEntering chroot in 10 seconds";

/// Plays `script` against [`menu_chroot`]; the checks it recorded.
fn chroot_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    menu_chroot(&test, &mut vm, Duration::ZERO).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_menu_s_chroot_reads_the_environment_read_only_and_comes_back() {
    let found = chroot_checks(
        "runtime-recovery-chroot",
        chroot_script(
            QUESTION,
            "\x1b[H\x1b[2J* tplain/ROOT/be is mounted read-only\r\n\
             * /tmp is mounted read/write\r\n\r\ntplain/ROOT/be / > ",
            CHROOT_TOOLS,
        ),
    );

    assert_eq!(
        found,
        [
            "ok   - the menu's Recovery Tools offer the chroot",
            "ok   - a chroot on a pool imported read-only asks first",
            "ok   - Enter at the question mounts the environment read-only",
            "ok   - the chroot's shell reads the environment's os-release (0)",
            "ok   - the menu is back on its Recovery Tools once the chroot's shell ends",
        ]
    );
}

#[test]
fn a_chroot_mounted_read_write_after_enter_fails() {
    let found = chroot_checks(
        "runtime-recovery-chroot-rw",
        chroot_script(
            QUESTION,
            "\x1b[H\x1b[2J* tplain/ROOT/be is mounted read/write\r\n\
             * /tmp is mounted read/write\r\n\r\ntplain/ROOT/be / > ",
            CHROOT_TOOLS,
        ),
    );

    assert_eq!(
        found[2],
        "FAIL - Enter at the question mounts the environment read-only"
    );
}
