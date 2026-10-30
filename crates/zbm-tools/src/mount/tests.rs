use pretty_assertions::assert_eq;
use zbm_testkit::FakeRunner;
use zbm_testkit::fakes::FakeFiles;

use super::*;

#[test]
fn without_system_calls_the_programs_run_with_the_legacy_command_lines() {
    let runner = FakeRunner::new();
    let files = FakeFiles::new();
    let mount = Mount::new(&runner, &files);
    let mnt = Path::new("/zfsbootmenu/mnt");

    mount
        .zfs("tank/ROOT/void", mnt, true, Shown::Hidden)
        .unwrap()
        .leave();
    mount
        .zfs("tank/ROOT/void@a", mnt, false, Shown::Console)
        .unwrap()
        .leave();
    mount
        .read_only("/dev/sdb1", Path::new("/tmp/luks-key.1-0"))
        .unwrap()
        .leave();
    mount
        .read_only_shown("tank/hooks", Path::new("/.external_hooks"))
        .unwrap()
        .leave();
    assert!(mount.efivarfs("/sys/firmware/efi/efivars", "remount,rw"));
    assert!(mount.unmount(mnt, Shown::Console));
    assert!(mount.unmount(mnt, Shown::Hidden));
    assert!(mount.unmount_recursive(mnt));

    assert_eq!(
        runner.lines(),
        [
            "mount -o zfsutil,ro -t zfs tank/ROOT/void /zfsbootmenu/mnt",
            "mount -o ro -t zfs tank/ROOT/void@a /zfsbootmenu/mnt",
            "mount -o ro /dev/sdb1 /tmp/luks-key.1-0",
            "mount -r tank/hooks /.external_hooks",
            "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,rw",
            "umount /zfsbootmenu/mnt",
            "umount /zfsbootmenu/mnt",
            "umount -R /zfsbootmenu/mnt",
        ]
    );
    let calls = runner.calls();
    assert_eq!(calls[0].get_stderr(), Stream::Null);
    assert_eq!(calls[1].get_stderr(), Stream::Inherit);
    assert_eq!(calls[2].get_stdout(), Stream::Null);
    assert_eq!(calls[3].get_stdout(), Stream::Inherit);
    assert_eq!(calls[5].get_stdout(), Stream::Inherit);
    assert_eq!(*calls[6].get_stdin(), Input::Null);
}

#[test]
fn a_mount_is_undone_when_dropped_unless_left() {
    let runner = FakeRunner::new();
    let files = FakeFiles::new();
    let mount = Mount::new(&runner, &files);
    let dir = Path::new("/mnt/esp");
    {
        let mounted = mount.fat("/dev/sda1", dir).unwrap().quietly();
        assert_eq!(mounted.path(), dir);
    }
    assert!(mount.fat("/dev/sda1", dir).unwrap().unmount());
    assert_eq!(mount.fat("/dev/sda1", dir).unwrap().leave(), dir);
    assert_eq!(
        runner.lines(),
        [
            "mount -t vfat -o rw,noatime /dev/sda1 /mnt/esp",
            "umount /mnt/esp",
            "mount -t vfat -o rw,noatime /dev/sda1 /mnt/esp",
            "umount /mnt/esp",
            "mount -t vfat -o rw,noatime /dev/sda1 /mnt/esp",
        ]
    );
    // Quietly: no output on the console.
    assert_eq!(runner.calls()[1].get_stdout(), Stream::Null);

    let failing = FakeRunner::new();
    failing.respond("mount -r tank/hooks /h", 32, "");
    let mount = Mount::new(&failing, &files);
    assert_eq!(
        mount.read_only_shown("tank/hooks", Path::new("/h")).err(),
        Some(32)
    );
    assert_eq!(failing.lines(), ["mount -r tank/hooks /h"]);
}

#[test]
fn system_calls_come_first_and_the_programs_after_them_fail() {
    let runner = FakeRunner::new();
    let files = FakeFiles::new();
    files.mount_calls(true);
    let mount = Mount::new(&runner, &files);
    let mnt = Path::new("/zfsbootmenu/mnt");
    drop(
        mount
            .zfs("tank/ROOT/void", mnt, true, Shown::Hidden)
            .unwrap(),
    );
    assert_eq!(
        files.mounts_made(),
        [
            "mount tank/ROOT/void /zfsbootmenu/mnt ro",
            "umount /zfsbootmenu/mnt"
        ]
    );
    assert!(runner.lines().is_empty());

    let refused = FakeFiles::new();
    refused.mount_calls(false).dir("/dev");
    let log = zbm_platform::KernelLog::new(&refused, "7", Path::new("/zfsbootmenu"), 1).install();
    let mount = Mount::new(&runner, &refused);
    drop(
        mount
            .zfs("tank/ROOT/void", mnt, false, Shown::Hidden)
            .unwrap(),
    );
    drop(log);
    assert_eq!(refused.mounts_made().len(), 2);
    let kmsg = refused.read_to_string(Path::new("/dev/kmsg")).unwrap();
    assert!(
        kmsg.contains("the system call failed; running the program"),
        "{kmsg}"
    );
    assert_eq!(
        runner.lines(),
        [
            "mount -o ro -t zfs tank/ROOT/void /zfsbootmenu/mnt",
            "umount /zfsbootmenu/mnt",
        ]
    );
}

#[test]
fn securityfs_mounts_read_only_and_a_failure_is_its_exit_status() {
    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    let line = "mount -t securityfs securityfs /sys/kernel/security -o ro,nosuid,nodev,noexec";
    runner.respond(line, 0, "");
    assert_eq!(
        Mount::new(&runner, &files).securityfs("/sys/kernel/security"),
        Ok(())
    );
    assert_eq!(runner.lines(), [line]);

    let (runner, files) = (FakeRunner::new(), FakeFiles::new());
    runner.respond(line, 32, "");
    assert_eq!(
        Mount::new(&runner, &files).securityfs("/sys/kernel/security"),
        Err(32)
    );
}
