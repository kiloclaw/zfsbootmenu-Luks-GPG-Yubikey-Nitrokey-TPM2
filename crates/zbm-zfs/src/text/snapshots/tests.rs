//! Tests of the readers of the snapshots, holds and destroys a review
//! reads, on hand-written lines; `tests/review.rs` reads whole outputs.

use pretty_assertions::assert_eq;

use super::*;

/// A detail as a test compares it: name, createtxg, used, userrefs and
/// the number of clones.
type Row = (String, u64, Option<u64>, u64, usize);

#[test]
fn snapshots_and_bookmarks_come_with_their_space_holds_and_clones() {
    let text = "tank/ROOT/be@a\t100\t4096\t0\t\n\
                tank/ROOT/be#a\t100\t-\t-\t-\n\
                tank/ROOT/be@b\t101\t0\t2\ttank/ROOT/copy,tank/other\n\
                tank/ROOT/be@c\t102\t512\t0\t-\n";

    let found = snapshot_details(text).unwrap();

    let shown: Vec<Row> = found
        .iter()
        .map(|detail| {
            (
                detail.name.to_string(),
                detail.createtxg,
                detail.used,
                detail.userrefs,
                detail.clones.len(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("tank/ROOT/be@a".to_owned(), 100, Some(4096), 0, 0),
            ("tank/ROOT/be#a".to_owned(), 100, None, 0, 0),
            ("tank/ROOT/be@b".to_owned(), 101, Some(0), 2, 2),
            ("tank/ROOT/be@c".to_owned(), 102, Some(512), 0, 0),
        ]
    );
    assert_eq!(found[2].clones[1].get(), "tank/other");
    // The bookmark `a` is not the snapshot `a`.
    assert_ne!(found[0].name, found[1].name);
    assert!(matches!(
        &found[1].name,
        SnapshotOrBookmark::Bookmark { dataset, name }
            if dataset.as_str() == "tank/ROOT/be" && name == "a"
    ));
}

#[test]
fn no_snapshot_is_an_empty_list_and_odd_lines_are_errors() {
    assert_eq!(snapshot_details("").unwrap(), []);
    assert_eq!(snapshot_details("\n").unwrap(), []);
    let reason = |text: &str| snapshot_details(text).unwrap_err().reason;
    assert_eq!(reason("tank/ROOT/be@a\t1\t2\t0\n"), "not five fields");
    assert_eq!(
        reason("tank/ROOT/be\t1\t2\t0\t-\n"),
        "invalid snapshot or bookmark name"
    );
    assert_eq!(
        reason("tank/ROOT/be@a#b\t1\t2\t0\t-\n"),
        "invalid snapshot or bookmark name"
    );
    assert_eq!(
        reason("tank/ROOT/be#a#b\t1\t2\t0\t-\n"),
        "invalid snapshot or bookmark name"
    );
    assert_eq!(
        reason("tank/ROOT/be@a\tlate\t2\t0\t-\n"),
        "invalid transaction group"
    );
    assert_eq!(reason("tank/ROOT/be@a\t1\tbig\t0\t-\n"), "invalid size");
    assert_eq!(
        reason("tank/ROOT/be@a\t1\t2\tmany\t-\n"),
        "invalid number of holds"
    );
    // A row that does not read is left out when others do.
    let mixed = "tank/ROOT/be@a\t1\t2\t0\t-\nodd\n";
    assert_eq!(snapshot_details(mixed).unwrap().len(), 1);
}

#[test]
fn a_hold_is_its_snapshot_and_its_tag_whatever_it_holds() {
    let text = "tank/ROOT/be@a\tkeep\t1792667543\n\
                tank/ROOT/be@b\twith\ttab\t1792667544\n";

    let found = holds(text).unwrap();

    assert_eq!(found.len(), 2);
    assert_eq!(found[0].snapshot.to_string(), "tank/ROOT/be@a");
    assert_eq!(found[0].tag.get(), "keep");
    assert_eq!(found[1].tag.get(), "with\ttab");
    assert_eq!(holds("").unwrap(), []);
    let reason = |text: &str| holds(text).unwrap_err().reason;
    assert_eq!(reason("tank/ROOT/be@a\t1\n"), "not three fields");
    assert_eq!(reason("tank/ROOT/be@a\n"), "not three fields");
    assert_eq!(reason("tank/ROOT/be\tkeep\t1\n"), "invalid snapshot name");
    assert_eq!(reason("tank/ROOT/be@a\tkeep\tsoon\n"), "invalid time");
}

#[test]
fn the_space_a_destroy_frees_is_its_reclaim_line() {
    let text = "destroy\ttank/ROOT/be@a\ndestroy\ttank/ROOT/be@b\nreclaim\t8192\n";
    assert_eq!(reclaim(text), Ok(8192));
    assert_eq!(reclaim("reclaim\t0\n"), Ok(0));
    let reason = |text: &str| reclaim(text).unwrap_err().reason;
    assert_eq!(reason(""), "no reclaim");
    assert_eq!(reason("destroy\ttank/ROOT/be@a\n"), "no reclaim");
    assert_eq!(reason("would reclaim 8K\n"), "no reclaim");
    assert_eq!(reason("reclaim\tmuch\n"), "invalid space");
}
