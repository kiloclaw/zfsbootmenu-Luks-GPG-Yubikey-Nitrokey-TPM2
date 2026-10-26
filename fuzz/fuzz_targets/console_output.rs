//! What the programs write to the console on the display (input I14 of
//! docs/dev/threat-model.md): the names and files a pool controls (A3) reach
//! the screen through the programs that print them, escape sequences
//! included. The first byte splits the rest into reads of that many bytes,
//! as the pseudo-terminal gives them; each read is fed to the terminal and
//! drawn, scrolled screens and the cursor included. Neither may panic or
//! hang.

#![no_main]

use std::time::Instant;

use libfuzzer_sys::fuzz_target;
use zbm_console::screen::Console;
use zbm_display::grid::Grid;
use zbm_types::psf::Font;

/// The image's Terminus 8×14.
const FONT: &[u8] = include_bytes!("../../legacy/zfsbootmenu/fonts/ter-v14b.psf");

fuzz_target!(|data: &[u8]| {
    let Some((&first, output)) = data.split_first() else {
        return;
    };
    let Ok(font) = Font::parse(FONT) else {
        return;
    };
    // A small screen: 12 columns and 5 rows, so that lines wrap and scroll.
    let mut console = Console::new(Grid::new(font, 1), (12 * 8, 5 * 14));
    for read in output.chunks(usize::from(first).max(1)) {
        console.feed(read);
        let _ = console.take_replies();
        console.tick(Instant::now());
        console.draw();
        let _ = console.frame_mut().take_damage();
    }
    let _ = console.text();
});
