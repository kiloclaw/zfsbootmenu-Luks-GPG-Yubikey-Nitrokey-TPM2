//! The EDID of a display and the header of a console font (input I13 of
//! docs/dev/threat-model.md): a display or adapter someone plugs in (A2)
//! chooses the EDID, and a changed image (A1) the fonts. The same bytes go to
//! both parsers, and a parsed EDID through the font choice. Parsing must
//! never panic or hang.

#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use zbm_startup::autosize::choice::{Density, Request, order};
use zbm_startup::autosize::{edid, font};

fuzz_target!(|data: &[u8]| {
    let cell = font::cell(data);
    if let Some(edid) = edid::parse(data) {
        let screen = edid.mode.unwrap_or((edid.width_mm, edid.height_mm));
        let density = Density::new(screen, &edid);
        if let Some(density) = density {
            let _ = density.dpi();
        }
        let fonts: Vec<(PathBuf, font::Cell)> = cell
            .into_iter()
            .map(|cell| (PathBuf::from("f.psf"), cell))
            .collect();
        let _ = order(&fonts, Request::Auto, density);
    }
});
