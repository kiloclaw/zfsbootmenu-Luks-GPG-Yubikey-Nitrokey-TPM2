//! The root on its own path, and a teardown that takes it (SB16).

use pretty_assertions::assert_eq;
use zbm_platform::KernelLog;
use zbm_testkit::fakes::FileOperation;
use zbm_tools::mount::table::{MOUNTINFO, mounts};

use super::*;
use crate::chroot::root::check;

/// Where environments are mounted below the state directory.
const ENVIRONMENTS: &str = "/zfsbootmenu/environments";

/// The kernel's line for a mount `id` of `tank/ROOT/be` on `point`,
/// escaped as written.
fn line(id: u64, point: &str) -> String {
    format!("{id} 25 0:50 / {point} rw,relatime shared:1 - zfs tank/ROOT/be rw,xattr")
}

/// The kernel's line for a `tmpfs` mount `id` on `point`, as the chroot's
/// shell makes one with `mount -t tmpfs x /`.
fn tmpfs(id: u64, point: &str) -> String {
    format!("{id} 40 0:{id} / {point} rw,relatime shared:2 - tmpfs x rw")
}

/// Installs the kernel log of `fake` for this thread, where error lines
/// go, without a journal.
fn kernel_log(fake: &FakeSystem) -> zbm_platform::Installed {
    KernelLog::new(&fake.files, "7", Path::new("/zfsbootmenu"), 1)
        .without_journal()
        .install()
}

/// Sets `fake`'s mountinfo to `lines`, the image's root first.
fn table(fake: &FakeSystem, lines: &[String]) {
    let mut text = "25 1 0:2 / / rw - rootfs rootfs rw\n".to_owned();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    fake.files.file(MOUNTINFO, text);
}

/// Makes `fake`'s mountinfo change with `change` as `command` runs,
/// before anything answers it.
fn when(
    fake: &FakeSystem,
    command: String,
    change: impl Fn(&mut Vec<String>) + Send + Sync + 'static,
) {
    let files = fake.files.clone();
    fake.runner.first(move |ran| {
        if ran.display() == command {
            let text = files.text(MOUNTINFO).unwrap_or_default();
            let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
            change(&mut lines);
            let mut text = lines.join("\n");
            text.push('\n');
            files.file(MOUNTINFO, text);
        }
        None
    });
}

/// Drops the last mount on `point`, as an unmount of its path does.
fn drop_top(lines: &mut Vec<String>, point: &str) {
    let top = lines
        .iter()
        .rposition(|line| line.split(' ').nth(4) == Some(point));
    if let Some(top) = top {
        lines.remove(top);
    }
}

/// `fake` with its root listed as mount 40 once mounted on `point`, a
/// `tmpfs` 41 on `point` over it once the shell ran, as `mount -t tmpfs x
/// /` in the chroot makes it, and the topmost mount on `point` taken by
/// `umount -R` where `recursive` holds, and by each `umount -l`.
fn overmounted(fake: &FakeSystem, mounted: &str, point: &'static str, recursive: bool) {
    table(fake, &[]);
    when(fake, mounted.to_owned(), move |lines| {
        lines.push(line(40, point));
    });
    when(fake, "/bin/sh".to_owned(), move |lines| {
        lines.push(tmpfs(41, point));
    });
    if recursive {
        when(fake, format!("umount -R {MP}"), move |lines| {
            drop_top(lines, point);
        });
    }
    when(fake, format!("umount -l {MP}"), move |lines| {
        drop_top(lines, point);
    });
}

/// The mount line of the root of a chroot of `tank/ROOT/be`, read/write.
fn root_mount() -> String {
    format!("mount -o zfsutil,rw -t zfs tank/ROOT/be {MP}")
}

/// Whether anything is listed on [`MP`] in `fake`'s mountinfo.
fn is_listed(fake: &FakeSystem) -> bool {
    !listed(fake).is_empty()
}

/// The IDs listed on [`MP`] in `fake`'s mountinfo.
fn listed(fake: &FakeSystem) -> Vec<u64> {
    let text = fake.files.text(MOUNTINFO).unwrap_or_default();
    mounts(text.as_bytes())
        .filter(|listed| listed.point == MP.as_bytes())
        .map(|listed| listed.id)
        .collect()
}

/// The lines of `fake` after its interactive shell.
fn after_the_shell(fake: &FakeSystem) -> Vec<String> {
    let lines = ran(fake);
    let shell = lines.iter().position(|line| line == "/bin/sh");
    lines[shell.expect("the shell ran") + 1..].to_vec()
}

#[test]
fn a_root_path_through_a_link_in_another_dataset_is_refused() {
    // `tank/a` is mounted on its own path, and its root links `mnt`, where
    // `tank/a/mnt` would be mounted, to the image's `/etc`.
    let fake = machine("off");
    fake.files
        .symlink(format!("{ENVIRONMENTS}/tank/a/mnt/mnt"), "/etc\x1b[2J");
    let before = fake.files.paths();
    let log = kernel_log(&fake);

    let error = open_on(&fake, "tank/a/mnt").unwrap_err();

    drop(log);
    assert!(matches!(error, ChrootError::Mount(_)), "{error:?}");
    assert_eq!(error.to_string(), "failed to mount tank/a/mnt");
    // The error line says why, the link's target escaped.
    let said = format!(
        "tank/a/mnt is not mounted: {ENVIRONMENTS}/tank/a/mnt/mnt is a link to /etc\\x1b[2J"
    );
    assert!(fake.kmsg().contains(&said), "{}", fake.kmsg());
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("mount")),
        "{lines:#?}"
    );
    // Nothing is written, created or removed but the error line and the
    // mark the kernel log leaves for it.
    let logged = [
        Path::new("/dev/kmsg"),
        Path::new("/zfsbootmenu/have_errors"),
    ];
    let operations = fake.files.operations();
    assert!(
        operations.iter().all(|operation| match operation {
            FileOperation::Append(path, _) | FileOperation::Write(path, _) => {
                logged.contains(&path.as_path())
            }
            _ => false,
        }),
        "{operations:#?}"
    );
    let mut paths = fake.files.paths();
    paths.retain(|path| *path != logged[1]);
    assert_eq!(paths, before);
}

#[test]
fn a_root_path_through_another_mounted_dataset_is_refused() {
    let fake = machine("off");
    table(&fake, &[line(40, &format!("{ENVIRONMENTS}/tank/a/mnt"))]);

    let error = open_on(&fake, "tank/a/mnt").unwrap_err();

    assert!(matches!(error, ChrootError::Mount(_)), "{error:?}");
    let lines = ran(&fake);
    assert!(
        !lines.iter().any(|line| line.contains("mount")),
        "{lines:#?}"
    );
    assert_eq!(fake.files.operations(), []);
    let at = format!("{ENVIRONMENTS}/tank/a/mnt/mnt");
    let fs = "tank/a/mnt".parse().unwrap();
    assert_eq!(
        check(&fake.files, Path::new(&at), &fs),
        Err(format!("{ENVIRONMENTS}/tank/a/mnt is a mount point"))
    );
}

#[test]
fn a_root_path_whose_names_cannot_be_read_is_refused() {
    let fake = machine("off");
    fake.files.fail(
        format!("{ENVIRONMENTS}/tank"),
        io::ErrorKind::PermissionDenied,
    );
    let error = open_on(&fake, "tank/ROOT/be").unwrap_err();
    assert!(matches!(error, ChrootError::Mount(_)), "{error:?}");
    assert!(ran(&fake).is_empty(), "{:#?}", ran(&fake));
}

#[test]
fn a_mount_point_that_is_not_the_filesystem_s_own_is_refused() {
    let fake = machine("off");
    let fs = "tank/ROOT/be".parse().unwrap();
    for at in [
        "/mnt",
        "/x/y/z/w/v/mnt",
        "/zfsbootmenu/environments/tank/ROOT/other/mnt",
    ] {
        assert_eq!(
            check(&fake.files, Path::new(at), &fs),
            Err(format!("{at} is not the mount point of tank/ROOT/be")),
        );
    }
    let relative = "tank/ROOT/be/mnt";
    assert_eq!(
        check(&fake.files, Path::new(relative), &fs),
        Err(format!("{relative} is not the mount point of tank/ROOT/be")),
    );
    // The link's own path, in full, and its target are the reason.
    fake.files
        .symlink(format!("{ENVIRONMENTS}/tank/ROOT"), "/etc");
    assert_eq!(
        check(&fake.files, Path::new(MP), &fs),
        Err(format!("{ENVIRONMENTS}/tank/ROOT is a link to /etc"))
    );
}

#[test]
fn a_dataset_already_mounted_on_its_own_path_still_opens() {
    // As the boot path leaves it before the emergency shell.
    // Its teardown takes the chroot's mount, and leaves the boot path's.
    let fake = machine("off");
    table(&fake, &[line(40, MP)]);
    when(&fake, root_mount(), |lines| lines.push(line(41, MP)));
    when(&fake, format!("umount -R {MP}"), |lines| {
        drop_top(lines, MP);
    });

    assert_eq!(open_on(&fake, "tank/ROOT/be").unwrap(), 0);

    let lines = ran(&fake);
    assert_eq!(lines[0], root_mount(), "{lines:#?}");
    assert_eq!(lines.last(), Some(&format!("umount -R {MP}")));
    assert!(!lines.iter().any(|line| line.starts_with("umount -l")));
    assert_eq!(listed(&fake), [40]);
}

#[test]
fn a_root_that_umount_r_did_not_take_is_detached_lazily() {
    let fake = machine("off");
    efi(&fake);
    overmounted(&fake, &root_mount(), MP, true);

    open_on(&fake, "tank/ROOT/be").unwrap();

    let lines = ran(&fake);
    let host = "mount -t efivarfs efivarfs /sys/firmware/efi/efivars -o remount,ro";
    assert_eq!(
        lines[lines.len() - 3..],
        [
            format!("umount -R {MP}"),
            format!("umount -l {MP}"),
            host.to_owned()
        ],
        "{lines:#?}"
    );
    assert!(!is_listed(&fake));
}

#[test]
fn a_busy_root_is_detached_lazily() {
    let fake = machine("off");
    overmounted(&fake, &root_mount(), MP, false);
    fake.runner.respond(&format!("umount -R {MP}"), 32, "");

    open_on(&fake, "tank/ROOT/be").unwrap();

    let lines = ran(&fake);
    assert_eq!(
        lines[lines.len() - 3..],
        [
            format!("umount -R {MP}"),
            format!("umount -l {MP}"),
            format!("umount -l {MP}"),
        ],
        "{lines:#?}"
    );
    assert!(!is_listed(&fake));
}

#[test]
fn the_lazy_rounds_stop_after_eight() {
    let fake = machine("off");
    table(&fake, &[]);
    when(&fake, root_mount(), |lines| lines.push(line(40, MP)));
    // The root is busy, `umount -l` fails, and the root stays listed.
    fake.runner.respond(&format!("umount -R {MP}"), 32, "");
    fake.runner.respond(&format!("umount -l {MP}"), 32, "");

    open_on(&fake, "tank/ROOT/be").unwrap();

    let lines = ran(&fake);
    let lazy = lines
        .iter()
        .filter(|line| **line == format!("umount -l {MP}"))
        .count();
    assert_eq!(lazy, 8, "{lines:#?}");
    assert_eq!(lines.last(), Some(&format!("umount -l {MP}")));
    let recursive = lines.iter().position(|line| line.starts_with("umount -R"));
    assert_eq!(recursive, Some(lines.len() - 9), "{lines:#?}");
}

#[test]
fn without_mountinfo_the_teardown_is_umount_r_alone() {
    let fake = machine("off");
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines.last(), Some(&format!("umount -R {MP}")));
    assert!(!lines.iter().any(|line| line.starts_with("umount -l")));

    // A root that `umount -R` took is not detached again.
    let fake = machine("off");
    table(&fake, &[]);
    when(&fake, root_mount(), |lines| lines.push(line(40, MP)));
    when(&fake, format!("umount -R {MP}"), |lines| {
        drop_top(lines, MP);
    });
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines.last(), Some(&format!("umount -R {MP}")));
    assert!(!lines.iter().any(|line| line.starts_with("umount -l")));
}

#[test]
fn a_link_or_a_mount_on_the_root_s_way_since_it_was_mounted_leaves_its_mounts() {
    // `tank/a` is mounted on its own path while a chroot of `tank/a/mnt`
    // is open, from a recovery shell, a hook or a failed boot; or a link
    // to the image's `/dev` appears on the way.
    let at = format!("{ENVIRONMENTS}/tank/a/mnt/mnt");
    for linked in [false, true] {
        let fake = machine("off");
        fake.files.file_with_mode(format!("{at}/bin/sh"), "", 0o755);
        let files = fake.files.clone();
        let (parent, link) = (format!("{ENVIRONMENTS}/tank/a/mnt"), at.clone());
        if linked {
            // No mountinfo either: the link alone leaves the mounts.
            fake.runner.first(move |ran| {
                if ran.display() == "/bin/sh" {
                    files.symlink(&link, "/dev");
                }
                None
            });
        } else {
            table(&fake, &[]);
            let point = at.clone();
            let mounted = format!("mount -o zfsutil,rw -t zfs tank/a/mnt {at}");
            when(&fake, mounted, move |lines| lines.push(line(40, &point)));
            when(&fake, "/bin/sh".to_owned(), move |lines| {
                lines.push(line(41, &parent));
            });
        }

        open_on(&fake, "tank/a/mnt").unwrap();

        let after = after_the_shell(&fake);
        assert!(
            !after.iter().any(|line| line.starts_with("umount")),
            "{linked}: {after:#?}"
        );
    }
}

#[test]
fn another_root_of_the_filesystem_over_the_chroot_s_is_left_mounted() {
    // Another front end's chroot of the same dataset, mounted over this
    // one while it was open.
    let fake = machine("off");
    table(&fake, &[]);
    when(&fake, root_mount(), |lines| lines.push(line(40, MP)));
    when(&fake, "/bin/sh".to_owned(), |lines| {
        lines.push(line(41, MP));
    });

    open_on(&fake, "tank/ROOT/be").unwrap();

    let after = after_the_shell(&fake);
    assert!(
        !after.iter().any(|line| line.starts_with("umount")),
        "{after:#?}"
    );
    assert_eq!(listed(&fake), [40, 41]);
}

#[test]
fn a_root_the_shell_unmounted_is_not_looked_for_again() {
    // The boot path left the dataset mounted, and the shell ran
    // `umount -l /` over it.
    let fake = machine("off");
    table(&fake, &[line(40, MP)]);
    when(&fake, root_mount(), |lines| lines.push(line(41, MP)));
    when(&fake, "/bin/sh".to_owned(), |lines| {
        drop_top(lines, MP);
    });

    open_on(&fake, "tank/ROOT/be").unwrap();

    let after = after_the_shell(&fake);
    assert!(
        !after.iter().any(|line| line.starts_with("umount")),
        "{after:#?}"
    );
    assert_eq!(listed(&fake), [40]);
}

#[test]
fn the_table_is_not_read_again_once_the_root_is_taken() {
    // The kernel gives the root's ID to the next mount on its path, here
    // another front end's chroot of the same dataset, at once.
    let reused = |lines: &mut Vec<String>| {
        drop_top(lines, MP);
        lines.push(line(40, MP));
    };
    // `umount -R` takes the root whole.
    let fake = machine("off");
    table(&fake, &[]);
    when(&fake, root_mount(), |lines| lines.push(line(40, MP)));
    when(&fake, format!("umount -R {MP}"), reused);
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(lines.last(), Some(&format!("umount -R {MP}")));
    assert!(!lines.iter().any(|line| line.starts_with("umount -l")));
    assert_eq!(listed(&fake), [40]);

    // A busy root, which `umount -l` takes.
    let fake = machine("off");
    table(&fake, &[]);
    when(&fake, root_mount(), |lines| lines.push(line(40, MP)));
    fake.runner.respond(&format!("umount -R {MP}"), 32, "");
    when(&fake, format!("umount -l {MP}"), reused);
    open_on(&fake, "tank/ROOT/be").unwrap();
    let lines = ran(&fake);
    assert_eq!(
        lines[lines.len() - 2..],
        [format!("umount -R {MP}"), format!("umount -l {MP}")],
        "{lines:#?}"
    );
    assert_eq!(listed(&fake), [40]);
}

/// The unmounts of the root's path in a teardown of a busy root, where
/// `change` changes `fake`'s mountinfo as `command` runs, and the table
/// cannot be read after it where `unread` holds.
fn root_unmounts(command: String, change: fn(&mut Vec<String>), unread: bool) -> Vec<String> {
    let fake = machine("off");
    table(&fake, &[]);
    when(&fake, root_mount(), |lines| lines.push(line(40, MP)));
    fake.runner.respond(&format!("umount -R {MP}"), 32, "");
    when(&fake, command.clone(), change);
    let files = fake.files.clone();
    fake.runner.first(move |ran| {
        if unread && ran.display() == command {
            files.fail(MOUNTINFO, io::ErrorKind::PermissionDenied);
        }
        None
    });
    open_on(&fake, "tank/ROOT/be").unwrap();
    let mut lines = ran(&fake);
    lines.retain(|line| line.starts_with("umount -"));
    lines
}

#[test]
fn what_comes_onto_the_root_s_path_during_the_teardown_stops_it() {
    // Another front end's chroot of the same dataset is mounted over the
    // root as the last mount below it is unmounted, or as `umount -R`
    // fails on the busy root; or the root is gone as the last mount below
    // it is unmounted, or as `umount -R` fails; or the table cannot be
    // read after either.
    let over = |lines: &mut Vec<String>| lines.push(line(41, MP));
    let gone = |lines: &mut Vec<String>| drop_top(lines, MP);
    let same = |_: &mut Vec<String>| {};
    let tmp = format!("umount {MP}/tmp");
    let none = Vec::<String>::new();
    assert_eq!(root_unmounts(tmp.clone(), over, false), none);
    assert_eq!(root_unmounts(tmp.clone(), gone, false), none);
    let recursive = vec![format!("umount -R {MP}")];
    assert_eq!(root_unmounts(tmp, same, true), recursive);
    assert_eq!(root_unmounts(recursive[0].clone(), over, false), recursive);
    assert_eq!(root_unmounts(recursive[0].clone(), gone, false), recursive);
    assert_eq!(root_unmounts(recursive[0].clone(), same, true), recursive);
}

#[test]
fn mountinfo_escapes_are_read_as_the_kernel_writes_them() {
    // A dataset with a space in its name: the kernel writes it `\040`.
    const SPACED: &str = "/zfsbootmenu/environments/tank/my be/mnt";
    const WRITTEN: &str = "/zfsbootmenu/environments/tank/my\\040be/mnt";
    let fake = machine("off");
    fake.files
        .file_with_mode(format!("{SPACED}/bin/sh"), "", 0o755);
    let mounted = format!("mount -o zfsutil,rw -t zfs tank/my be {SPACED}");
    table(&fake, &[]);
    when(&fake, mounted, |lines| lines.push(line(40, WRITTEN)));
    when(&fake, format!("umount -l {SPACED}"), |lines| {
        drop_top(lines, WRITTEN);
    });
    fake.runner.respond(&format!("umount -R {SPACED}"), 32, "");

    open_on(&fake, "tank/my be").unwrap();

    let lines = ran(&fake);
    assert_eq!(
        lines[lines.len() - 2..],
        [format!("umount -R {SPACED}"), format!("umount -l {SPACED}")],
        "{lines:#?}"
    );

    // A prefix of the path refused through an escape.
    let fake = machine("off");
    let prefix = "/zfsbootmenu/environments/tank/my\\040be";
    table(&fake, &[line(40, prefix)]);
    let fs = "tank/my be/x".parse().unwrap();
    let at = Path::new("/zfsbootmenu/environments/tank/my be/x/mnt");
    assert_eq!(
        check(&fake.files, at, &fs),
        Err("/zfsbootmenu/environments/tank/my be is a mount point".to_owned())
    );
}
