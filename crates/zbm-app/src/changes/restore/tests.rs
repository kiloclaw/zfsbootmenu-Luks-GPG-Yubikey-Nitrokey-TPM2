//! Tests of restoring paths on a fake machine, whose `cp` copies within
//! its fake files.

use std::path::Path;

use pretty_assertions::assert_eq;
use zbm_platform::exec::{Output, Status};
use zbm_testkit::fakes::FakeSystem;

use super::*;

const MOUNTED: &str = "/zfsbootmenu/environments/zroot/ROOT/void/mnt";
const FROM: &str = "/zfsbootmenu/environments/zroot/ROOT/void/mnt/.zfs/snapshot/first";

fn snapshot() -> SnapshotName {
    "zroot/ROOT/void@first".parse().unwrap()
}

/// A machine whose environment has `/etc/fstab` changed since `@first`,
/// and `/etc/old` removed with the file in it; its `cp -a` copies files
/// and directories, and fails when told.
fn machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.files
        .file(format!("{FROM}/etc/fstab"), "old fstab")
        .file(format!("{FROM}/etc/old/conf"), "conf")
        .file(format!("{MOUNTED}/etc/fstab"), "new fstab");
    let files = fake.files.clone();
    fake.runner.first(move |command| {
        let [_, _, from, to] = command.get_args() else {
            return None;
        };
        if command.program() != "cp" {
            return None;
        }
        let (from, to) = (Path::new(from), Path::new(to));
        if from.ends_with("broken") {
            return Some(Ok(Output::new(Status::exited(1), "")));
        }
        if from.ends_with("full") {
            files.dir(to);
            let why = format!("cp: {}/\u{1b}]0;x: No space left", to.display());
            let failed = Output {
                stderr: why.into_bytes(),
                ..Output::new(Status::exited(1), "")
            };
            return Some(Ok(failed));
        }
        for path in files
            .paths()
            .into_iter()
            .filter(|path| path.starts_with(from))
        {
            let copy = to.join(path.strip_prefix(from).unwrap());
            match files.contents(&path) {
                Some(contents) => files.file(copy, contents),
                None => files.dir(copy),
            };
        }
        Some(Ok(Output::new(Status::exited(0), "")))
    });
    fake
}

/// Restores `paths` on `fake`: the check, then the copy.
fn restore(fake: &FakeSystem, paths: &[&str]) -> Result<usize, Vec<String>> {
    let snapshot = snapshot();
    let restore = Restore::new(&fake.files, &fake.runner, MOUNTED, &snapshot);
    let paths: Vec<String> = paths.iter().map(|path| (*path).to_owned()).collect();
    restore.check(&paths)?;
    restore.copy(&paths)
}

#[test]
fn a_file_is_copied_beside_itself_then_replaces_itself() {
    let fake = machine();

    assert_eq!(restore(&fake, &["/etc/fstab"]), Ok(1));

    assert_eq!(
        fake.files.text(format!("{MOUNTED}/etc/fstab")).as_deref(),
        Some("old fstab")
    );
    assert_eq!(
        fake.runner.lines(),
        [format!(
            "cp -a -- {FROM}/etc/fstab {MOUNTED}/etc/.fstab.zbm-restore"
        )]
    );
    assert!(
        !fake
            .files
            .exists(Path::new(&format!("{MOUNTED}/etc/.fstab.zbm-restore")))
    );
}

#[test]
fn a_removed_directory_comes_back_whole_and_one_still_there_is_left() {
    let fake = machine();

    // The directories left as they are count as restored.
    assert_eq!(
        restore(&fake, &["/", "/etc", "/etc/old", "/etc/old/conf"]),
        Ok(4)
    );

    assert_eq!(
        fake.files
            .text(format!("{MOUNTED}/etc/old/conf"))
            .as_deref(),
        Some("conf")
    );
    // The directories the environment has are not copied; nothing is
    // deleted.
    assert_eq!(fake.runner.lines().len(), 2);
    assert_eq!(
        fake.files.text(format!("{MOUNTED}/etc/fstab")).as_deref(),
        Some("new fstab")
    );
}

#[test]
fn a_link_above_a_path_is_refused_before_anything_is_copied() {
    let fake = machine();
    fake.files
        .symlink(format!("{MOUNTED}/boot"), "/zfsbootmenu")
        .file(format!("{FROM}/boot/vmlinuz"), "kernel")
        .symlink(format!("{FROM}/lib"), "/usr/lib")
        .file("/usr/lib/libc.so", "the image's");

    let why = restore(
        &fake,
        &["/boot/vmlinuz", "/lib/libc.so", "/etc/fstab", "/etc/gone"],
    )
    .unwrap_err();

    assert_eq!(
        why,
        [
            "/boot is a symbolic link in zroot/ROOT/void",
            "/lib is a symbolic link in zroot/ROOT/void@first",
            "/etc/gone is not in zroot/ROOT/void@first",
        ]
    );
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn a_link_at_the_path_is_replaced_not_written_through() {
    let fake = machine();
    fake.files
        .file("/zfsbootmenu/secret", "kept")
        .symlink(format!("{MOUNTED}/etc/hostname"), "/zfsbootmenu/secret")
        .file(format!("{FROM}/etc/hostname"), "void");

    assert_eq!(restore(&fake, &["/etc/hostname"]), Ok(1));

    assert_eq!(
        fake.files.text("/zfsbootmenu/secret").as_deref(),
        Some("kept")
    );
    assert!(
        fake.files
            .read_link(Path::new(&format!("{MOUNTED}/etc/hostname")))
            .is_err()
    );
    assert_eq!(
        fake.files
            .text(format!("{MOUNTED}/etc/hostname"))
            .as_deref(),
        Some("void")
    );
}

#[test]
fn a_failed_copy_stops_the_restore_and_says_how_far_it_got() {
    let fake = machine();
    fake.files
        .file(format!("{FROM}/etc/broken"), "x")
        .file(format!("{MOUNTED}/etc/.hosts.zbm-restore"), "left")
        .file(format!("{FROM}/etc/hosts"), "hosts");

    let why = restore(&fake, &["/etc/fstab", "/etc/broken", "/etc/hosts"]).unwrap_err();
    assert_eq!(
        why,
        [
            "/etc/broken was not restored: cp failed",
            "1 restored before it"
        ]
    );

    let why = restore(&fake, &["/etc/hosts"]).unwrap_err();
    assert_eq!(
        why[0],
        format!("/etc/hosts was not restored: {MOUNTED}/etc/.hosts.zbm-restore is in the way")
    );

    let unreadable = FakeSystem::new();
    assert_eq!(
        restore(&unreadable, &["/etc/fstab"]),
        Err(vec![
            "the files of zroot/ROOT/void@first cannot be read".to_owned()
        ])
    );
}

#[test]
fn a_path_of_another_kind_is_refused_before_anything_is_copied() {
    let fake = machine();
    fake.files
        .file(format!("{MOUNTED}/etc/old"), "a file now")
        .file(format!("{FROM}/etc/motd"), "motd")
        .file(format!("{MOUNTED}/etc/motd/new"), "a directory now");

    let why = restore(&fake, &["/etc/fstab", "/etc/motd", "/etc/old"]).unwrap_err();

    assert_eq!(
        why,
        [
            "/etc/motd is a directory in zroot/ROOT/void but not in zroot/ROOT/void@first",
            "/etc/old is a directory in zroot/ROOT/void@first but not in zroot/ROOT/void",
        ]
    );
    assert!(fake.runner.lines().is_empty());
}

#[test]
fn a_partial_copy_of_a_directory_is_left_and_named() {
    let fake = machine();
    fake.files.file(format!("{FROM}/etc/full/a"), "a");

    let why = restore(&fake, &["/etc/full"]).unwrap_err();

    let left = format!("{MOUNTED}/etc/.full.zbm-restore");
    assert_eq!(
        why[0],
        format!(
            "/etc/full was not restored: cp: {left}/\\x1b]0;x: No space left; its partial copy \
             {left} is left"
        )
    );
    assert!(fake.files.is_dir(Path::new(&left)));
}

#[test]
fn names_from_the_environment_are_shown_escaped() {
    let fake = machine();
    let name = "/etc/\u{1b}]0;owned\u{7}";

    let why = restore(&fake, &[name]).unwrap_err();

    assert_eq!(
        why,
        ["/etc/\\x1b]0;owned\\x07 is not in zroot/ROOT/void@first"]
    );
}

#[test]
fn a_copy_that_does_not_replace_its_path_is_removed() {
    let fake = machine();
    fake.files.fail(
        format!("{MOUNTED}/etc/fstab"),
        std::io::ErrorKind::PermissionDenied,
    );

    let why = restore(&fake, &["/etc/fstab"]).unwrap_err();

    assert_eq!(
        why,
        [
            "/etc/fstab was not restored: the copy did not replace it: Permission denied \
             (os error 13)",
            "0 restored before it",
        ]
    );
    assert!(
        !fake
            .files
            .exists(Path::new(&format!("{MOUNTED}/etc/.fstab.zbm-restore")))
    );
}

/// The copy checks each path again, as one copied before it may have
/// brought a link with it; it stops at the first it refuses.
#[test]
fn a_path_refused_during_the_copy_stops_it() {
    let fake = machine();
    fake.files.symlink(format!("{FROM}/lib"), "/usr/lib");
    let snapshot = snapshot();
    let restore = Restore::new(&fake.files, &fake.runner, MOUNTED, &snapshot);

    let why = restore
        .copy(&["/etc/fstab".to_owned(), "/lib/libc.so".to_owned()])
        .unwrap_err();

    assert_eq!(
        why,
        [
            "/lib is a symbolic link in zroot/ROOT/void@first",
            "1 restored before it",
        ]
    );
    assert_eq!(fake.runner.lines().len(), 1);
}

/// A path that names no file, which the paths a change restores never
/// are, is not copied.
#[test]
fn a_path_with_no_name_is_not_copied() {
    let fake = machine();
    let snapshot = snapshot();
    let restore = Restore::new(&fake.files, &fake.runner, MOUNTED, &snapshot);

    let why = restore.copy(&["/etc/old/..".to_owned()]).unwrap_err();

    assert_eq!(
        why,
        [
            "/etc/old/.. was not restored: it has no name",
            "0 restored before it",
        ]
    );
    assert!(fake.runner.lines().is_empty());
}
