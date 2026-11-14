//! A filesystem mounted on its own path in the state directory, never
//! through a link or another mount on the way, the state directory
//! included, nor over another filesystem (SB19); and the recursive
//! unmount, deepest first.

use std::io;
use std::path::Path;

use zbm_platform::{Context, Installed, KernelLog};
use zbm_state::State;
use zbm_testkit::fakes::FakeSystem;

use super::*;
use crate::testing::name;

/// The state directory.
const BASE: &str = "/zfsbootmenu";
/// The directory of every filesystem's state.
const E: &str = "/zfsbootmenu/environments";
/// Where `tank/a` is mounted.
const MP: &str = "/zfsbootmenu/environments/tank/a/mnt";

/// A machine with the state directory and its kernel log, where error
/// lines go.
fn machine() -> (FakeSystem, Installed) {
    let fake = FakeSystem::new();
    fake.files.dir(BASE).dir("/etc");
    let log = KernelLog::new(&fake.files, "7", Path::new(BASE), 1)
        .without_journal()
        .install();
    (fake, log)
}

/// The kernel's line for a mount `id` of a `kind` from `source` on
/// `point`, escaped as written.
fn line(id: u64, point: &str, kind: &str, source: &str) -> String {
    format!("{id} 25 0:{id} / {point} rw,relatime shared:1 - {kind} {source} rw\n")
}

/// Sets `fake`'s mountinfo to `lines`, the image's root first.
fn table(fake: &FakeSystem, lines: &[String]) {
    let mut text = "25 1 0:2 / / rw - rootfs rootfs rw\n".to_owned();
    for line in lines {
        text.push_str(line);
    }
    fake.files.file(MOUNTINFO, text);
}

/// Mounts `fs` read-only on `fake`; whether it is mounted.
fn mounts_ro(fake: &FakeSystem, fs: &str) -> bool {
    let state = State::new(&fake.files, BASE);
    let context = Context::new(fake.system());
    mount_zfs(&context, &state, &name(fs), Noise::Hidden)
        .map(Mounted::leave)
        .is_some()
}

/// Whether anything ran on `fake` that mounts.
fn has_mounted(fake: &FakeSystem) -> bool {
    fake.runner
        .lines()
        .iter()
        .any(|line| line.starts_with("mount"))
}

/// Whether `fake`'s error lines say that `fs` is not mounted, and `why`.
fn says(fake: &FakeSystem, fs: &str, why: &str) -> bool {
    fake.kmsg().contains(&format!("{fs} is not mounted: {why}"))
}

#[test]
fn a_state_path_through_a_link_is_not_mounted_and_says_why() {
    let (fake, log) = machine();
    fake.files.symlink(format!("{E}/tank"), "/etc\x1b[2J");

    assert!(!mounts_ro(&fake, "tank/a"));

    drop(log);
    assert!(!has_mounted(&fake), "{:#?}", fake.runner.lines());
    // The link's target is escaped.
    let why = format!("{E}/tank is a link to /etc\\x1b[2J");
    assert!(says(&fake, "tank/a", &why), "{}", fake.kmsg());
    let below_etc = fake.files.paths();
    let below_etc: Vec<_> = below_etc
        .iter()
        .filter(|path| path.starts_with("/etc") && *path != Path::new("/etc"))
        .collect();
    assert!(below_etc.is_empty(), "{below_etc:#?}");
}

#[test]
fn a_state_path_through_a_mount_is_not_mounted() {
    let (fake, log) = machine();
    table(&fake, &[line(40, &format!("{E}/tank"), "tmpfs", "x")]);

    assert!(!mounts_ro(&fake, "tank/a"));

    drop(log);
    assert!(!has_mounted(&fake));
    assert!(says(&fake, "tank/a", &format!("{E}/tank is a mount point")));
}

#[test]
fn a_dataset_mounted_on_the_environments_directory_is_an_obstacle() {
    // A dataset whose `mountpoint` property is the environments'
    // directory, mounted there by `zfs mount -a` in a recovery shell.
    let (fake, log) = machine();
    table(&fake, &[line(40, E, "zfs", "evil/x")]);

    assert!(!mounts_ro(&fake, "tank/a"));

    drop(log);
    assert!(!has_mounted(&fake));
    assert!(says(&fake, "tank/a", &format!("{E} is a mount point")));
}

#[test]
fn a_dataset_mounted_on_the_state_directory_is_an_obstacle() {
    // A dataset whose `mountpoint` property is the state directory: the
    // environments' paths would be plain directories in its root.
    let (fake, log) = machine();
    table(&fake, &[line(40, BASE, "zfs", "evil/x")]);

    assert!(!mounts_ro(&fake, "tank/a"));

    drop(log);
    assert!(!has_mounted(&fake));
    assert!(says(&fake, "tank/a", &format!("{BASE} is a mount point")));
}

#[test]
fn a_state_directory_that_is_a_link_is_an_obstacle() {
    let fake = FakeSystem::new();
    fake.files.dir("/etc").symlink(BASE, "/etc");
    table(&fake, &[]);

    assert!(!mounts_ro(&fake, "tank/a"));

    assert!(!has_mounted(&fake));
    let below_etc = fake.files.paths();
    let below_etc: Vec<_> = below_etc
        .iter()
        .filter(|path| path.starts_with("/etc") && *path != Path::new("/etc"))
        .collect();
    assert!(below_etc.is_empty(), "{below_etc:#?}");
    let state = State::new(&fake.files, BASE);
    assert_eq!(
        refusal(&fake.files, &state, &name("tank/a"), Path::new(MP)),
        Some(format!("{BASE} is a link to /etc")),
    );
}

#[test]
fn a_state_directory_that_cannot_be_read_is_an_obstacle() {
    let fake = FakeSystem::new();
    fake.files.fail(BASE, io::ErrorKind::PermissionDenied);
    table(&fake, &[]);
    let state = State::new(&fake.files, BASE);

    let why = refusal(&fake.files, &state, &name("tank/a"), Path::new(MP));

    assert!(why.is_some_and(|why| why.contains("denied")));
}

#[test]
fn a_missing_state_directory_is_no_obstacle() {
    // Its directories are made, with no link on the way, before the mount.
    let fake = FakeSystem::new();
    table(&fake, &[]);
    let state = State::new(&fake.files, BASE);

    assert_eq!(
        refusal(&fake.files, &state, &name("tank/a"), Path::new(MP)),
        None
    );
}

#[test]
fn a_foreign_mount_on_the_path_itself_is_not_covered() {
    // A hook's `tmpfs`, another dataset, or one that only names the
    // filesystem as its source.
    for (kind, source) in [("tmpfs", "x"), ("zfs", "tank/b"), ("tmpfs", "tank/a")] {
        let (fake, log) = machine();
        table(&fake, &[line(40, MP, kind, source)]);

        assert!(!mounts_ro(&fake, "tank/a"), "{kind} {source}");

        drop(log);
        assert!(!has_mounted(&fake), "{kind} {source}");
        assert!(says(&fake, "tank/a", &format!("{MP} is a mount point")));
    }
}

#[test]
fn a_filesystem_mounted_on_its_own_path_is_mounted_again() {
    // As the boot path leaves it before the emergency shell.
    let (fake, _log) = machine();
    table(&fake, &[line(40, MP, "zfs", "tank/a")]);

    assert!(mounts_ro(&fake, "tank/a"));

    let mount = format!("mount -o zfsutil,ro -t zfs tank/a {MP}");
    assert!(
        fake.runner.lines().contains(&mount),
        "{:#?}",
        fake.runner.lines()
    );
}

#[test]
fn a_table_that_cannot_be_read_mounts_nothing() {
    let (fake, log) = machine();
    fake.files.fail(MOUNTINFO, io::ErrorKind::PermissionDenied);

    assert!(!mounts_ro(&fake, "tank/a"));

    drop(log);
    assert!(!has_mounted(&fake));
    let why = format!("{MOUNTINFO} could not be read");
    assert!(says(&fake, "tank/a", &why), "{}", fake.kmsg());
}

#[test]
fn a_snapshot_s_state_path_is_checked_too() {
    let (fake, log) = machine();
    fake.files.symlink(format!("{E}/tank/a@s"), "/etc");

    assert!(!mounts_ro(&fake, "tank/a@s"));

    drop(log);
    assert!(!has_mounted(&fake));
    let why = format!("{E}/tank/a@s is a link to /etc");
    assert!(says(&fake, "tank/a@s", &why), "{}", fake.kmsg());
    // Writable mounts are checked alike.
    let state = State::new(&fake.files, BASE);
    let context = Context::new(fake.system());
    fake.files.symlink(format!("{E}/tank/b"), "/etc");
    let dataset = name("tank/b");
    assert!(mount_zfs_writable(&context, &state, &dataset, Noise::Shown).is_none());
    assert!(!has_mounted(&fake));
}

#[test]
fn nothing_is_created_before_the_check() {
    let fake = FakeSystem::new();
    fake.files.dir(BASE);
    table(&fake, &[line(40, &format!("{E}/tank"), "tmpfs", "x")]);

    assert!(!mounts_ro(&fake, "tank/a"));

    assert_eq!(fake.files.operations(), []);
}

#[test]
fn a_mount_point_outside_the_state_directory_is_refused() {
    // The state's paths are always below it; the check holds all the same.
    let fake = FakeSystem::new();
    let state = State::new(&fake.files, BASE);
    let fs = name("tank/a");
    for at in ["/mnt", "/zfsbootmenu/../etc/mnt", "tank/a/mnt"] {
        assert_eq!(
            refusal(&fake.files, &state, &fs, Path::new(at)),
            Some(format!("{at} is not below {BASE}")),
        );
    }
}

/// The unmounts run on `fake`.
fn unmounts(fake: &FakeSystem) -> Vec<String> {
    fake.runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("umount"))
        .collect()
}

#[test]
fn recursive_unmounts_go_deepest_first() {
    let (fake, _log) = machine();
    table(
        &fake,
        &[
            line(40, "/mnt/be", "zfs", "tank/a"),
            line(41, "/mnt/be/usr", "zfs", "tank/a/b"),
            line(42, "/mnt/be/usr/local", "zfs", "tank/a/c"),
            line(43, "/mnt/other", "zfs", "tank/d"),
            line(44, "/mnt/bee", "zfs", "tank/e"),
        ],
    );
    fake.runner.respond("umount -R /mnt/be", 32, "");
    fake.runner.respond("umount /mnt/be/usr", 32, "");

    let error = recursive_umount(&fake.context(), Path::new("/mnt/be")).unwrap_err();

    // Names are compared whole: `/mnt/bee` is not below `/mnt/be`.
    assert_eq!(
        unmounts(&fake),
        [
            "umount -R /mnt/be",
            "umount /mnt/be/usr/local",
            "umount /mnt/be/usr",
            "umount /mnt/be",
        ]
    );
    assert!(fake.kmsg().contains("unable to unmount /mnt/be/usr"));
    let BootEnvError::Unmount { mounts } = error else {
        panic!("not an unmount: {error:?}");
    };
    assert_eq!(mounts, ["/mnt/be/usr"]);
}

#[test]
fn the_recursive_unmount_s_fallback_takes_no_sibling_whose_name_starts_alike() {
    // The snapshot of a child named `mnt`, and a child `mnt x`, are other
    // datasets' mounts, which a front end may be using.
    let (fake, _log) = machine();
    table(
        &fake,
        &[
            line(40, MP, "zfs", "tank/a"),
            line(41, &format!("{MP}@s/mnt"), "zfs", "tank/a/mnt@s"),
            line(
                42,
                &format!("{E}/tank/a/mnt\\040x/mnt"),
                "zfs",
                "tank/a/mnt\\040x",
            ),
        ],
    );
    fake.runner.respond(&format!("umount -R {MP}"), 32, "");

    recursive_umount(&fake.context(), Path::new(MP)).unwrap();

    assert_eq!(
        unmounts(&fake),
        [format!("umount -R {MP}"), format!("umount {MP}")]
    );
}

#[test]
fn the_recursive_unmount_s_fallback_finds_a_name_with_a_space() {
    // The kernel writes the space `\040`; the path is unmounted as it is.
    const SPACED: &str = "/zfsbootmenu/environments/tank/my be/mnt";
    let (fake, _log) = machine();
    table(
        &fake,
        &[
            line(
                40,
                "/zfsbootmenu/environments/tank/my\\040be/mnt",
                "zfs",
                "tank/my\\040be",
            ),
            line(
                41,
                "/zfsbootmenu/environments/tank/my\\040be/mnt/usr",
                "zfs",
                "tank/u",
            ),
        ],
    );
    fake.runner.respond(&format!("umount -R {SPACED}"), 32, "");

    recursive_umount(&fake.context(), Path::new(SPACED)).unwrap();

    assert_eq!(
        unmounts(&fake),
        [
            format!("umount -R {SPACED}"),
            format!("umount {SPACED}/usr"),
            format!("umount {SPACED}"),
        ]
    );
}
