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

/// The shell's answers to [`SHELL_CHROOT`] and [`in_the_shell_s_chroot`]:
/// `zfs-chroot` ends with `status`.
fn shell_chroot_script(status: &'static str) -> Vec<Step> {
    vec![
        Step::Hear(SHELL_CHROOT),
        Step::Say(QUESTION),
        Step::Keys("\r"),
        Step::Say(
            "\x1b[H\x1b[2J* tplain/ROOT/be is mounted read-only\r\n\
             * /tmp is mounted read/write\r\n\r\ntplain/ROOT/be / > ",
        ),
        Step::Hear("cat /etc/os-release; echo RC=$?"),
        Step::Say("NAME=\"ZBM test boot environment\"\r\nRC=0\r\n"),
        Step::Hear("exit"),
        Step::Say(status),
    ]
}

/// Plays `script` against the shell's chroot; the checks it recorded.
fn shell_chroot_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    vm.sendline(SHELL_CHROOT).unwrap();
    in_the_shell_s_chroot(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_shell_s_zfs_chroot_reads_the_environment_read_only_and_ends_with_0() {
    let found = shell_chroot_checks(
        "runtime-recovery-shell-chroot",
        shell_chroot_script("ZFS-CHROOT-RC=0\r\nzbm# "),
    );

    assert_eq!(
        found,
        [
            "ok   - the shell's zfs-chroot asks first on a pool imported read-only",
            "ok   - the shell's zfs-chroot mounts the environment read-only after Enter",
            "ok   - the shell's chroot reads the environment's os-release (0)",
            "ok   - zfs-chroot ends with 0 in the recovery shell once its shell ended",
        ]
    );
}

#[test]
fn a_shell_s_zfs_chroot_that_ends_with_1_fails() {
    let found = shell_chroot_checks(
        "runtime-recovery-shell-chroot-1",
        shell_chroot_script("ZFS-CHROOT-RC=1\r\nzbm# "),
    );

    assert_eq!(
        found[3],
        "FAIL - zfs-chroot ends with 0 in the recovery shell once its shell ended"
    );
}

/// The shell's answers to [`mount_esp`]: the first `mount_esp` prints
/// `first`, and the second `again`.
fn mount_esp_script(first: &'static str, again: &'static str) -> Vec<Step> {
    vec![
        Step::Hear("mount_esp /dev/disk/by-id/virtio-esp; echo RC=$?"),
        Step::Say(first),
        Step::Hear("grep ' /mnt/virtio-esp vfat rw,' /proc/self/mounts; echo RC=$?"),
        Step::Say("/dev/vdb /mnt/virtio-esp vfat rw,relatime 0 0\r\nRC=0\r\n"),
        Step::Hear("mount_esp /dev/disk/by-id/virtio-esp; echo RC=$?"),
        Step::Say(again),
    ]
}

/// Plays `script` against [`mount_esp`]; the checks it recorded.
fn mount_esp_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    mount_esp(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_shell_s_mount_esp_prints_the_mount_point_and_leaves_it_mounted() {
    let found = mount_esp_checks(
        "runtime-recovery-mount-esp",
        mount_esp_script("/mnt/virtio-esp\r\nRC=0\r\n", "/mnt/virtio-esp\r\nRC=0\r\n"),
    );

    assert_eq!(
        found,
        [
            "ok   - mount_esp mounts the disk and prints /mnt/virtio-esp (0)",
            "ok   - the disk stays mounted read/write at /mnt/virtio-esp (0)",
            "ok   - mount_esp again prints where the disk is mounted already (0)",
        ]
    );
}

#[test]
fn a_mount_esp_that_refuses_a_disk_mounted_already_fails() {
    let found = mount_esp_checks(
        "runtime-recovery-mount-esp-busy",
        mount_esp_script(
            "/mnt/virtio-esp\r\nRC=0\r\n",
            "mount_esp: mount point /mnt/virtio-esp already in use\r\nRC=1\r\n",
        ),
    );

    assert_eq!(
        found[2],
        "FAIL - mount_esp again prints where the disk is mounted already (1)"
    );
}

/// The boot environment's report after the shell's `zkexec`.
const BOOTED_TPLAIN: &str = "BE-INIT-START\r\nBE-KERNEL: 7.3.1-zbm\r\n\
    BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1\r\nBE-ROOT: tplain/ROOT/be / zfs rw\r\n\
    BE|   pool: tplain\r\nBE|  state: ONLINE\r\nBE-WRITE: ok\r\n\
    BE-SCRUB: tplain pool 'tplain' is healthy\r\nBE-INIT-DONE\r\n";

/// The same report of a boot environment whose root is another pool's.
const BOOTED_TANK: &str = "BE-INIT-START\r\nBE-KERNEL: 7.3.1-zbm\r\n\
    BE-ZFS-VERSION: zfs-2.4.4-1 zfs-kmod-2.4.4-1\r\nBE-ROOT: tank/ROOT/be / zfs rw\r\n\
    BE|   pool: tplain\r\nBE|  state: ONLINE\r\nBE-WRITE: ok\r\n\
    BE-SCRUB: tplain pool 'tplain' is healthy\r\nBE-INIT-DONE\r\n";

/// What the shell's `zkexec` boots into: the teardown's report, then the
/// boot environment's, `booted`.
fn booted_by_files(booted: &'static str) -> Vec<Step> {
    vec![
        Step::Hear("zkexec tplain/ROOT/be vmlinuz-7.3.1-zbm initramfs-7.3.1-zbm.img"),
        Step::Say(
            "ZBMTEST-STATUS-BEGIN\r\nZS|   pool: tplain\r\nZS|  state: ONLINE\r\n\
             ZBMTEST-STATUS-END\r\nZBMTEST-TEARDOWN dm=[] pools=[] state=[]\r\n",
        ),
        Step::Say(booted),
    ]
}

/// Plays `script` against [`boot_by_files`]; the checks it recorded.
fn by_files_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    boot_by_files(&test, &mut vm, "7.3.1-zbm").unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_shell_s_zkexec_boots_the_environment_by_its_files() {
    let found = by_files_checks("runtime-recovery-zkexec", booted_by_files(BOOTED_TPLAIN));

    assert_eq!(
        found,
        [
            "ok   - kexec into the ZFS 2.4.4 boot environment",
            "ok   - boot environment kernel 7.3.1-zbm",
            "ok   - boot environment runs ZFS 2.4.4 (zfs-2.4.4-1 zfs-kmod-2.4.4-1)",
            "ok   - root file system tplain/ROOT/be / zfs rw",
            "ok   - boot environment imported tplain: ONLINE (expected ONLINE)",
            "ok   - root dataset is writable in the boot environment",
            "ok   - scrub of tplain in the boot environment: pool 'tplain' is healthy",
        ]
    );
}

#[test]
fn a_zkexec_into_another_root_fails() {
    let found = by_files_checks(
        "runtime-recovery-zkexec-other",
        booted_by_files(BOOTED_TANK),
    );

    assert_eq!(found[3], "FAIL - root file system tank/ROOT/be / zfs rw");
}

/// The shell's answer to [`logs`]: `logs` prints `said`.
fn logs_script(said: &'static str) -> Vec<Step> {
    vec![
        Step::Hear("{ logs; echo LOGS-STATUS=$?; } | cat; echo RC=$?"),
        Step::Say(said),
    ]
}

/// Plays `script` against [`logs`]; the checks it recorded.
fn logs_checks(name: &str, script: Vec<Step>) -> Vec<String> {
    let settings = settings(name);
    let test = Test::new(&settings, "runtime-rust-recovery").unwrap();
    let (mut vm, guest) = scripted(&test, script);

    logs(&test, &mut vm).unwrap();

    assert_eq!(guest.finish(), Vec::<String>::new());
    let found = checks(&test);
    cleanup(&settings);
    found
}

#[test]
fn the_shell_s_logs_prints_the_boot_s_warning_through_a_pipe() {
    let found = logs_checks(
        "runtime-recovery-logs",
        logs_script(
            "ZFSBootMenu: unknown import policy 'zbmtest', defaulting to hostid\r\n\
             LOGS-STATUS=0\r\nRC=0\r\n",
        ),
    );

    assert_eq!(
        found,
        [
            "ok   - logs exits 0 through a pipe",
            "ok   - logs prints the warning start-up logged at the boot",
        ]
    );
}

#[test]
fn a_logs_that_needs_a_terminal_fails() {
    let found = logs_checks(
        "runtime-recovery-logs-terminal",
        logs_script("zbm: no terminal: not a terminal\r\nLOGS-STATUS=1\r\nRC=0\r\n"),
    );

    assert_eq!(
        found,
        [
            "FAIL - logs exits 0 through a pipe",
            "FAIL - logs prints the warning start-up logged at the boot",
        ]
    );
}
