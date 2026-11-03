//! Tests of the root argument and its prefix.

use super::*;
use zbm_testkit::fakes::FakeFiles;

#[test]
fn a_root_argument_is_one_argument() {
    let root = |prefix, fs| root_argument(prefix, fs).map(|arg| arg.to_string());
    let refused = |prefix, fs| root_argument(prefix, fs).is_none();
    assert_eq!(
        root("root=zfs:", "tank/ROOT/void").unwrap(),
        "root=zfs:tank/ROOT/void"
    );
    assert_eq!(
        root("root=ZFS=", "tank/a b").unwrap(),
        "root=\"ZFS=tank/a b\""
    );
    assert!(refused("zfs:", "tank/ROOT"));
    assert!(refused("root=\"", "tank"));
    for prefix in ["root=zfs:", "zfs=", "root=ZFS=", "root=custom:"] {
        assert!(is_root_prefix(prefix), "{prefix}");
    }
    // Each of these would add arguments, or swallow them, before the fix.
    for prefix in [
        "root=zfs: init=/bin/sh ",
        "init=/bin/sh root=",
        "root=\"zfs:",
        "=zfs:",
        "zfs:",
        "",
    ] {
        assert!(!is_root_prefix(prefix), "{prefix}");
    }
}

#[test]
fn a_root_prefix_that_is_not_one_is_ignored() {
    let files = FakeFiles::new();
    let mnt = Path::new("/mnt");
    assert_eq!(
        root_prefix(&files, Some("root=zfs: init=/bin/sh "), mnt),
        "root=zfs:"
    );
}

#[test]
fn root_prefixes() {
    let files = FakeFiles::new();
    let mnt = Path::new("/mnt");
    assert_eq!(
        root_prefix(&files, Some("root=custom:"), mnt),
        "root=custom:"
    );
    assert_eq!(root_prefix(&files, Some("-"), mnt), "root=zfs:");
    assert_eq!(root_prefix(&files, None, mnt), "root=zfs:");
    files.file("/mnt/usr/lib/os-release", "NAME=\"Arch Linux\"\nID=arch\n");
    assert_eq!(root_prefix(&files, None, mnt), "zfs=");
    files.file("/mnt/etc/os-release", "ID='gentoo'\n");
    assert_eq!(root_prefix(&files, None, mnt), "root=ZFS=");
    files.file(
        "/mnt/etc/os-release",
        "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
    );
    assert_eq!(root_prefix(&files, None, mnt), "root=zfs:");
    files.file("/mnt/etc/os-release", "ID=other\n");
    files.file("/mnt/usr/lib/os-release", "ID_LIKE=alpine\n");
    assert_eq!(root_prefix(&files, None, mnt), "root=ZFS=");
    assert_eq!(
        os_release("A=\"x \\\"y\\\" \\n\"\n# c\nB='z'\nC=a\\ b\nbad line\nD=\"open\n"),
        [
            ("A".to_owned(), "x \"y\" \\n".to_owned()),
            ("B".to_owned(), "z".to_owned()),
            ("C".to_owned(), "a b".to_owned()),
        ]
    );
}
