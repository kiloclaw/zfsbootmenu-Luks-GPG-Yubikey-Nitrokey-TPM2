use pretty_assertions::assert_eq;
use zbm_testkit::fakes::{FakeFiles, FakeFonts};

use super::*;

fn args(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

/// A whole PSF 2 font of 8×16 cells: 256 blank glyphs, no table.
fn psf() -> Vec<u8> {
    let mut bytes = vec![0x72, 0xb5, 0x4a, 0x86];
    for field in [0_u32, 32, 0, 256, 16, 16, 8] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes.resize(32 + 16 * 256, 0);
    bytes
}

#[test]
fn the_command_line_names_the_console_the_doubling_and_one_font() {
    assert_eq!(
        parse(&args("-C /dev/tty2 -d /f/ter.psf")),
        Some(Request {
            console: PathBuf::from("/dev/tty2"),
            doubled: true,
            font: PathBuf::from("/f/ter.psf"),
        })
    );
    assert_eq!(
        parse(&args("/f/ter.psf")).unwrap().console,
        Path::new("/dev/tty0")
    );
    for wrong in ["", "-C", "-x /f/ter.psf", "/f/a.psf /f/b.psf"] {
        assert_eq!(parse(&args(wrong)), None, "{wrong}");
    }
}

#[test]
fn the_font_is_loaded_doubled_or_why_not_is_said() {
    let files = FakeFiles::new();
    files.file("/f/ter.psf", psf());
    files.file("/f/broken.psf", "x");
    let fonts = FakeFonts::default();
    let request = |font: &str, doubled| Request {
        console: PathBuf::from("/dev/tty1"),
        doubled,
        font: PathBuf::from(font),
    };

    load(&files, &fonts, &request("/f/ter.psf", true)).unwrap();
    let broken = load(&files, &fonts, &request("/f/broken.psf", false)).unwrap_err();
    let missing = load(&files, &fonts, &request("/f/none.psf", false)).unwrap_err();
    fonts.refuse_taller_than(16);
    let refused = load(&files, &fonts, &request("/f/ter.psf", true)).unwrap_err();

    assert_eq!(
        fonts.loaded(),
        [(PathBuf::from("/dev/tty1"), (16, 32), 256)]
    );
    assert!(broken.starts_with("/f/broken.psf: "), "{broken}");
    assert!(missing.starts_with("/f/none.psf: "), "{missing}");
    assert!(refused.starts_with("/dev/tty1: "), "{refused}");
    let mut stderr = Vec::new();
    let mut stdout = Vec::new();
    let mut io = Io {
        stdout: &mut stdout,
        stderr: &mut stderr,
    };
    assert_eq!(run(&args("-x"), &mut io), crate::USAGE);
    assert_eq!(run(&args("/nonexistent/font.psf"), &mut io), 1);
}
