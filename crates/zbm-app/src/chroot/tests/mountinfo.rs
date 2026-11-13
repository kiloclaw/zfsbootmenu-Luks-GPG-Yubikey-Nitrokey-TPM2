//! The reader of the kernel's table of mounts (SB16).

use pretty_assertions::assert_eq;
use proptest::prelude::*;

use crate::chroot::root::{Listed, mounts};

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

/// The ID and the mount point of each mount of `table`.
fn points(table: &[u8]) -> Vec<(u64, Vec<u8>)> {
    mounts(table)
        .map(|listed| (listed.id, listed.point))
        .collect()
}

/// `point` as the kernel writes it in mountinfo.
fn escaped(point: &[u8]) -> Vec<u8> {
    let mut written = Vec::new();
    for &byte in point {
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\\') {
            written.extend(format!("\\{byte:03o}").bytes());
        } else {
            written.push(byte);
        }
    }
    written
}

proptest! {
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
