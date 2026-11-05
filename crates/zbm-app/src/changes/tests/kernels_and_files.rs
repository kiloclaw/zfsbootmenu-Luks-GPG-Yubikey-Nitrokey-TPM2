//! Tests of the default kernel and of restoring files, on [`FakeZfs`] with
//! needs that record what was asked of them.

use pretty_assertions::assert_eq;
use zbm_types::KernelSelector;

use super::*;

const KERNEL: &str = "org.zfsbootmenu:kernel";

fn version(text: &str) -> Untrusted<String> {
    Untrusted::new(text.to_owned())
}

#[test]
fn a_default_kernel_is_set_as_its_file_and_inherited_again() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();
    let void = dataset("zroot/ROOT/void");

    let events = perform(
        &zfs,
        &mut needs,
        &Action::SetDefaultKernel(void.clone(), version("6.12.9+")),
    );
    assert_eq!(
        outcome(&events),
        &Outcome::Done("vmlinuz-6.12.9+ is the default kernel of zroot/ROOT/void".to_owned())
    );
    let events = perform(
        &zfs,
        &mut needs,
        &Action::InheritDefaultKernel(void.clone()),
    );
    assert!(is_done(&events));

    assert_eq!(
        needs.asked,
        [
            "writable zroot",
            "load_key zroot/ROOT/void",
            "kernel_file zroot/ROOT/void 6.12.9+",
            "writable zroot",
        ]
    );
    let changes: Vec<String> = zfs
        .calls()
        .into_iter()
        .filter(|call| call.starts_with("set ") || call.starts_with("inherit "))
        .collect();
    assert_eq!(
        changes,
        [
            format!("set zroot/ROOT/void {KERNEL}=/vmlinuz-6\\.12\\.9\\+$"),
            format!("inherit zroot/ROOT/void {KERNEL}"),
        ]
    );
}

#[test]
fn inheriting_the_default_kernel_needs_no_key() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        locked: true,
        ..FakeNeeds::default()
    };
    let void = dataset("zroot/ROOT/void");

    let events = perform(
        &zfs,
        &mut needs,
        &Action::InheritDefaultKernel(void.clone()),
    );
    assert!(is_done(&events));
    let events = perform(
        &zfs,
        &mut needs,
        &Action::SetDefaultKernel(void, version("6.12.9")),
    );
    assert!(!is_done(&events));

    assert_eq!(
        needs.asked,
        [
            "writable zroot",
            "writable zroot",
            "load_key zroot/ROOT/void"
        ]
    );
    assert!(
        zfs.calls()
            .contains(&format!("inherit zroot/ROOT/void {KERNEL}"))
    );
}

#[test]
fn a_kernel_file_is_shown_escaped() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();

    let events = perform(
        &zfs,
        &mut needs,
        &Action::SetDefaultKernel(dataset("zroot/ROOT/void"), version("6\x1b]0;x\x07")),
    );

    assert_eq!(
        outcome(&events),
        &Outcome::Done(
            "vmlinuz-6\\x1b]0;x\\x07 is the default kernel of zroot/ROOT/void".to_owned()
        )
    );
}

#[test]
fn the_kernel_value_names_its_kernel_alone() {
    let kernels = [
        "/boot/vmlinuz-6.12.9+",
        "/boot/vmlinuz-6.12.99",
        "/boot/vmlinuz-6x12x9+",
        "/boot/vmlinux-6.12.9+",
        "/boot/vmlinuz-6.12.9++",
    ];
    for (file, path) in [
        ("vmlinuz-6.12.9+", kernels[0]),
        ("vmlinux-6.12.9+", kernels[3]),
        ("vmlinuz-6.12.9++", kernels[4]),
    ] {
        let selector: KernelSelector = kernel_selector(file).parse().unwrap();
        let matched: Vec<&str> = kernels
            .into_iter()
            .filter(|kernel| selector.matches(kernel))
            .collect();
        assert_eq!(matched, [path], "{selector}");
    }
    let every = "a.b[c]d(e)f*g+h?i{1}j|k^l$m\\n";
    let selector: KernelSelector = kernel_selector(every).parse().unwrap();
    assert!(selector.matches(&format!("/boot/{every}")));
    assert!(!selector.matches("/boot/aXb[c]d(e)f*g+h?i{1}j|k^l$m\\n"));
}

#[test]
fn a_kernel_the_environment_lacks_sets_nothing() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        no_kernels: true,
        ..FakeNeeds::default()
    };

    let events = perform(
        &zfs,
        &mut needs,
        &Action::SetDefaultKernel(dataset("zroot/ROOT/void"), version("6.9")),
    );

    let Outcome::Failed { why, .. } = outcome(&events) else {
        panic!("set a kernel that is not there");
    };
    assert_eq!(why, &["zroot/ROOT/void has no kernel 6.9"]);
    assert!(!zfs.calls().iter().any(|call| call.starts_with("set ")));
}

fn restoring(paths: &[&str]) -> Action {
    Action::RestoreFiles {
        snapshot: snapshot("zroot/ROOT/void@first"),
        paths: paths.iter().map(|path| version(path)).collect(),
    }
}

#[test]
fn a_restore_takes_a_safety_snapshot_then_copies_into_the_mounted_environment() {
    let zfs = machine();
    let mut needs = FakeNeeds::default();

    let events = perform(
        &zfs,
        &mut needs,
        &restoring(&["/etc/rc.conf", "/etc/fstab"]),
    );

    let safety = "zroot/ROOT/void@before-restore-2026-10-26-101515";
    assert_eq!(
        outcome(&events),
        &Outcome::Done(format!(
            "2 paths of zroot/ROOT/void restored from first; rolling back to {safety} undoes \
             it"
        ))
    );
    assert_eq!(
        needs.asked,
        [
            "writable zroot",
            "load_key zroot/ROOT/void",
            "mount_writable zroot/ROOT/void",
            "check zroot/ROOT/void@first /etc/fstab,/etc/rc.conf",
            "restore zroot/ROOT/void@first /etc/fstab,/etc/rc.conf",
            "unmount /zfsbootmenu/environments/mnt",
        ]
    );
    assert!(zfs.calls().contains(&format!("create_snapshot {safety}")));

    // A second restore in the same second has a safety snapshot of its own.
    let events = perform(&zfs, &mut needs, &restoring(&["/etc/fstab"]));
    assert!(is_done(&events));
    assert!(zfs.calls().contains(&format!("create_snapshot {safety}-2")));
}

#[test]
fn paths_that_could_leave_the_environment_are_refused_before_anything_is_written() {
    for (path, why) in [
        ("etc/fstab", "etc/fstab is not an absolute path"),
        ("/etc/../../boot", "/etc/../../boot goes up with .."),
        ("/etc//fstab", "/etc//fstab is not a normalised path"),
        ("/etc/./fstab", "/etc/./fstab is not a normalised path"),
        ("/etc/", "/etc/ is not a normalised path"),
        ("/etc/a\0b", "/etc/a\\x00b holds a NUL byte"),
    ] {
        let zfs = machine();
        let mut needs = FakeNeeds::default();
        let events = perform(&zfs, &mut needs, &restoring(&["/etc/fstab", path]));
        let Outcome::Failed { why: given, .. } = outcome(&events) else {
            panic!("{path} was restored");
        };
        assert_eq!(given, &[why.to_owned()], "{path}");
        assert!(needs.asked.is_empty(), "{path}: {:?}", needs.asked);
    }
    let events = perform(&machine(), &mut FakeNeeds::default(), &restoring(&[]));
    assert!(!is_done(&events));

    // A link above a path, found on the mounted environment.
    let zfs = machine();
    let mut needs = FakeNeeds {
        refused: Some("/boot is a symbolic link in zroot/ROOT/void".to_owned()),
        ..FakeNeeds::default()
    };
    let events = perform(&zfs, &mut needs, &restoring(&["/boot/vmlinuz"]));
    assert!(!is_done(&events));
    assert!(!zfs.calls().iter().any(|call| call.starts_with("create")));
    assert_eq!(
        needs.asked.last().map(String::as_str),
        Some("unmount /zfsbootmenu/environments/mnt")
    );
}

#[test]
fn a_failed_copy_still_unmounts_and_names_the_safety_snapshot() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        copy_error: Some("cp failed".to_owned()),
        ..FakeNeeds::default()
    };

    let events = perform(&zfs, &mut needs, &restoring(&["/etc/fstab"]));

    let Outcome::Failed { why, .. } = outcome(&events) else {
        panic!("the failed copy did not fail the restore");
    };
    assert_eq!(
        why,
        &[
            "cp failed",
            "rolling back to zroot/ROOT/void@before-restore-2026-10-26-101515 undoes what was \
             restored",
        ]
    );
    assert_eq!(
        needs.asked.last().map(String::as_str),
        Some("unmount /zfsbootmenu/environments/mnt")
    );

    let refused = machine().failing("create_snapshot", "refused");
    let mut needs = FakeNeeds::default();
    let events = perform(&refused, &mut needs, &restoring(&["/etc/fstab"]));
    assert!(!is_done(&events));
    assert!(!needs.asked.iter().any(|asked| asked.starts_with("restore")));
}

#[test]
fn an_environment_that_cannot_be_mounted_read_write_restores_nothing() {
    let zfs = machine();
    let mut needs = FakeNeeds {
        mount_error: Some("zroot/ROOT/void could not be mounted read/write".to_owned()),
        ..FakeNeeds::default()
    };

    let events = perform(&zfs, &mut needs, &restoring(&["/etc/fstab"]));

    assert_eq!(
        outcome(&events),
        &Outcome::Failed {
            what: "unable to mount zroot/ROOT/void read/write".to_owned(),
            why: vec!["zroot/ROOT/void could not be mounted read/write".to_owned()],
            next: Vec::new(),
        }
    );
    assert_eq!(
        needs.asked.last().map(String::as_str),
        Some("mount_writable zroot/ROOT/void")
    );
    assert!(!zfs.calls().iter().any(|call| call.starts_with("create")));
}

#[test]
fn a_restore_takes_no_snapshot_when_no_name_is_left_for_it() {
    // ZFS names are at most 255 bytes: `@first` fits, the safety
    // snapshot's name does not.
    let long = format!("zroot/ROOT/{}", "v".repeat(220));
    let zfs = machine().filesystem(&long, &[]);
    let mut needs = FakeNeeds::default();
    let action = Action::RestoreFiles {
        snapshot: snapshot(&format!("{long}@first")),
        paths: vec![version("/"), version("/etc/fstab")],
    };

    let events = perform(&zfs, &mut needs, &action);

    let Outcome::Failed { why, .. } = outcome(&events) else {
        panic!("restored without a safety snapshot");
    };
    assert_eq!(
        why,
        &[format!("no name is left for a safety snapshot of {long}")]
    );
    assert!(!zfs.calls().iter().any(|call| call.starts_with("create")));
    assert!(!needs.asked.iter().any(|asked| asked.starts_with("restore")));
    // The root is a path like any other, and the check sees it first.
    assert!(
        needs
            .asked
            .contains(&format!("check {long}@first /,/etc/fstab")),
        "{:?}",
        needs.asked
    );
    assert_eq!(
        needs.asked.last().map(String::as_str),
        Some("unmount /zfsbootmenu/environments/mnt")
    );
}
