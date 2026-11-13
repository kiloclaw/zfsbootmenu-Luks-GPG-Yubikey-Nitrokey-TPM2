//! The reader of the kernel's table of mounts (SB16, public since SB19).

use pretty_assertions::assert_eq;
use proptest::prelude::*;

use super::{Listed, mounts};

#[test]
fn each_escape_the_kernel_writes_is_read_back() {
    let written = b"7 1 0:7 / /a\\040b\\011c\\012d\\134e rw - zfs t\\040x rw\n";
    let read = Listed {
        id: 7,
        root: b"/".to_vec(),
        point: b"/a b\tc\nd\\e".to_vec(),
        kind: b"zfs".to_vec(),
        source: b"t x".to_vec(),
    };
    assert_eq!(mounts(written).collect::<Vec<_>>(), [read]);
}

#[test]
fn odd_mountinfo_lines_are_skipped() {
    let table = concat!(
        "\n",
        "x\n",
        "40\n",
        "40 1 0:1 /\n",
        "-1 1 0:1 / /negative rw\n",
        "abc 1 0:1 / /word rw\n",
        "99999999999999999999999 1 0:1 / /huge rw\n",
        "  41   1 0:1 / /spaced  rw\n",
        "42 1 0:1 / /odd\\0\\999\\x\\04 rw\n",
        "43 1 0:1 / /end\\",
    );
    assert_eq!(
        points(table.as_bytes()),
        [
            (41, b"/spaced".to_vec()),
            (42, b"/odd\\0\\999\\x\\04".to_vec()),
            (43, b"/end\\".to_vec()),
        ]
    );
}

#[test]
fn optional_fields_before_the_separator_are_skipped() {
    let table = concat!(
        "50 1 0:50 / /none rw - zfs tank/a rw\n",
        "51 1 0:51 / /two rw shared:1 master:2 - zfs tank/b rw\n",
    );
    let described: Vec<_> = mounts(table.as_bytes())
        .map(|listed| (listed.point, listed.kind, listed.source))
        .collect();
    assert_eq!(
        described,
        [
            (b"/none".to_vec(), b"zfs".to_vec(), b"tank/a".to_vec()),
            (b"/two".to_vec(), b"zfs".to_vec(), b"tank/b".to_vec()),
        ]
    );
}

#[test]
fn a_line_without_a_separator_has_no_kind_or_source() {
    let table = b"52 1 0:52 /dir /alone rw shared:1 zfs tank/c rw\n";
    let read = Listed {
        id: 52,
        root: b"/dir".to_vec(),
        point: b"/alone".to_vec(),
        kind: Vec::new(),
        source: Vec::new(),
    };
    assert_eq!(mounts(table).collect::<Vec<_>>(), [read]);
}

#[test]
fn is_mounted_as_is_false_for_the_same_mount() {
    let line = zfs_mount(60, b"/zfsbootmenu/a");
    let again = zfs_mount(60, b"/elsewhere");

    assert!(!line.is_mounted_as(&line));
    assert!(!again.is_mounted_as(&line));
}

#[test]
fn is_mounted_as_is_true_for_another_mount_of_the_same_source() {
    let first = zfs_mount(60, b"/zfsbootmenu/a");
    let second = zfs_mount(61, b"/zfsbootmenu/a");
    let other_root = Listed {
        root: b"/sub".to_vec(),
        ..zfs_mount(62, b"/zfsbootmenu/a")
    };
    let other_source = Listed {
        source: b"tank/other".to_vec(),
        ..zfs_mount(63, b"/zfsbootmenu/a")
    };

    assert!(second.is_mounted_as(&first));
    assert!(first.is_mounted_as(&second));
    assert!(!other_root.is_mounted_as(&first));
    assert!(!other_source.is_mounted_as(&first));
}

/// A mount `id` of `tank/a`'s root directory on `point`.
fn zfs_mount(id: u64, point: &[u8]) -> Listed {
    Listed {
        id,
        root: b"/".to_vec(),
        point: point.to_vec(),
        kind: b"zfs".to_vec(),
        source: b"tank/a".to_vec(),
    }
}

/// The ID and the mount point of each mount of `table`.
fn points(table: &[u8]) -> Vec<(u64, Vec<u8>)> {
    mounts(table)
        .map(|listed| (listed.id, listed.point))
        .collect()
}

/// `field` as the kernel writes it in mountinfo.
fn escaped(field: &[u8]) -> Vec<u8> {
    let mut written = Vec::new();
    for &byte in field {
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\\') {
            written.extend(format!("\\{byte:03o}").bytes());
        } else {
            written.push(byte);
        }
    }
    written
}

/// A field of a line as the kernel writes it: arbitrary bytes, never
/// empty, since an empty field would not be one.
fn field() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(any::<u8>(), 1..32)
}

/// A mount as [`mounts`] reads it, from arbitrary fields.
fn listed() -> impl Strategy<Value = Listed> {
    (any::<u64>(), field(), field(), field(), field()).prop_map(
        |(id, root, point, kind, source)| Listed {
            id,
            root,
            point,
            kind,
            source,
        },
    )
}

/// The optional fields the kernel may write before `-`.
fn optional() -> impl Strategy<Value = Vec<&'static str>> {
    let tag = prop_oneof![
        Just("shared:1"),
        Just("master:2"),
        Just("propagate_from:3"),
        Just("unbindable"),
    ];
    proptest::collection::vec(tag, 0..4)
}

/// `listed`'s line as the kernel writes it, with `optional` fields.
fn line(listed: &Listed, optional: &[&str]) -> Vec<u8> {
    let mut written = format!("{} 1 0:1 ", listed.id).into_bytes();
    written.extend(escaped(&listed.root));
    written.push(b' ');
    written.extend(escaped(&listed.point));
    written.extend(b" rw,relatime");
    for field in optional {
        written.push(b' ');
        written.extend(field.bytes());
    }
    written.extend(b" - ");
    written.extend(escaped(&listed.kind));
    written.push(b' ');
    written.extend(escaped(&listed.source));
    written.extend(b" rw\n");
    written
}

proptest! {
    #[test]
    fn escaped_lines_are_read_back_exactly(
        lines in proptest::collection::vec((listed(), optional()), 0..8),
    ) {
        // A dropped or misread line would make a check of the table fail
        // open: each line comes back, as it was mounted, in its order.
        let table: Vec<u8> = lines
            .iter()
            .flat_map(|(listed, optional)| line(listed, optional))
            .collect();
        let expected: Vec<Listed> = lines.into_iter().map(|(listed, _)| listed).collect();
        prop_assert_eq!(mounts(&table).collect::<Vec<_>>(), expected);
    }

    #[test]
    fn the_mountinfo_reader_never_panics(
        noise in proptest::collection::vec(any::<u8>(), 0..512),
        id in any::<u64>(),
        point in proptest::collection::vec(any::<u8>(), 1..64),
    ) {
        // Any bytes at all are read without a panic.
        let _ = mounts(&noise).count();
        // A line as the kernel writes it is read back as it was mounted,
        // whatever bytes its mount point has.
        let mut table = format!("{id} 1 0:1 / ").into_bytes();
        table.extend(escaped(&point));
        table.extend(b" rw shared:1 - zfs ");
        table.extend(escaped(&point));
        table.extend(b" rw\n");
        let read = Listed {
            id,
            root: b"/".to_vec(),
            point: point.clone(),
            kind: b"zfs".to_vec(),
            source: point,
        };
        prop_assert_eq!(mounts(&table).collect::<Vec<_>>(), [read]);
    }
}
