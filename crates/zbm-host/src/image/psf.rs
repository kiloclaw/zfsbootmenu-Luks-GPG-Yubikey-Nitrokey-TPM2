//! The console fonts of an image as 256-glyph subsets (ROADMAP.md #20): a
//! console font of more than 256 glyphs costs colours; the subsets keep the
//! characters the menu and the shell need (`zbm_display::psf`).

use std::io;
use std::path::Path;

use zbm_display::psf::{Font, SUBSET_GLYPHS, repertoire};
use zbm_platform::Files;

/// What became of one font.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Done {
    /// Written as a subset; with the characters it could not draw.
    Subset {
        /// The glyphs of the source.
        from: usize,
        /// Characters neither the font nor a look-alike in it could draw.
        missing: Vec<char>,
    },
    /// Copied as it was: it has no more than 256 glyphs.
    Small,
    /// Copied as it was: it could not be read as a PSF font.
    Unreadable(String),
}

/// Each PSF font of `source` (`*.psf`, `*.psfu`), by name and in byte
/// order, as a subset where it needs one: its bytes and what became of it.
///
/// # Errors
///
/// An error reading `source`.
pub fn subsets(files: &dyn Files, source: &Path) -> io::Result<Vec<(String, Vec<u8>, Done)>> {
    let mut names: Vec<String> = files
        .read_dir(source)?
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "psf" || extension == "psfu")
        })
        .collect();
    names.sort();
    let mut fonts = Vec::new();
    for name in names {
        let bytes = files.read(&source.join(&name))?;
        let (output, done) = match Font::parse(&bytes) {
            Ok(font) if font.glyph_count() <= SUBSET_GLYPHS => (bytes, Done::Small),
            Ok(font) => {
                let subset = font.subset(&repertoire());
                let done = Done::Subset {
                    from: font.glyph_count(),
                    missing: subset.missing,
                };
                (subset.font.to_bytes(), done)
            }
            Err(error) => (bytes, Done::Unreadable(error.to_string())),
        };
        fonts.push((name, output, done));
    }
    Ok(fonts)
}
