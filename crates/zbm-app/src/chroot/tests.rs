use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use pretty_assertions::assert_eq;
use zbm_platform::{Output, Status};
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::auth::Lock;

mod menu;
mod resolv;

/// Where the environment and its snapshot are mounted.
const MP: &str = "/zfsbootmenu/environments/tank/ROOT/be/mnt";
const SNAPSHOT_MP: &str = "/zfsbootmenu/environments/tank/ROOT/be@old/mnt";

/// A machine with `/bin/sh` in the environment `tank/ROOT/be` and in its
/// snapshot `@old`, whose pool is imported read-only when `readonly` is
/// `on`, read/write when it is `off`.
fn machine(readonly: &str) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files.dir("/zfsbootmenu");
    fake.runner.respond(
        "zpool get -H -o value readonly tank",
        0,
        &format!("{readonly}\n"),
    );
    for root in [MP, SNAPSHOT_MP] {
        fake.files
            .file_with_mode(format!("{root}/bin/sh"), "", 0o755);
    }
    fake
}

/// Makes `tank/ROOT/be` encrypted, its root `tank/ROOT` locked until
/// `zfs load-key` loads it, which it does when `loads` holds.
fn locked(fake: &FakeSystem, loads: bool) {
    let runner = &fake.runner;
    runner.respond("zpool list -H -o feature@encryption tank", 0, "active\n");
    runner.respond(
        "zfs get -H -o value encryptionroot tank/ROOT/be",
        0,
        "tank/ROOT\n",
    );
    runner.respond("zfs get -H -o value keylocation tank/ROOT", 0, "prompt\n");
    let loaded = Arc::new(AtomicBool::new(false));
    let load = Arc::clone(&loaded);
    runner.on(move |command| {
        (command.display() == "zfs load-key -L prompt tank/ROOT").then(|| {
            load.store(loads, Ordering::SeqCst);
            Ok(Output::new(Status::exited(u8::from(!loads)), ""))
        })
    });
    runner.on(move |command| {
        (command.display() == "zfs get -H -o value keystatus tank/ROOT").then(|| {
            let status = if loaded.load(Ordering::SeqCst) {
                "available\n"
            } else {
                "unavailable\n"
            };
            Ok(Output::new(Status::exited(0), status))
        })
    });
}

/// How the tests boot: the hooks of the image.
fn settings() -> BootSettings {
    BootSettings::default().hooks("/libexec/hooks")
}

/// Opens a shell in a chroot of `fs` on `fake`, without a lock.
fn open_on(fake: &FakeSystem, fs: &str) -> Result<i32, ChrootError> {
    let state = State::new(&fake.files, "/zfsbootmenu");
    let authority = Authority::new(Lock::Off);
    let fs = fs.parse().unwrap();
    open(&fake.system(), &state, &authority, &settings(), &fs)
}

/// What ran on `fake` that mounts, unmounts, imports, exports, loads a
/// key, or is a shell.
fn ran(fake: &FakeSystem) -> Vec<String> {
    const SHOWN: [&str; 6] = [
        "mount",
        "umount",
        "zpool import",
        "zpool export",
        "zfs load",
        "/bin/",
    ];
    fake.runner
        .lines()
        .into_iter()
        .filter(|line| SHOWN.iter().any(|start| line.starts_with(start)))
        .collect()
}

/// Whether every mount of `lines` is unmounted after it.
fn is_all_unmounted(lines: &[String]) -> bool {
    let mounted = lines
        .iter()
        .filter(|line| line.starts_with("mount") && !line.contains("remount"))
        .count();
    let unmounted = lines
        .iter()
        .filter(|line| line.starts_with("umount"))
        .count();
    // The root's recursive unmount takes the root alone.
    mounted == unmounted
}

/// Whether the question was asked on `fake`'s console.
fn is_asked(fake: &FakeSystem) -> bool {
    fake.console.output_text().contains("Entering chroot in")
}

#[test]
fn the_question_is_asked_only_for_a_read_only_pool_and_not_for_a_snapshot() {
    let read_only = machine("on");
    assert_eq!(open_on(&read_only, "tank/ROOT/be").unwrap(), 0);
    assert!(is_asked(&read_only));
    let shown = read_only.console.output_text();
    assert!(shown.contains("Enter r/w chroot"), "{shown}");
    assert!(shown.contains("Enter r/o chroot"), "{shown}");

    let snapshot = machine("on");
    open_on(&snapshot, "tank/ROOT/be@old").unwrap();
    assert!(!is_asked(&snapshot));

    let writable = machine("off");
    open_on(&writable, "tank/ROOT/be").unwrap();
    assert!(!is_asked(&writable));
}

#[test]
fn the_time_out_and_return_keep_the_chroot_read_only() {
    // The chroot asks through the console's interaction, which it makes
    // itself: no default answer, such as `NoInteraction`'s ESCAPE, can
    // make it read/write.
    for keys in [&b""[..], b"\n"] {
        let fake = machine("on");
        fake.console.keys(keys);
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        assert!(
            !lines.iter().any(|line| line.starts_with("zpool")),
            "{lines:?}"
        );
        assert!(lines.contains(&format!("mount -o zfsutil,ro -t zfs tank/ROOT/be {MP}")));
        let shown = decolorize(&fake.console.output_text());
        assert!(
            shown.contains("* tank/ROOT/be is mounted read-only\n"),
            "{shown}"
        );
    }
}

/// `fake`'s pool, imported read-only until `zpool import` succeeds, or
/// until never when `imports` does not hold.
fn imported_read_only(fake: &FakeSystem, imports: bool) {
    fake.runner.respond(
        "zpool get -H -o value org.zfsbootmenu:readonly tank",
        0,
        "-\n",
    );
    // No suspend partition.
    fake.runner.respond_program("blkid", 2, "");
    let imported = Arc::new(AtomicBool::new(false));
    let import = Arc::clone(&imported);
    fake.runner.on(move |command| {
        command.display().starts_with("zpool import").then(|| {
            import.store(imports, Ordering::SeqCst);
            Ok(Output::new(Status::exited(u8::from(!imports)), ""))
        })
    });
    fake.runner.first(move |command| {
        (command.display() == "zpool get -H -o value readonly tank").then(|| {
            let readonly = if imported.load(Ordering::SeqCst) {
                "off\n"
            } else {
                "on\n"
            };
            Ok(Output::new(Status::exited(0), readonly))
        })
    });
}

#[test]
fn escape_imports_the_pool_read_write_then_loads_the_key() {
    let fake = machine("on");
    imported_read_only(&fake, true);
    locked(&fake, true);
    fake.console.keys(b"\x1b");
    fake.console.secret(b"passphrase");

    open_on(&fake, "tank/ROOT/be").unwrap();

    let lines = ran(&fake);
    let at = |start: &str| lines.iter().position(|line| line.starts_with(start));
    let (export, import) = (at("zpool export tank"), at("zpool import"));
    let (key, mount) = (at("zfs load-key"), at("mount -o zfsutil,rw -t zfs"));
    assert!(export.is_some() && export < import, "{lines:#?}");
    assert!(import < key && key < mount, "{lines:#?}");
    let shown = decolorize(&fake.console.output_text());
    assert!(
        shown.contains("* tank/ROOT/be is mounted read/write\n"),
        "{shown}"
    );

    // A pool that cannot be imported read/write stays read-only, and so
    // does the chroot, as `zfs-chroot` left it.
    let fake = machine("on");
    imported_read_only(&fake, false);
    fake.console.keys(b"\x1b");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(lines.contains(&format!("mount -o zfsutil,ro -t zfs tank/ROOT/be {MP}")));
}

#[test]
fn a_locked_root_is_unlocked_before_the_chroot() {
    // Read-only too: RETURN at the question, then the key, loaded as for a
    // boot chosen in the menu, whose hooks know the default environment.
    let fake = machine("on");
    fake.files.file("/zfsbootmenu/bootfs", "tank/ROOT/be\n");
    locked(&fake, true);
    fake.console.keys(b"\n");
    fake.console.secret(b"passphrase");
    open_on(&fake, "tank/ROOT/be").unwrap_or_else(|error| panic!("{error}"));
    let lines = ran(&fake);
    assert_eq!(lines[0], "zfs load-key -L prompt tank/ROOT", "{lines:#?}");
    assert!(lines[1].starts_with("mount -o zfsutil,ro -t zfs tank/ROOT/be "));
    assert!(
        fake.console
            .output_text()
            .contains("Enter passphrase for 'tank/ROOT':"),
        "{}",
        fake.console.output_text()
    );

    // A key refused, or none given: nothing is mounted.
    for given in [None, Some(&b"wrong"[..])] {
        let fake = machine("off");
        locked(&fake, false);
        if let Some(key) = given {
            fake.console.secret(key);
        }
        let error = open_on(&fake, "tank/ROOT/be").unwrap_err();
        assert!(matches!(error, ChrootError::Key(_)), "{error:?}");
        assert_eq!(error.to_string(), "unable to load the key of tank/ROOT");
        let lines = ran(&fake);
        assert!(
            !lines.iter().any(|line| line.contains("mount")),
            "{lines:?}"
        );
    }
}

/// The machine of the mount tests: a writable pool, so no question, and
/// `/dev/pts` in the image's `/dev`.
fn mounting() -> FakeSystem {
    let fake = machine("off");
    fake.files.dir(format!("{MP}/dev/pts"));
    fake
}

/// The mounts of a chroot of `tank/ROOT/be`, read/write, below [`MP`].
fn legacy_mounts() -> Vec<String> {
    vec![
        format!("mount -o zfsutil,rw -t zfs tank/ROOT/be {MP}"),
        format!("mount -B /tmp {MP}/tmp"),
        format!("mount -t proc proc {MP}/proc"),
        format!("mount -t sysfs sys {MP}/sys"),
        format!("mount -B /dev {MP}/dev"),
        format!("mount -t devpts pts {MP}/dev/pts"),
    ]
}

#[test]
fn the_mounts_run_in_the_legacy_order() {
    let fake = mounting();
    // What the console and the runner saw, in order.
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let screens = Arc::clone(&seen);
    fake.console
        .on_screen(move |screen| screens.lock().unwrap().push(format!("{screen:?}")));
    let mounts = Arc::clone(&seen);
    fake.runner.on(move |command| {
        let line = command.display();
        if line.starts_with("mount ") {
            mounts.lock().unwrap().push(line);
        }
        None
    });
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines[..6], legacy_mounts());
    assert_eq!(lines[6..8], ["/bin/sh -c exit 0", "/bin/sh"]);
    // The screen is cleared, with the cursor on, before the first mount.
    let seen = seen.lock().unwrap();
    assert_eq!(seen[..3], ["Clear", "ShowCursor", &legacy_mounts()[0]]);
    let shown = decolorize(&fake.console.output_text());
    assert_eq!(
        shown,
        "* tank/ROOT/be is mounted read/write\n* /tmp is mounted read/write\n\n"
    );
}

#[test]
fn everything_is_unmounted_in_reverse_and_the_root_recursively() {
    let fake = mounting();
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(
        lines[8..],
        [
            format!("umount {MP}/dev/pts"),
            format!("umount {MP}/dev"),
            format!("umount {MP}/sys"),
            format!("umount {MP}/proc"),
            format!("umount {MP}/tmp"),
            format!("umount -R {MP}"),
        ]
    );
}

#[test]
fn devpts_is_mounted_only_where_the_dev_bind_has_dev_pts() {
    let without = machine("off");
    open_on(&without, "tank/ROOT/be").unwrap();
    let lines = ran(&without);
    assert!(
        !lines.iter().any(|line| line.contains("devpts")),
        "{lines:?}"
    );
    assert!(is_all_unmounted(&lines), "{lines:#?}");

    let with = mounting();
    open_on(&with, "tank/ROOT/be").unwrap();
    assert!(ran(&with).contains(&format!("mount -t devpts pts {MP}/dev/pts")));
}

#[test]
fn a_snapshot_and_a_read_only_pool_are_mounted_read_only() {
    let snapshot = machine("off");
    open_on(&snapshot, "tank/ROOT/be@old").unwrap();
    assert_eq!(
        ran(&snapshot)[0],
        format!("mount -o ro -t zfs tank/ROOT/be@old {SNAPSHOT_MP}")
    );
    let shown = decolorize(&snapshot.console.output_text());
    assert!(
        shown.contains("* tank/ROOT/be@old is mounted read-only\n"),
        "{shown}"
    );

    let read_only = machine("on");
    open_on(&read_only, "tank/ROOT/be").unwrap();
    assert_eq!(
        ran(&read_only)[0],
        format!("mount -o zfsutil,ro -t zfs tank/ROOT/be {MP}")
    );
}

/// `fake` booted with UEFI, its `efivarfs` mounted.
fn efi(fake: &FakeSystem) {
    fake.files.dir("/sys/firmware/efi");
    fake.files.file(
        "/proc/self/mounts",
        "efivarfs /sys/firmware/efi/efivars efivarfs ro 0 0\n",
    );
}

#[test]
fn on_efi_the_host_efivars_follow_the_mode_and_go_back_to_read_only() {
    for (readonly, mode, said) in [("off", "rw", "read/write"), ("on", "ro", "read-only")] {
        let fake = machine(readonly);
        efi(&fake);
        open_on(&fake, "tank/ROOT/be").unwrap();
        let lines = ran(&fake);
        let host = "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o";
        let inside = format!("{MP}/sys/firmware/efi/efivars");
        let efivarfs = [
            format!("{host} remount,{mode}"),
            format!("mount -t efivarfs efivarfs {inside} -o {mode}"),
        ];
        assert_eq!(lines[5..7], efivarfs, "{lines:#?}");
        assert!(lines.contains(&format!("umount {inside}")));
        assert_eq!(lines.last(), Some(&format!("{host} remount,ro")));
        assert_eq!(lines[lines.len() - 2], format!("umount -R {MP}"));
        let shown = decolorize(&fake.console.output_text());
        assert!(
            shown.contains(&format!("* efivarfs is mounted {said}\n")),
            "{shown}"
        );
    }

    // The host's set to the mode, but none mounted in the chroot: it is
    // set back all the same, and no notice says one is mounted.
    let fake = machine("off");
    efi(&fake);
    let inside = format!("mount -t efivarfs efivarfs {MP}/sys/firmware/efi/efivars -o rw");
    fake.runner.respond(&inside, 32, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert!(
        lines
            .last()
            .is_some_and(|line| line.ends_with("remount,ro"))
    );
    assert!(!fake.console.output_text().contains("efivarfs"));
}

#[test]
fn on_bios_no_efivarfs_is_touched() {
    let bios = machine("off");
    open_on(&bios, "tank/ROOT/be").unwrap();
    // On EFI, an efivarfs that cannot be remounted is left alone.
    let refused = machine("off");
    efi(&refused);
    refused.runner.respond(
        "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw",
        32,
        "",
    );
    open_on(&refused, "tank/ROOT/be").unwrap();
    let lines = ran(&bios);
    assert!(
        !lines.iter().any(|line| line.contains("efivar")),
        "{lines:?}"
    );
    let lines = ran(&refused);
    let efivarfs: Vec<_> = lines
        .iter()
        .filter(|line| line.contains("efivar"))
        .collect();
    assert_eq!(efivarfs.len(), 1, "{lines:#?}");
}

#[test]
fn on_efi_an_efivarfs_not_mounted_is_mounted_in_the_mode_and_left_read_only() {
    let fake = machine("on");
    fake.files.dir("/sys/firmware/efi");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    let host = "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o";
    let inside = format!("{MP}/sys/firmware/efi/efivars");
    let efivarfs = [
        format!("{host} ro"),
        format!("mount -t efivarfs efivarfs {inside} -o ro"),
    ];
    assert_eq!(lines[5..7], efivarfs, "{lines:#?}");
    assert_eq!(lines.last(), Some(&format!("{host} remount,ro")));
    let shown = decolorize(&fake.console.output_text());
    assert!(
        shown.contains("* efivarfs is mounted read-only\n"),
        "{shown}"
    );
}

#[test]
fn the_first_shell_that_runs_in_the_chroot_is_taken() {
    let fake = machine("off");
    fake.files
        .file_with_mode(format!("{MP}/bin/bash"), "", 0o755)
        .file_with_mode(format!("{MP}/bin/busybox"), "", 0o755);
    fake.runner.respond("/bin/bash -c exit 0", 126, "");
    fake.runner.respond("/bin/sh -c exit 0", 127, "");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let shells: Vec<String> = ran(&fake)
        .into_iter()
        .filter(|line| line.starts_with("/bin/"))
        .collect();
    assert_eq!(
        shells,
        [
            "/bin/bash -c exit 0",
            "/bin/sh -c exit 0",
            "/bin/busybox sh -c exit 0",
            "/bin/busybox sh"
        ]
    );
}

#[test]
fn no_shell_is_an_error_and_leaves_nothing_mounted() {
    let fake = machine("off");
    fake.runner.respond("/bin/sh -c exit 0", 127, "");
    let error = open_on(&fake, "tank/ROOT/be").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unable to test execute a shell in tank/ROOT/be"
    );
    let lines = ran(&fake);
    assert!(is_all_unmounted(&lines), "{lines:#?}");
    assert!(
        lines
            .last()
            .is_some_and(|line| line.starts_with("umount -R"))
    );
    // Nothing is said of what was mounted.
    assert!(!fake.console.output_text().contains("is mounted"));

    // A root the shell cannot enter ends the search at once.
    let fake = machine("off");
    fake.files
        .file_with_mode(format!("{MP}/bin/busybox"), "", 0o755);
    fake.runner.on(|command| {
        command
            .get_root()
            .is_some()
            .then(|| Err(io::Error::other("cannot change root directory")))
    });
    let error = open_on(&fake, "tank/ROOT/be").unwrap_err();
    assert!(matches!(error, ChrootError::Start(_)), "{error:?}");
    let lines = ran(&fake);
    let shells = lines
        .iter()
        .filter(|line| line.starts_with("/bin/"))
        .count();
    assert_eq!(shells, 1, "{lines:#?}");
    assert!(is_all_unmounted(&lines), "{lines:#?}");
}

#[test]
fn a_filesystem_that_does_not_mount_opens_nothing() {
    let fake = machine("off");
    fake.runner.respond(
        &format!("mount -o zfsutil,rw -t zfs tank/ROOT/be {MP}"),
        32,
        "",
    );
    let error = open_on(&fake, "tank/ROOT/be").unwrap_err();
    assert_eq!(error.to_string(), "failed to mount tank/ROOT/be");
    assert_eq!(ran(&fake).len(), 1, "{:?}", ran(&fake));
}

/// Strips the colours of `text`.
fn decolorize(text: &str) -> String {
    zbm_tools::console::decolorize(text)
}
