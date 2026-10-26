//! `zbm console`: the console on the display, without the kernel's
//! (ROADMAP.md #19, step 2), which the supervisor starts when
//! `zbm.console=drm` asks for it.
//!
//! Its standard input is the controlling side of the pseudo-terminal on
//! which the start-up program and the recovery shell run. It draws them on
//! the display through DRM with the image's console font, at the size
//! `zbm console-autosize` would choose, and sends them what is typed on the
//! keyboards, with the virtual terminal shown now taken over meanwhile.
//! `zbm console release`, before `kexec`, has it hand all of that back; it
//! then relays between the pseudo-terminal and the kernel console, which
//! it also does at once when it cannot use the display.
//!
//! ```text
//! zbm console [--card <path>] [--height <pixels>] [--psf] [--layout <name>]
//!             [--mirror <file>]
//! zbm console release [--timeout <milliseconds>]
//! zbm console reattach [--timeout <milliseconds>]
//! zbm console layout [<name>]
//! ```
//!
//! `zbm console reattach`, when `kexec` failed after the hand-over, has the
//! console take the display again, so that the error and the menu after it
//! are shown.
//!
//! Keys are translated with the image's first keyboard layout, or the one
//! `--layout` names (ROADMAP.md #19, step 4); `zbm console layout` names
//! the layout and the others of the image, or switches to another.
//!
//! The console draws with the image's TrueType font, in cells as high as
//! the console font `zbm console-autosize` would choose, with its bold,
//! oblique and bold oblique faces where the image has them, and the
//! fallback fonts of the image (`fonts/fallback/`, in name order) for the
//! characters it lacks; `--psf` draws with that console font itself.
//!
//! `--mirror` writes the screen's text to a file or device after each
//! change, after a form feed, for the VM tests.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_display::faces::{Faces, FontFace};
use zbm_display::grid::Grid;
use zbm_display::outline::Outline;
use zbm_platform::Files;
use zbm_startup::autosize::FONTS;

use zbm_console::serve::{Layouts, Places, Released, Settings, reattach, release, serve};
use zbm_platform::RealFiles;

use crate::{Io, USAGE};

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm console [--card <path>] [--height <pixels>] [--psf] [--layout <name>]
                   [--mirror <file>]
       zbm console release [--timeout <milliseconds>]
       zbm console reattach [--timeout <milliseconds>]
       zbm console layout [<name>]
";

/// How long `zbm console release` waits by default.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

/// What the command line asks for.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// The DRM card; the first otherwise.
    pub card: Option<PathBuf>,
    /// The font height asked for, as `zbm.autosize=<pixels>`.
    pub height: Option<u16>,
    /// Whether to draw with the console fonts instead of the TrueType font.
    pub psf: bool,
    /// The keyboard layout asked for; the image's first otherwise.
    pub layout: Option<String>,
    /// Where to write the screen's text.
    pub mirror: Option<PathBuf>,
}

/// The options in `args`, or `None` when one is wrong.
pub fn parse(args: &[String]) -> Option<Options> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--card" => options.card = Some(PathBuf::from(args.next()?)),
            "--height" => options.height = Some(args.next()?.parse().ok()?),
            "--mirror" => options.mirror = Some(PathBuf::from(args.next()?)),
            "--layout" => options.layout = Some(args.next()?.clone()),
            "--psf" => options.psf = true,
            _ => return None,
        }
    }
    Some(options)
}

/// The timeout of `zbm console release` in `args`, or `None` when an
/// option is wrong.
pub fn parse_release(args: &[String]) -> Option<Duration> {
    match args {
        [] => Some(RELEASE_TIMEOUT),
        [option, millis] if option == "--timeout" => millis.parse().ok().map(Duration::from_millis),
        _ => None,
    }
}

/// The image's TrueType font, in its fonts directory.
pub const TTF: &str = "DejaVuSansMono.ttf";

/// The TrueType font's other faces, in its fonts directory, when the image
/// has them.
pub const STYLED: [(FontFace, &str); 3] = [
    (FontFace::Bold, "DejaVuSansMono-Bold.ttf"),
    (FontFace::Italic, "DejaVuSansMono-Oblique.ttf"),
    (FontFace::BoldItalic, "DejaVuSansMono-BoldOblique.ttf"),
];

/// The directory below the fonts directory with the fallback fonts.
pub const FALLBACK: &str = "fallback";

/// The regular TrueType font `regular`, sized to cells `height` pixels
/// high, with the faces and fallback fonts of the fonts directory `fonts`
/// that can be read.
fn faces(files: &dyn Files, fonts: &Path, regular: Outline, height: u32) -> Faces {
    let load = |path: &Path| {
        files
            .read(path)
            .ok()
            .and_then(|data| Outline::new(data, height).ok())
    };
    let mut faces = Faces::new(regular);
    for (face, name) in STYLED {
        if let Some(outline) = load(&fonts.join(name)) {
            faces = faces.with_face(face, outline);
        }
    }
    let directory = fonts.join(FALLBACK);
    let mut names = files.read_dir(&directory).unwrap_or_default();
    names.sort();
    for name in names {
        if let Some(outline) = load(&directory.join(name)) {
            faces = faces.with_fallback(outline);
        }
    }
    faces
}

/// The grid the console draws with on a display of `size`, from the
/// image's fonts below `root`: the TrueType font in cells as high as the
/// console font `zbm console-autosize` would choose, or that console font
/// itself, with `--psf` or when the image has no TrueType font.
///
/// # Errors
///
/// When the image has no console font.
pub fn console_grid(
    files: &dyn Files,
    root: &Path,
    options: &Options,
    size: (u32, u32),
) -> io::Result<Grid> {
    let bitmap = crate::menu_demo::grid(files, root, options.height, size)?;
    if options.psf {
        return Ok(bitmap);
    }
    let fonts = root.join(FONTS);
    let Ok(data) = files.read(&fonts.join(TTF)) else {
        return Ok(bitmap);
    };
    let height = bitmap.cell().1;
    Ok(match Outline::new(data, height) {
        Ok(outline) => Grid::faces(faces(files, &fonts, outline, height)).named(TTF),
        Err(_) => bitmap,
    })
}

/// `zbm console`, or `zbm console release`, `reattach` or `layout`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    match args.first().map(String::as_str) {
        Some("release") => return run_release(args.get(1..).unwrap_or(&[]), io),
        Some("reattach") => return run_reattach(args.get(1..).unwrap_or(&[]), io),
        Some("layout") => return crate::layouts::run_switch(args.get(1..).unwrap_or(&[]), io),
        _ => {}
    }
    let Some(options) = parse(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    let files = RealFiles;
    let settings = Settings {
        card: options.card.clone(),
        mirror: options.mirror.clone(),
        flavour: zbm_console::keys::Flavour::of_image(|path| files.exists(path)),
    };
    let places = Places::system();
    let mut grid = |size| console_grid(&files, Path::new("/"), &options, size);
    let directory = Path::new(crate::layouts::DIRECTORY);
    let mut warn = |text: String| places.log(4, &text);
    let first = crate::layouts::first(&files, directory, options.layout.as_deref(), &mut warn);
    let mut load = |name: &str| crate::layouts::load(&files, directory, name);
    let layouts = Layouts {
        first,
        load: &mut load,
    };
    match serve(&settings, &places, &mut grid, layouts) {
        Ok(()) => 0,
        Err(error) => {
            places.log(3, &error.to_string());
            let _ = writeln!(io.stderr, "zbm console: {error}");
            1
        }
    }
}

/// `zbm console release`: asks the console to hand the display back, and
/// waits until it did. Nothing to do without a console.
fn run_release(args: &[String], io: &mut Io) -> i32 {
    let Some(timeout) = parse_release(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    match release(&Places::system().base, timeout) {
        Ok(Released::Done) => 0,
        Ok(Released::Late) => {
            let _ = writeln!(io.stderr, "zbm console: the display was not handed back");
            1
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm console: {error}");
            1
        }
    }
}

/// `zbm console reattach`: asks the console that handed the display back
/// to take it again, and waits until it did. Nothing to do without one.
fn run_reattach(args: &[String], io: &mut Io) -> i32 {
    let Some(timeout) = parse_release(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    match reattach(&Places::system().base, timeout) {
        Ok(true) => 0,
        Ok(false) => {
            let _ = writeln!(io.stderr, "zbm console: the display was not taken again");
            1
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm console: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn options_are_parsed_and_wrong_ones_refused() {
        assert_eq!(
            parse(&args(
                "--card /dev/dri/card1 --height 32 --mirror /dev/ttyS1"
            )),
            Some(Options {
                card: Some(PathBuf::from("/dev/dri/card1")),
                height: Some(32),
                psf: false,
                layout: None,
                mirror: Some(PathBuf::from("/dev/ttyS1")),
            })
        );
        assert_eq!(parse(&[]), Some(Options::default()));
        assert!(parse(&args("--psf")).unwrap().psf);
        let layout = parse(&args("--layout de(nodeadkeys)")).unwrap().layout;
        assert_eq!(layout.as_deref(), Some("de(nodeadkeys)"));
        assert_eq!(parse(&args("--layout")), None);
        assert_eq!(parse(&args("--height tall")), None);
        assert_eq!(parse(&args("--mirror")), None);
        assert_eq!(parse(&args("--loud")), None);

        assert_eq!(parse_release(&[]), Some(RELEASE_TIMEOUT));
        assert_eq!(
            parse_release(&args("--timeout 500")),
            Some(Duration::from_millis(500))
        );
        assert_eq!(parse_release(&args("--timeout soon")), None);
        assert_eq!(parse_release(&args("--now")), None);
    }

    #[test]
    fn wrong_options_show_the_usage() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };

        assert_eq!(run(&args("--loud"), &mut io), USAGE);
        // No console runs on the machine of the tests: nothing to hand back.
        assert_eq!(run(&args("release --timeout 10"), &mut io), 0);
        assert_eq!(run(&args("release --now"), &mut io), USAGE);
        assert_eq!(run(&args("reattach --timeout 10"), &mut io), 0);
        assert_eq!(run(&args("reattach --now"), &mut io), USAGE);
        let text = String::from_utf8_lossy(&stderr).into_owned();
        assert_eq!(text.matches("Usage: zbm console").count(), 3, "{text}");
    }

    #[test]
    fn the_console_draws_with_the_truetype_font_at_the_console_font_s_height() {
        let files = zbm_testkit::fakes::FakeFiles::new();
        let fonts = Path::new("/").join(FONTS);
        let terminus = include_bytes!("../../../legacy/zfsbootmenu/fonts/ter-v20b.psf");
        files.file(fonts.join("ter-v20b.psf"), terminus.as_slice());
        let options = Options::default();
        let psf = Options {
            psf: true,
            ..Options::default()
        };
        let grid = |options: &Options| console_grid(&files, Path::new("/"), options, (1280, 800));

        // Without a TrueType font, the console font.
        let bitmap = grid(&options).unwrap();
        assert_eq!((bitmap.name(), bitmap.cell()), ("ter-v20b.psf", (10, 20)));

        let dejavu = include_bytes!("../../../legacy/zfsbootmenu/fonts/dejavu/DejaVuSansMono.ttf");
        files.file(fonts.join(TTF), dejavu.as_slice());
        let outline = grid(&options).unwrap();
        assert_eq!(outline.name(), TTF);
        assert_eq!(outline.cell().1, 20);
        assert_eq!(grid(&psf).unwrap().name(), "ter-v20b.psf");

        assert_eq!(outline.font_count(), 1);

        // Its faces and fallback fonts, those that are fonts.
        files.file(fonts.join(STYLED[0].1), dejavu.as_slice());
        let fallback = fonts.join(FALLBACK);
        files.file(fallback.join("b.ttf"), dejavu.as_slice());
        files.file(fallback.join("a.ttf"), "not a font");
        assert_eq!(grid(&options).unwrap().font_count(), 3);
        assert_eq!(grid(&psf).unwrap().font_count(), 1);

        // A file that is no font leaves the console font.
        files.file(fonts.join(TTF), "not a font");
        assert_eq!(grid(&options).unwrap().name(), "ter-v20b.psf");
    }
}
