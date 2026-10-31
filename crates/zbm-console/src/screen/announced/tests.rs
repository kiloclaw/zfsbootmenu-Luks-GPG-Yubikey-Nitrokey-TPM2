#![allow(
    clippy::disallowed_methods,
    reason = "the lines are said from a real instant"
)]

use std::time::{Duration, Instant};

use pretty_assertions::assert_eq;

use super::*;
use crate::screen::Status;
use crate::screen::tests::console;

#[test]
fn a_line_is_said_a_while_on_the_status_line_in_place_of_the_title() {
    let now = Instant::now();
    let mut console = console().with_status_line();
    let status = Status::default();
    console.set_status(&status, now);
    assert_eq!(console.announcement(now), None);

    console.announce("Booting\u{7} tank/ROOT/be in 9 s", now);
    console.set_status(&status, now);

    assert_eq!(
        console.announcement(now),
        Some("Booting tank/ROOT/be in 9 s")
    );
    let line: String = console.status_text().unwrap().iter().collect();
    assert!(line.starts_with(" Booting tank/ROOT"), "{line:?}");
    // The status line is said again when the line has been said.
    assert_eq!(console.status_at(), Some(now + ANNOUNCED));
    let later = now + ANNOUNCED + Duration::from_millis(1);
    assert_eq!(console.announcement(later), None);
    console.set_status(&status, later);
    let line: String = console.status_text().unwrap().iter().collect();
    assert!(!line.contains("Booting"), "{line:?}");
}
