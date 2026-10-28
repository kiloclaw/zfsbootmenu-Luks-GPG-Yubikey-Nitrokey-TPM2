//! `zbm console-fonts <source> <destination>`: the console fonts of an image
//! as 256-glyph subsets (ROADMAP.md #20), which the image build runs for
//! images with the Rust runtime. A console font of more than 256 glyphs costs
//! colours; the subsets keep the characters the menu and the shell need
//! (`zbm_display::psf`).

use std::io;
use std::path::Path;

use zbm_display::psf::{Font, SUBSET_GLYPHS, repertoire};
use zbm_platform::{Files, RealFiles};

use crate::Io;

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

/// Writes each PSF font of `source` (`*.psf`, `*.psfu`) to `destination`,
/// under its name, as a subset where it needs one; returns what became of
/// each, by name.
///
/// # Errors
///
/// An error reading `source` or writing `destination`.
pub fn convert(
    files: &dyn Files,
    source: &Path,
    destination: &Path,
) -> io::Result<Vec<(String, Done)>> {
    let mut names: Vec<String> = files
        .read_dir(source)?
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.ends_with(".psf") || name.ends_with(".psfu"))
        .collect();
    names.sort();
    files.create_dir_all(destination)?;
    let mut report = Vec::new();
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
        files.write(&destination.join(&name), &output)?;
        report.push((name, done));
    }
    Ok(report)
}

/// The line `console-fonts` prints for one font.
fn line(name: &str, done: &Done) -> String {
    match done {
        Done::Subset { from, missing } if missing.is_empty() => {
            format!("{name}: {from} glyphs, kept {SUBSET_GLYPHS}")
        }
        Done::Subset { from, missing } => {
            let missing: String = missing.iter().collect();
            format!("{name}: {from} glyphs, kept {SUBSET_GLYPHS}; it cannot draw {missing}")
        }
        Done::Small => format!("{name}: {SUBSET_GLYPHS} glyphs or fewer, copied"),
        Done::Unreadable(why) => format!("{name}: {why}, copied as it is"),
    }
}

/// `zbm console-fonts <source> <destination>`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let [source, destination] = args else {
        let _ = writeln!(
            io.stderr,
            "Usage: zbm console-fonts <source directory> <destination directory>"
        );
        return crate::USAGE;
    };
    match convert(&RealFiles, Path::new(source), Path::new(destination)) {
        Ok(report) => {
            for (name, done) in report {
                let _ = writeln!(io.stdout, "{}", line(&name, &done));
            }
            0
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "console-fonts: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use zbm_testkit::fakes::FakeFiles;

    use super::*;

    const TER_V32B: &[u8] = include_bytes!("../../../legacy/zfsbootmenu/fonts/ter-v32b.psf");

    /// A PSF 2 header of `count` 8×1 glyphs, without a table, and the glyphs.
    fn small(count: u32) -> Vec<u8> {
        let mut bytes = vec![0x72, 0xb5, 0x4a, 0x86];
        for field in [0, 32, 0, count, 1, 1, 8] {
            bytes.extend_from_slice(&u32::to_le_bytes(field));
        }
        bytes.extend(std::iter::repeat_n(0, usize::try_from(count).unwrap()));
        bytes
    }

    #[test]
    fn large_fonts_become_subsets_and_the_others_are_copied() {
        let files = FakeFiles::new();
        files
            .file("/src/ter-v32b.psf", TER_V32B)
            .file("/src/tiny.psfu", small(256))
            .file("/src/broken.psf", b"not a font")
            .file("/src/README", b"text");

        let report = convert(&files, Path::new("/src"), Path::new("/out")).unwrap();

        let names: Vec<&str> = report.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["broken.psf", "ter-v32b.psf", "tiny.psfu"]);
        let subset = Font::parse(&files.contents("/out/ter-v32b.psf").unwrap()).unwrap();
        assert_eq!(subset.glyph_count(), SUBSET_GLYPHS);
        assert!(subset.has('─') && subset.has('ü') && subset.has('▶'));
        assert_eq!(files.contents("/out/tiny.psfu"), Some(small(256)));
        assert_eq!(
            files.contents("/out/broken.psf"),
            Some(b"not a font".to_vec())
        );
        assert_eq!(files.contents("/out/README"), None);
        let lines: Vec<String> = report.iter().map(|(name, done)| line(name, done)).collect();
        assert_eq!(
            lines,
            [
                "broken.psf: not a PSF font, copied as it is",
                "ter-v32b.psf: 512 glyphs, kept 256",
                "tiny.psfu: 256 glyphs or fewer, copied",
            ]
        );
    }

    #[test]
    fn a_font_that_lacks_characters_says_which() {
        let done = Done::Subset {
            from: 512,
            missing: vec!['ñ', '€'],
        };

        assert_eq!(
            line("x.psf", &done),
            "x.psf: 512 glyphs, kept 256; it cannot draw ñ€"
        );
    }

    #[test]
    fn a_missing_source_is_an_error() {
        let files = FakeFiles::new();

        assert!(convert(&files, Path::new("/nowhere"), Path::new("/out")).is_err());
    }

    #[test]
    fn the_command_writes_the_subsets_and_says_what_it_did() {
        let fonts = zbm_testkit::repo_root().join("legacy/zfsbootmenu/fonts");
        let out = zbm_testkit::TempDir::new("console-fonts");
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let args = [
            fonts.display().to_string(),
            out.path().display().to_string(),
        ];

        assert_eq!(run(&args, &mut io), 0);
        assert!(String::from_utf8_lossy(&stdout).contains("ter-v32b.psf: 512 glyphs, kept 256\n"));
        let missing = ["/nonexistent".to_owned(), out.path().display().to_string()];
        let mut io = Io {
            stdout: &mut Vec::new(),
            stderr: &mut stderr,
        };
        assert_eq!(run(&missing, &mut io), 1);
        assert!(String::from_utf8_lossy(&stderr).starts_with("console-fonts: "));
    }

    #[test]
    fn the_command_needs_two_directories() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };

        assert_eq!(run(&["one".to_owned()], &mut io), crate::USAGE);
        assert!(String::from_utf8_lossy(&stderr).starts_with("Usage: zbm console-fonts"));
    }
}
