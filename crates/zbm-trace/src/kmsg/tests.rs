//! The kernel log layer and its formats.

use super::*;
use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex, PoisonError};
use tracing_subscriber::layer::SubscriberExt;

/// A sink that keeps what it is given.
#[derive(Clone, Default)]
struct Kept {
    lines: Arc<Mutex<Vec<String>>>,
    markers: Arc<Mutex<Vec<String>>>,
}

impl KmsgSink for Kept {
    fn record(&self, line: &str) {
        let mut lines = self.lines.lock().unwrap_or_else(PoisonError::into_inner);
        lines.push(line.to_owned());
    }

    fn mark(&self, marker: &str) {
        let mut markers = self.markers.lock().unwrap_or_else(PoisonError::into_inner);
        markers.push(marker.to_owned());
    }
}

impl Kept {
    fn text(&self) -> String {
        self.lines.lock().unwrap().concat()
    }
}

fn with_layer(layer: KmsgLayer<Kept>, body: impl FnOnce()) {
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, body);
}

#[test]
fn levels_match_the_kernel_log() {
    let numbers: Vec<i64> = [
        Level::Error,
        Level::Warning,
        Level::Notice,
        Level::Info,
        Level::Debug,
    ]
    .iter()
    .map(|level| level.number())
    .collect();
    assert_eq!(numbers, [3, 4, 5, 6, 7]);
    assert_eq!(Message::warning("w").stderr_line(), "WARN: w");
    assert_eq!(parse_level("+7"), Some(7));
    assert_eq!(parse_level("high"), None);
    // `${loglevel:-4}`: empty or not set is 4.
    assert_eq!(parse_level(""), Some(DEFAULT_LEVEL));
    assert_eq!(parse_level("  "), Some(4));
}

#[test]
fn kernel_log_lines_follow_kmsg_log_lib() {
    assert_eq!(
        kmsg_lines(Level::Info, "hello", 7, "t"),
        ["<6>ZFSBootMenu: hello"]
    );
    assert_eq!(
        kmsg_lines(Level::Error, "a\nb", 7, "t"),
        ["<3>ZFSBootMenu: a\nb"]
    );
    assert_eq!(
        kmsg_lines(Level::Debug, "one\ntwo", 7, "f,1"),
        ["<7>ZBM:[7]||one", "<7>ZBM:[7]|f,1|two"]
    );
    assert_eq!(kmsg_lines(Level::Debug, "", 7, "f,1"), ["<7>ZBM:[7]|f,1|"]);
    assert_eq!(
        kmsg_lines(Level::Debug, "x\n", 7, "f,1"),
        ["<7>ZBM:[7]||x", "<7>ZBM:[7]|f,1|"]
    );
    // Long text goes in several records the kernel takes, cut between
    // characters.
    let long = "é".repeat(800);
    let records = kmsg_lines(Level::Warning, &long, 7, "t");
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|record| record.len() <= RECORD));
    assert!(
        records
            .iter()
            .all(|record| record.starts_with("<4>ZFSBootMenu: "))
    );
    let joined: String = records
        .iter()
        .map(|record| record.trim_start_matches("<4>ZFSBootMenu: "))
        .collect();
    assert_eq!(joined, long);
    let debug = kmsg_lines(Level::Debug, &"x".repeat(2000), 7, "f,1");
    assert_eq!(debug.len(), 3);
    assert!(debug[0].starts_with("<7>ZBM:[7]||"));
    assert!(debug[2].starts_with("<7>ZBM:[7]|f,1|"));
}

#[test]
fn writes_records_and_markers_that_the_level_allows() {
    let kept = Kept::default();

    with_layer(KmsgLayer::new(kept.clone(), "6", 42), || {
        crate::info!("importing tank");
        crate::debug!("not shown");
        crate::warn!("canmount=on set for 'tank/ROOT'");
        crate::error!("unable to import a pool");
        crate::notice!("note");
        tracing::warn!("a journal event");
        assert!(enabled(Level::Info) && !enabled(Level::Debug));
    });

    assert_eq!(
        kept.text(),
        "<6>ZFSBootMenu: importing tank\n\
         <4>ZFSBootMenu: canmount=on set for 'tank/ROOT'\n\
         <3>ZFSBootMenu: unable to import a pool\n\
         <5>ZFSBootMenu: note\n"
    );
    assert_eq!(
        *kept.markers.lock().unwrap(),
        ["have_warnings", "have_errors"]
    );
}

#[test]
fn debugging_output_names_the_caller() {
    let kept = Kept::default();

    let line = line!() + 2;
    with_layer(KmsgLayer::new(kept.clone(), "7", 42), || {
        crate::debug!("one\ntwo");
        Message::debug("three").emit();
    });

    assert_eq!(
        kept.text(),
        format!(
            "<7>ZBM:[42]||one\n<7>ZBM:[42]|crates/zbm-trace/src/kmsg/tests.rs,{line}|two\n\
             <7>ZBM:[42]|crates/zbm-trace/src/kmsg/tests.rs,{}|three\n",
            line + 1
        )
    );
}

#[test]
fn labelled_lines_are_written_as_the_suites_expect() {
    let kept = Kept::default();

    with_layer(KmsgLayer::new(kept.clone(), "0", 42).labelled(), || {
        Message::info("one").emit();
        Message::warning("two\nlines").emit();
        Message::notice("three").emit();
        Message::error("four").emit();
        crate::debug!("");
        crate::debug!("five");
    });

    assert_eq!(
        kept.text(),
        "INFO: one\nWARN: two\nlines\nNOTICE: three\nERROR: four\nDEBUG: five\n"
    );
}

#[test]
fn without_a_level_errors_and_warnings_are_written() {
    let kept = Kept::default();

    with_layer(KmsgLayer::new(kept.clone(), "", 1), || {
        crate::error!("kept");
        crate::info!("left out");
    });

    assert_eq!(kept.text(), "<3>ZFSBootMenu: kept\n");
    assert_eq!(*kept.markers.lock().unwrap(), ["have_errors"]);
}

#[test]
fn invalid_levels_write_nothing() {
    let kept = Kept::default();

    with_layer(KmsgLayer::new(kept.clone(), "loud", 1), || {
        crate::error!("lost");
        assert!(!enabled(Level::Error));
    });

    assert_eq!(kept.text(), "");
    assert!(kept.markers.lock().unwrap().is_empty());
}
