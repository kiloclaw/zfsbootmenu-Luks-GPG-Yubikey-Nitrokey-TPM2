//! Tests of the reader of `zpool status -j` on hand-written documents.
//! The keys of the classes (`logs`, `l2cache`, `spares`, ...), `was` and
//! `aux` are those of `zpool_main.c`; the recorded pool has none of them,
//! and `tests/adapter.rs` reads the recording.

use pretty_assertions::assert_eq;

use super::*;

/// A vdev object, with `inner` added to its fields.
fn vdev(name: &str, vdev_type: &str, class: &str, inner: &str) -> String {
    format!(
        r#""{name}":{{"name":"{name}","vdev_type":"{vdev_type}","class":"{class}",
        "state":"ONLINE","read_errors":"0","write_errors":1,"checksum_errors":"2"{inner}}}"#
    )
}

#[test]
fn the_order_of_the_document_is_kept_and_classes_follow_the_data() {
    let leaf = |name: &str| vdev(name, "disk", "normal", "");
    let mirror = vdev(
        "mirror-0",
        "mirror",
        "normal",
        &format!(r#","vdevs":{{{},{}}}"#, leaf("/dev/z"), leaf("/dev/a")),
    );
    let log = vdev("/dev/log0", "disk", "log", "");
    let root = vdev(
        "tank",
        "root",
        "normal",
        &format!(r#","vdevs":{{{mirror},{}}}"#, leaf("/dev/m")),
    );
    let document = format!(
        r#"{{"pools":{{"tank":{{"name":"tank","state":"DEGRADED",
        "status":"One or more devices could not be opened.  Sufficient replicas\n\tfor the pool.\n",
        "action":"Attach it.","error_count":"4","vdevs":{{{root}}},
        "logs":{{{log}}},"l2cache":{{{}}},"spares":{{{}}},"special":{{{}}}}}}}}}"#,
        leaf("/dev/cache"),
        leaf("/dev/spare"),
        leaf("/dev/special"),
    );

    let pools = status(&document).unwrap();

    let tank = &pools[0];
    assert_eq!(tank.state, Health::Degraded);
    assert_eq!(tank.errors, Some(4));
    assert_eq!(
        tank.status.as_ref().unwrap().get(),
        "One or more devices could not be opened.  Sufficient replicas for the pool."
    );
    let top: Vec<(&str, VdevClass)> = tank
        .vdevs
        .iter()
        .map(|vdev| (vdev.name.get().as_str(), vdev.class))
        .collect();
    assert_eq!(
        top,
        [
            ("mirror-0", VdevClass::Data),
            ("/dev/m", VdevClass::Data),
            ("/dev/special", VdevClass::Special),
            ("/dev/log0", VdevClass::Log),
            ("/dev/cache", VdevClass::Cache),
            ("/dev/spare", VdevClass::Spare),
        ]
    );
    let mirror = &tank.vdevs[0];
    assert_eq!(mirror.kind, VdevKind::Mirror);
    let children: Vec<&str> = mirror
        .children
        .iter()
        .map(|c| c.name.get().as_str())
        .collect();
    assert_eq!(children, ["/dev/z", "/dev/a"]);
    assert_eq!(
        mirror.children[0].errors,
        Some(VdevErrors {
            read: 0,
            write: 1,
            cksum: 2
        })
    );
    assert_eq!(mirror.children[0].kind, VdevKind::Device);
}

#[test]
fn a_missing_device_keeps_its_path_and_counts_may_be_missing() {
    let document = r#"{"pools":{"t":{"name":"t","state":"ONLINE","vdevs":{
        "x":{"name":"123","vdev_type":"disk","state":"UNAVAIL","was":"/dev/sdb1",
        "read_errors":"1.2K"}}}}}"#;

    let pools = status(document).unwrap();

    let device = &pools[0].vdevs[0];
    assert_eq!(device.note.as_ref().unwrap().get(), "was /dev/sdb1");
    assert_eq!(device.errors, None);
    assert_eq!(device.state, Some(Health::Unavailable));
    assert_eq!(pools[0].errors, None);
    assert_eq!(pools[0].status, None);
    assert_eq!(one_line(Some(" \n".to_owned())), None);
    assert_eq!(VdevKind::of_type("hole", true), VdevKind::Other);
}

#[test]
fn invalid_documents_and_names_are_errors() {
    assert!(status("").unwrap().is_empty());
    assert!(status(r#"{"pools":{}}"#).unwrap().is_empty());
    assert!(status(r#"{"pools":[]}"#).is_err());
    assert_eq!(
        status(r#"{"pools":{"a/b":{"name":"a/b"}}}"#)
            .unwrap_err()
            .to_string(),
        "invalid pool name \"a/b\""
    );
    assert!(status(r#"{"pools":{"t":{"name":"t","vdevs":{"x":{"name":3}}}}}"#).is_err());
}

/// A pool whose vdevs are `vdevs`, the fields of each of which are given.
fn pool_of(vdevs: &str) -> String {
    format!(r#"{{"pools":{{"t":{{"name":"t","state":"DEGRADED","vdevs":{{{vdevs}}}}}}}}}"#)
}

#[test]
fn states_and_reasons_are_the_words_of_the_text() {
    let leaf = |name: &str, fields: &str| {
        format!(
            r#""{name}":{{"name":"{name}","vdev_type":"disk","read_errors":"0",
            "write_errors":"0","checksum_errors":"0",{fields}}}"#
        )
    };
    let leaves = [
        leaf("a", r#""state":"CANT_OPEN","aux":"OPEN_FAILED""#),
        leaf("b", r#""state":"CANT_OPEN","aux":"CORRUPT_DATA""#),
        leaf("c", r#""state":"CANT_OPEN","aux":"SPLIT_POOL""#),
        leaf(
            "d",
            r#""state":"FAULTED","aux":"ERR_EXCEEDED","slow_ios":"0""#,
        ),
        leaf(
            "e",
            r#""state":"FAULTED","aux":"ERR_EXCEEDED","slow_ios":"9""#,
        ),
        leaf("f", r#""state":"CLOSED""#),
        leaf("g", r#""state":"ONLINE","aux":"SPARED","used_by":"other""#),
        leaf("h", r#""state":"INUSE","aux":"SPARED""#),
        leaf("i", r#""state":"CANT_OPEN","aux":"SOMETHING_NEW""#),
    ];
    let document = pool_of(&leaves.join(","));

    let pools = status(&document).unwrap();

    let read: Vec<(Health, Option<&str>)> = pools[0]
        .vdevs
        .iter()
        .map(|vdev| {
            (
                vdev.state.clone().unwrap(),
                vdev.note.as_ref().map(|note| note.get().as_str()),
            )
        })
        .collect();
    let other = |word: &str| Health::Other(word.to_owned());
    assert_eq!(
        read,
        [
            (Health::Unavailable, Some("cannot open")),
            (Health::Faulted, Some("corrupted data")),
            (other("SPLIT"), Some("split into new pool")),
            (Health::Faulted, Some("too many errors")),
            (Health::Faulted, Some("too many slow I/Os")),
            (Health::Offline, None),
            (Health::Online, Some("in use by pool 'other'")),
            (other("INUSE"), Some("currently in use")),
            (Health::Unavailable, Some("corrupted data")),
        ]
    );
}

#[test]
fn every_reason_has_the_note_of_the_text() {
    let notes: Vec<String> = [
        "BAD_GUID_SUM",
        "NO_REPLICAS",
        "VERSION_NEWER",
        "UNSUP_FEAT",
        "ASHIFT_TOO_BIG",
        "IO_FAILURE",
        "BAD_LOG",
        "EXTERNAL",
        "ACTIVE",
        "CHILDREN_OFFLINE",
        "BAD_LABEL",
    ]
    .iter()
    .map(|aux| aux_note(aux, false, None))
    .collect();
    assert_eq!(
        notes,
        [
            "missing device",
            "insufficient replicas",
            "newer version",
            "unsupported feature(s)",
            "unsupported minimum blocksize",
            "experienced I/O failures",
            "bad intent log",
            "external device fault",
            "currently in use",
            "all children offline",
            "invalid label",
        ]
    );
    assert_eq!(state_word("DEGRADED", Some("NO_REPLICAS")), "DEGRADED");
}
