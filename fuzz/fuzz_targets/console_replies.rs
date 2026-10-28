//! The replies of the console on the display (input I14 of
//! docs/dev/threat-model.md). A program's output, such as a file a pool
//! holds being shown (A3), may ask the terminal for its colours, its size
//! or where its cursor is; the console answers into the terminal's input,
//! where the menu reads keys. Whatever the output, the answers decode to no
//! key: else a file could type into the menu, or press Enter in it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zbm_console::screen::Console;
use zbm_display::grid::Grid;
use zbm_display::psf::Font;
use zbm_tui::menu::term_keys::decode;

/// The image's Terminus 8×14.
const FONT: &[u8] = include_bytes!("../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

fuzz_target!(|data: &[u8]| {
    let font = Font::parse(FONT).expect("the image's font parses");
    let mut console = Console::new(Grid::new(font, 1), (40 * 8, 10 * 14));
    console.feed(data);
    let replies = console.take_replies();
    let mut keys = Vec::new();
    assert_eq!(decode(&replies, true, &mut keys), replies.len());
    assert!(keys.is_empty(), "{keys:?} pressed by {replies:?}");
});
