//! The EDID of a display and a console font (input I13 of
//! docs/dev/threat-model.md): a display or adapter someone plugs in (A2)
//! chooses the EDID, and a changed image (A1) the fonts. The same bytes go to
//! the parsers, a parsed EDID through the font choice, and a parsed font
//! through the 256-glyph subset. Parsing must never panic or hang.

#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use zbm_startup::autosize::choice::{Density, Request, order};
use zbm_startup::autosize::{edid, font};
use zbm_types::psf::{Font, repertoire};

fuzz_target!(|data: &[u8]| {
    if let Ok(psf) = Font::parse(data) {
        let subset = psf.subset(&repertoire()).font;
        let _ = Font::parse(&subset.to_bytes());
    }
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
