use pretty_assertions::assert_eq;
use zbm_testkit::fakes::FakeSystem;

use super::*;

/// The arguments `args` as the applet gets them.
fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

/// Runs `zkexec` with `args`, which never reach a terminal: its status
/// and standard error.
fn call(args: &[&str]) -> (i32, String) {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = run(
        &strings(args),
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    assert!(stdout.is_empty());
    (status, String::from_utf8(stderr).unwrap())
}

/// A machine with the unencrypted boot environment `tank/ROOT/be`, whose
/// `/boot` holds a kernel and its initramfs.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool list -H -o feature@encryption tank", 0, "disabled\n");
    let boot = "/zfsbootmenu/environments/tank/ROOT/be/mnt/boot";
    fake.files
        .file(format!("{boot}/vmlinuz-6.1.10"), "kernel")
        .file(format!("{boot}/initramfs-6.1.10.img"), "initramfs");
    fake
}

/// Boots the kernel `args` name on `fake`, with the image's configuration:
/// the exit status, and what was said on standard error.
fn boot_on(fake: &FakeSystem, args: &[&str]) -> (i32, String) {
    fake.files.file(
        "/etc/zfsbootmenu.conf",
        "export BASE=\"/zfsbootmenu\"\nexport loglevel=\"7\"\n",
    );
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let kernel = files(&strings(args), &mut io).unwrap();
    let status = configured(fake.system(), &kernel, &mut io);
    assert!(stdout.is_empty());
    (status, String::from_utf8(stderr).unwrap())
}

#[test]
fn wrong_usage_prints_the_usage_and_exits_2() {
    for args in [
        &[][..],
        &["tank/ROOT/be"],
        &["tank/ROOT/be", "vmlinuz"],
        &["tank/ROOT/be", "vmlinuz", "initramfs.img", "more"],
    ] {
        assert_eq!(call(args), (USAGE, ZKEXEC.usage.to_owned()), "{args:?}");
    }
}

#[test]
fn a_name_that_is_no_boot_environment_is_said_escaped_and_exits_1() {
    let (status, stderr) = call(&["tank//\x1b[2J", "vmlinuz", "initramfs.img"]);
    assert_eq!(status, 1);
    assert!(stderr.starts_with("zkexec: tank//"), "{stderr:?}");
    assert!(stderr.ends_with(" is not the name of a boot environment\n"));
    assert!(!stderr.contains('\x1b'), "{stderr:?}");

    // A snapshot is not one: the key, the mount and the hooks would not
    // agree on what boots.
    let (status, stderr) = call(&["tank/ROOT/be@snap", "vmlinuz", "initramfs.img"]);
    assert_eq!(
        (status, stderr.as_str()),
        (
            1,
            "zkexec: tank/ROOT/be@snap is not the name of a boot environment\n"
        )
    );
}

#[test]
fn the_files_are_below_boot_of_the_filesystem() {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let args = strings(&["tank/ROOT/be", "vmlinuz-6.1.10", "initramfs-6.1.10.img"]);
    assert_eq!(
        files(&args, &mut io).unwrap(),
        Kernel::new(
            "tank/ROOT/be",
            "/boot/vmlinuz-6.1.10",
            "/boot/initramfs-6.1.10.img"
        )
    );

    let fake = machine();
    let _ = boot_on(
        &fake,
        &["tank/ROOT/be", "vmlinuz-6.1.10", "initramfs-6.1.10.img"],
    );

    let ran = fake.runner.lines();
    let load = ran
        .iter()
        .find(|line| line.starts_with("kexec -a -l"))
        .unwrap_or_else(|| panic!("{ran:#?}"));
    let mnt = "/zfsbootmenu/environments/tank/ROOT/be/mnt";
    assert!(
        load.contains(&format!("{mnt}/boot/vmlinuz-6.1.10"))
            && load.contains(&format!("{mnt}/boot/initramfs-6.1.10.img")),
        "{load}"
    );
    // The boot's messages reach the kernel log while it runs, as the
    // legacy script's did.
    let kmsg = fake.kmsg();
    let said = "|fs: tank/ROOT/be, kernel: /boot/vmlinuz-6.1.10";
    assert!(kmsg.contains(said), "{said} in {kmsg}");
}

/// Without the image's configuration nothing boots: it says so and exits
/// 1, as the legacy script exited when it could not source it, and opens
/// no shell.
#[test]
fn an_unreadable_configuration_exits_1() {
    let fake = machine();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    let kernel = Kernel::new("tank/ROOT/be", "/boot/vmlinuz-6.1.10", "/boot/x.img");

    let status = configured(fake.system(), &kernel, &mut io);

    assert_eq!(status, 1);
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(
        stderr.starts_with("zkexec: unable to read /etc/zfsbootmenu.conf"),
        "{stderr:?}"
    );
    assert!(fake.runner.lines().is_empty(), "{:#?}", fake.runner.lines());
}

#[test]
fn a_failed_boot_prints_why_and_exits_1() {
    let unmountable = machine();
    unmountable.runner.respond_program("mount", 32, "");

    let (status, stderr) = boot_on(
        &unmountable,
        &["tank/ROOT/be", "vmlinuz-6.1.10", "initramfs-6.1.10.img"],
    );

    assert_eq!(status, 1);
    assert_eq!(stderr, "zkexec: unable to mount tank/ROOT/be\n");

    // The fake's kexec -e returns.
    let (status, stderr) = boot_on(
        &machine(),
        &["tank/ROOT/be", "vmlinuz-6.1.10", "initramfs-6.1.10.img"],
    );
    assert_eq!(status, 1);
    assert_eq!(
        stderr,
        "zkexec: unable to boot tank/ROOT/be: the kernel could not be executed\n"
    );
}

#[test]
fn late_volumes_are_unlocked_as_the_menu_s_recovery_unlocks_them() {
    let fake = FakeSystem::new();
    let state = State::new(&fake.files, "/zfsbootmenu");
    let recovery = PrintRecovery {
        shell: Shell::new(fake.system(), &state),
        said: Mutex::default(),
    };

    // Without cryptsetup there is nothing to unlock, as for the menu.
    assert!(matches!(
        recovery.unlock_late(),
        Err(BootError::NoCryptsetup)
    ));
    recovery.emergency_shell("unable to mount \x1b[0;36mtank/ROOT/be\x1b[0m");
    assert_eq!(recovery.said(), ["unable to mount tank/ROOT/be"]);
}
