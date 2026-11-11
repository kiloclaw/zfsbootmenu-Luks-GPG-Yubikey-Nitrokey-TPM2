use pretty_assertions::assert_eq;
use zbm_platform::{Input, Stream};
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeFiles;

use super::*;

/// The mount point of the filesystem the tests chroot into.
const ROOT: &str = "/zfsbootmenu/environments/tank/ROOT/void/mnt";

/// `path` below [`ROOT`].
fn below(path: &str) -> std::path::PathBuf {
    Path::new(ROOT).join(path)
}

#[test]
fn each_special_mount_runs_mount_with_its_type_and_options() {
    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    let mount = Mount::new(&runner, &files);
    let tmp = mount.bind(Path::new("/tmp"), &below("tmp")).unwrap();
    let proc = mount.proc(&below("proc")).unwrap();
    let sys = mount.sysfs(&below("sys")).unwrap();
    let pts = mount.devpts(&below("dev/pts")).unwrap();
    let efivars = mount
        .efivarfs_in(&below("sys/firmware/efi/efivars"), "ro")
        .unwrap();
    assert_eq!(tmp.path(), below("tmp"));
    for mounted in [tmp, proc, sys, pts, efivars] {
        let _ = mounted.leave();
    }
    assert_eq!(
        runner.lines(),
        [
            format!("mount -B /tmp {ROOT}/tmp"),
            format!("mount -t proc proc {ROOT}/proc"),
            format!("mount -t sysfs sys {ROOT}/sys"),
            format!("mount -t devpts pts {ROOT}/dev/pts"),
            format!("mount -t efivarfs efivarfs {ROOT}/sys/firmware/efi/efivars -o ro"),
        ]
    );
    // Their messages show on the console, as the legacy script's did.
    for call in runner.calls() {
        assert_eq!(call.get_stdout(), Stream::Inherit, "{}", call.display());
        assert_eq!(call.get_stderr(), Stream::Inherit, "{}", call.display());
    }
}

#[test]
fn a_failed_mount_returns_its_code_and_mounts_nothing() {
    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    runner.respond_program("mount", 32, "");
    let mount = Mount::new(&runner, &files);
    let target = below("dev");
    assert_eq!(mount.bind(Path::new("/dev"), &target).err(), Some(32));
    assert_eq!(mount.proc(&below("proc")).err(), Some(32));
    assert_eq!(mount.sysfs(&below("sys")).err(), Some(32));
    assert_eq!(mount.devpts(&below("dev/pts")).err(), Some(32));
    assert_eq!(
        mount
            .efivarfs_in(&below("sys/firmware/efi/efivars"), "rw")
            .err(),
        Some(32)
    );
    assert_eq!(HostEfivars::remount(mount, "rw").err(), Some(32));
    // Nothing was mounted, so nothing is unmounted, nor set back.
    let lines = runner.lines();
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert!(
        lines.iter().all(|line| line.starts_with("mount ")),
        "{lines:?}"
    );
}

#[test]
fn guards_unmount_in_reverse_and_the_root_recursively() {
    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    let mount = Mount::new(&runner, &files);
    let mounting = {
        let _root = mount
            .zfs("tank/ROOT/void", Path::new(ROOT), true, Shown::Hidden)
            .unwrap()
            .recursive();
        let _tmp = mount.bind(Path::new("/tmp"), &below("tmp")).unwrap();
        let _proc = mount.proc(&below("proc")).unwrap();
        let _sys = mount.sysfs(&below("sys")).unwrap();
        let _dev = mount.bind(Path::new("/dev"), &below("dev")).unwrap();
        let _host = HostEfivars::remount(mount, "rw").unwrap();
        let _efivars = mount
            .efivarfs_in(&below("sys/firmware/efi/efivars"), "rw")
            .unwrap();
        let _pts = mount.devpts(&below("dev/pts")).unwrap();
        runner.lines().len()
    };
    assert_eq!(
        runner.lines()[mounting..],
        [
            format!("umount {ROOT}/dev/pts"),
            format!("umount {ROOT}/sys/firmware/efi/efivars"),
            "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,ro".to_owned(),
            format!("umount {ROOT}/dev"),
            format!("umount {ROOT}/sys"),
            format!("umount {ROOT}/proc"),
            format!("umount {ROOT}/tmp"),
            format!("umount -R {ROOT}"),
        ]
    );

    // Unmounted at once, the root goes with everything below it too.
    let root = mount
        .zfs("tank/ROOT/void", Path::new(ROOT), true, Shown::Hidden)
        .unwrap()
        .recursive();
    assert!(root.unmount());
    assert_eq!(runner.lines().last(), Some(&format!("umount -R {ROOT}")));
}

#[test]
fn an_inner_guard_ignores_a_target_already_unmounted() {
    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    let mount = Mount::new(&runner, &files);
    let tmp = mount.bind(Path::new("/tmp"), &below("tmp")).unwrap();
    let proc = mount.proc(&below("proc")).unwrap();
    // The user unmounted /tmp in the chroot.
    runner.respond(&format!("umount {ROOT}/tmp"), 32, "");
    drop(proc);
    drop(tmp);
    assert_eq!(
        runner.lines()[2..],
        [format!("umount {ROOT}/proc"), format!("umount {ROOT}/tmp")]
    );
    // Quietly: `umount` says nothing on the console.
    for call in &runner.calls()[2..] {
        assert_eq!(*call.get_stdin(), Input::Null);
        assert_eq!(call.get_stdout(), Stream::Null);
        assert_eq!(call.get_stderr(), Stream::Null);
    }
}

#[test]
fn the_host_efivars_go_back_to_read_only_when_dropped() {
    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    let mount = Mount::new(&runner, &files);
    let host = HostEfivars::remount(mount, "rw").unwrap();
    assert_eq!(
        runner.lines(),
        ["mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw"]
    );
    drop(host);
    assert_eq!(
        runner.lines()[1],
        "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,ro"
    );
    assert_eq!(runner.calls()[1].get_stdout(), Stream::Inherit);
}
