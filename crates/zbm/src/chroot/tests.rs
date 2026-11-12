use std::time::Duration;

use pretty_assertions::assert_eq;
use zbm_app::auth::Lock;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::lock::tests::{Checking, locked};

/// The PIN of the locked image of the tests.
const PIN: &[u8] = b"correct horse";

/// A machine whose pool `tank` is imported read/write, so that no
/// question is asked, with `/bin/sh` in `tank/ROOT/be`.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");
    fake.files.file_with_mode(
        "/zfsbootmenu/environments/tank/ROOT/be/mnt/bin/sh",
        "",
        0o755,
    );
    fake
}

/// Opens a chroot of `tank/ROOT/be` on `fake` as `authority` allows:
/// the exit status, and what was said on standard error.
fn open_on(fake: &FakeSystem, authority: &Authority) -> (i32, String) {
    let state = State::new(&fake.files, "/zfsbootmenu");
    let keyholders = Checking { pin: PIN };
    let fs = "tank/ROOT/be".parse().unwrap();
    let settings = BootSettings::default();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = open(
        &fake.system(),
        &state,
        (authority, &keyholders),
        &settings,
        &fs,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    assert!(stdout.is_empty());
    (status, String::from_utf8(stderr).unwrap())
}

/// What ran on `fake` that mounts or is a shell.
fn mounts_and_shells(fake: &FakeSystem) -> Vec<String> {
    fake.runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("mount") || line.starts_with("/bin/"))
        .collect()
}

/// Runs `zfs-chroot` with `args`, which never reach a terminal: its
/// status and standard error.
fn call(args: &[&str]) -> (i32, String) {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let status = run(
        &args,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    assert!(stdout.is_empty());
    (status, String::from_utf8(stderr).unwrap())
}

#[test]
fn a_locked_image_asks_for_the_pin_before_the_chroot() {
    let fake = machine();
    fake.console.secret(b"wrong pin").secret(PIN);
    let authority = locked();

    let (status, stderr) = open_on(&fake, &authority);

    assert_eq!((status, stderr.as_str()), (0, ""));
    let shown = fake.console.output_text();
    assert!(shown.starts_with("This ZFSBootMenu is locked."), "{shown}");
    assert_eq!(shown.matches("PIN: ").count(), 2, "{shown}");
    let ran = mounts_and_shells(&fake);
    assert!(ran.iter().any(|line| line.starts_with("mount")), "{ran:?}");
    assert_eq!(ran.last().map(String::as_str), Some("/bin/sh"), "{ran:?}");
    // The chroot forgot the proof: the next one asks again.
    assert!(authority.is_locked(false, Duration::ZERO));
}

#[test]
fn a_wrong_pin_opens_nothing() {
    let fake = machine();
    for pin in ["a", "b", "c", "correct horse"] {
        fake.console.secret(pin.as_bytes());
    }

    let (status, stderr) = open_on(&fake, &locked());

    assert_eq!((status, stderr.as_str()), (1, ""));
    assert_eq!(fake.runner.lines(), Vec::<String>::new());
    let shown = fake.console.output_text();
    assert!(shown.ends_with("Back to ZFSBootMenu.\n"), "{shown}");
}

#[test]
fn an_attended_unlock_needs_no_pin() {
    let fake = machine();
    State::new(&fake.files, "/zfsbootmenu")
        .record_unlocked("home", zbm_state::Unlocked::Attended)
        .unwrap();

    let (status, _) = open_on(&fake, &locked());

    assert_eq!(status, 0);
    assert!(!fake.console.output_text().contains("PIN: "));
    let ran = mounts_and_shells(&fake);
    assert_eq!(ran.last().map(String::as_str), Some("/bin/sh"), "{ran:?}");
}

#[test]
fn a_filesystem_without_a_shell_exits_1_and_says_why() {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner
        .respond("zpool get -H -o value readonly tank", 0, "off\n");

    let (status, stderr) = open_on(&fake, &Authority::new(Lock::Off));

    assert_eq!(status, 1);
    assert_eq!(
        stderr,
        "zfs-chroot: unable to test execute a shell in tank/ROOT/be\n"
    );
}

#[test]
fn a_shell_that_cannot_start_says_its_cause() {
    let error = ChrootError::Start(io::Error::other("no \x1b[31mroot"));
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());

    say(
        &error,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );

    let said = String::from_utf8(stderr).unwrap();
    assert!(
        said.starts_with("zfs-chroot: unable to start a shell in the chroot\n  no "),
        "{said}"
    );
    assert!(!said.contains('\x1b'), "{said:?}");
}

/// This machine's image is not locked: the chroot opens at once, with
/// the image's configuration.
#[test]
fn this_process_opens_the_chroot_with_the_image_s_configuration() {
    let fake = machine();
    fake.files.file(
        conf::PATH,
        "export BASE=\"/zfsbootmenu\"\nexport loglevel=\"7\"\n",
    );
    let fs = "tank/ROOT/be".parse().unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());

    let status = open_here(
        &fake.system(),
        &fs,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );

    assert_eq!(status, 0, "{}", String::from_utf8_lossy(&stderr));
    let ran = mounts_and_shells(&fake);
    assert_eq!(ran.last().map(String::as_str), Some("/bin/sh"), "{ran:?}");
    // Its messages, and the chroot's, reach the kernel log while the
    // chroot runs, as the legacy script's did.
    let kmsg = fake.kmsg();
    for said in [
        "|zfs-chroot started with tank/ROOT/be\n",
        "|mounting tank/ROOT/be at /zfsbootmenu/environments/tank/ROOT/be/mnt",
    ] {
        assert!(kmsg.contains(said), "{said} in {kmsg}");
    }
}

#[test]
fn zfs_chroot_binds_the_image_s_resolv_conf() {
    let fake = machine();
    let target = "/zfsbootmenu/environments/tank/ROOT/be/mnt/etc/resolv.conf";
    fake.files
        .file("/etc/resolv.conf", "nameserver 10.0.2.3\n")
        .file(target, "");

    let (status, _) = open_on(&fake, &Authority::new(Lock::Off));

    assert_eq!(status, 0);
    let ran = mounts_and_shells(&fake);
    assert_eq!(
        ran[1..3],
        [
            format!("mount -B /etc/resolv.conf {target}"),
            format!("mount -o remount,bind,ro {target}"),
        ]
    );
    let shown = zbm_platform::color::decolorize(&fake.console.output_text());
    assert!(
        shown.contains("* /etc/resolv.conf is mounted read-only\n"),
        "{shown}"
    );
}

/// The signals that would end it with the chroot mounted are the legacy
/// script's traps, `SIGABRT` aside, and the default signal of `kill`.
#[test]
fn the_held_signals_are_those_the_legacy_script_trapped() {
    let held: Vec<Signal> = HELD.iter().collect();
    assert_eq!(
        held,
        [
            Signal::Hangup,
            Signal::Interrupt,
            Signal::Quit,
            Signal::Terminate
        ]
    );
}

#[test]
fn no_terminal_exits_1() {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());

    let status = no_terminal(
        &io::Error::other("standard input is closed"),
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );

    assert_eq!(status, 1);
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "zfs-chroot: no terminal to open a shell on: standard input is closed\n"
    );
}

#[test]
fn no_filesystem_exits_0_as_the_legacy_script_did() {
    assert_eq!(call(&[]), (0, String::new()));
    assert_eq!(call(&[""]), (0, String::new()));
}

#[test]
fn a_second_argument_is_a_usage_error_and_a_bad_name_exits_1() {
    assert_eq!(call(&["tank/a", "b"]), (USAGE, ZFS_CHROOT.usage.to_owned()));
    let (status, stderr) = call(&["tank//\x1b[2J"]);
    assert_eq!(status, 1);
    assert!(stderr.starts_with("zfs-chroot: tank//"), "{stderr:?}");
    assert!(!stderr.contains('\x1b'), "{stderr:?}");
}
