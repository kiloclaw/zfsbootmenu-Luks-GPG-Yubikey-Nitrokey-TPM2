#![allow(
    clippy::disallowed_methods,
    reason = "the text is said from a real instant, into a real file"
)]

use std::time::Instant;

use pretty_assertions::assert_eq;
use zbm_testkit::TempDir;

use super::*;
use crate::screen::tests::console;

#[test]
fn the_screen_goes_into_the_terminal_s_text_in_its_size_when_it_changed() {
    let dir = TempDir::new("zbm-vcsa");
    // A terminal of 2 rows of 3 columns, as the kernel's header says.
    let path = dir.write("vcsa1", [2_u8, 3, 0, 0].repeat(4));
    let mut text = VcsaText::open(&path).unwrap();
    assert_eq!(text.size(), (2, 3));
    let mut console = console();
    console.feed(b"hey you");

    let now = Instant::now();
    text.show(&console, now);
    let written = std::fs::read(&path).unwrap();
    assert_eq!(written[..HEADER], [2, 3, 3 - 1, 0]);
    let chars: Vec<u8> = written[HEADER..HEADER + 12]
        .iter()
        .step_by(2)
        .copied()
        .collect();
    assert_eq!(
        chars,
        b"heyyou"[..3]
            .iter()
            .chain(b"   ")
            .copied()
            .collect::<Vec<_>>()
    );

    // The same screen is not written again; an announcement is.
    std::fs::write(&path, [2_u8, 3, 0, 0]).unwrap();
    text.show(&console, now);
    assert_eq!(std::fs::read(&path).unwrap().len(), HEADER);
    console.announce("Done", now);
    text.show(&console, now);
    let written = std::fs::read(&path).unwrap();
    let last: Vec<u8> = written[HEADER + 6..].iter().step_by(2).copied().collect();
    assert_eq!(last, b"Don");
    assert!(VcsaText::open(&dir.join("missing")).is_err());
}

#[test]
fn a_terminal_s_text_that_cannot_be_written_is_written_no_more() {
    // `/dev/full` reads as zeros, a terminal of no size, and takes nothing.
    let mut text = VcsaText::open(std::path::Path::new("/dev/full")).unwrap();
    assert_eq!(text.size(), (0, 0));
    assert!(format!("{text:?}").contains("size: (0, 0)"));
    let console = console();

    text.show(&console, Instant::now());
    assert!(text.is_gone);
    text.show(&console, Instant::now());

    assert!(text.written.is_empty());
}
