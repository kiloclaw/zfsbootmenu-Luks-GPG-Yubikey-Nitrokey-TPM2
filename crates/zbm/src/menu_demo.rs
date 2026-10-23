//! `zbm debug menu-demo`: the menu prototype on the display without a
//! terminal (ROADMAP.md #19, step 1), in images built with the `menu-demo`
//! feature, for the VM test `runtime-rust-drm`.
//!
//! It runs the demo of `zbm-tui` on its fake machine, drawn through DRM with
//! the image's console font at the size `zbm console-autosize` would choose
//! for the display, and keys from the keyboards; it gives the display and
//! the virtual terminal back when the menu ends.
//!
//! ```text
//! zbm debug menu-demo [--card <path>] [--height <pixels>]
//!                     [--dump <file>] [--wait]
//! ```
//!
//! `--dump` writes the screen's text to a file after each frame, which the
//! VM test reads over the serial console; `--wait` stops the countdown.

use std::path::{Path, PathBuf};

use zbm_display::grid::Grid;
use zbm_platform::{Files, RealFiles};
use zbm_startup::autosize::choice::{self, Candidate, Request};
use zbm_startup::autosize::font::Cell;
use zbm_startup::autosize::{FONTS, density, fonts};
use zbm_tui::menu::caps::Capabilities;
use zbm_tui::menu::demo::{DemoOptions, Session};
use zbm_tui::menu::display::run_on_display;
use zbm_tui::menu::theme::Theme;
use zbm_types::psf::Font;

use crate::{Io, USAGE};

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm debug menu-demo [--card <path>] [--height <pixels>] [--dump <file>] [--wait]
";

/// What the command line asks for.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// The DRM card; the first otherwise.
    pub card: Option<PathBuf>,
    /// The font height asked for, as `zbm.autosize=<pixels>`.
    pub height: Option<u16>,
    /// Where to write the screen's text.
    pub dump: Option<PathBuf>,
    /// Whether the countdown waits for a key.
    pub wait: bool,
}

/// The options in `args`, or `None` when one is wrong.
pub fn parse(args: &[String]) -> Option<Options> {
    let mut options = Options::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--card" => options.card = Some(PathBuf::from(args.next()?)),
            "--dump" => options.dump = Some(PathBuf::from(args.next()?)),
            "--height" => options.height = Some(args.next()?.parse().ok()?),
            "--wait" => options.wait = true,
            _ => return None,
        }
    }
    Some(options)
}

/// The font for a display `width` pixels wide, from `fonts` (each with its
/// cell), as `zbm console-autosize` chooses: the best candidate for the
/// request and the density that leaves enough columns, else the smallest.
pub fn choose_font(
    fonts: &[(PathBuf, Cell)],
    request: Request,
    density: Option<choice::Density>,
    width: u32,
) -> Option<Candidate> {
    let columns = |candidate: &Candidate| {
        let cell = u32::from(candidate.cell.width.max(1));
        u16::try_from(width / cell).ok()
    };
    choice::order(fonts, request, density)
        .into_iter()
        .find(|candidate| request.fits(columns(candidate)))
        .or_else(|| choice::smallest(fonts))
}

/// The grid of the font chosen for a display of `size`, from the image's
/// fonts below `root`; `zbm console` uses it too.
pub(crate) fn grid(
    files: &dyn Files,
    root: &Path,
    height: Option<u16>,
    size: (u32, u32),
) -> std::io::Result<Grid> {
    let fonts = fonts(files, &root.join(FONTS));
    let request = height.map_or(Request::Auto, Request::Height);
    let chosen = choose_font(&fonts, request, density(files, root), size.0)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no console font"))?;
    let font = Font::parse(&files.read(&chosen.path)?)
        .map_err(|error| std::io::Error::other(format!("{}: {error}", chosen.path.display())))?;
    Ok(Grid::new(font, if chosen.doubled { 2 } else { 1 }))
}

/// `zbm debug menu-demo`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let Some(options) = parse(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    let demo = DemoOptions {
        timeout: if options.wait {
            None
        } else {
            DemoOptions::default().timeout
        },
        ..DemoOptions::default()
    };
    let session = Session::new(&demo, Theme::new(Capabilities::CONSOLE));
    let files = RealFiles;
    let mut chosen = String::new();
    let mut make_grid = |size: (u32, u32)| {
        let grid = grid(&files, Path::new("/"), options.height, size)?;
        let (columns, rows) = grid.dimensions(size);
        let (width, height) = grid.cell();
        chosen = format!(
            "{}x{} display, {width}x{height} cells, {columns}x{rows}",
            size.0, size.1
        );
        Ok(grid)
    };
    let mut shown = |text: &str| {
        if let Some(dump) = &options.dump {
            let _ = files.write(dump, text.as_bytes());
        }
    };
    let card = options.card.as_deref();
    match run_on_display(session, card, &mut make_grid, &mut shown) {
        Ok(why) => {
            let _ = writeln!(io.stdout, "menu-demo: {chosen}: {why}");
            0
        }
        Err(error) => {
            let _ = writeln!(io.stderr, "menu-demo: {error}");
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
        let options = parse(&args(
            "--card /dev/dri/card1 --height 64 --dump /tmp/s --wait",
        ))
        .unwrap();

        assert_eq!(
            options,
            Options {
                card: Some(PathBuf::from("/dev/dri/card1")),
                height: Some(64),
                dump: Some(PathBuf::from("/tmp/s")),
                wait: true,
            }
        );
        assert_eq!(parse(&[]), Some(Options::default()));
        assert_eq!(parse(&args("--height big")), None);
        assert_eq!(parse(&args("--card")), None);
        assert_eq!(parse(&args("--loud")), None);
    }

    fn terminus() -> Vec<(PathBuf, Cell)> {
        [(6, 12), (8, 14), (10, 20), (12, 24), (14, 28), (16, 32)]
            .into_iter()
            .map(|(width, height)| {
                (
                    PathBuf::from(format!("ter-{height}.psf")),
                    Cell { width, height },
                )
            })
            .collect()
    }

    #[test]
    fn the_font_is_chosen_as_console_autosize_chooses() {
        let fonts = terminus();
        let height = |c: Option<Candidate>| c.map(|c| (c.cell.height, c.doubled));

        // Without a density: the largest that leaves 110 columns.
        assert_eq!(
            height(choose_font(&fonts, Request::Auto, None, 1280)),
            Some((20, false))
        );
        assert_eq!(
            height(choose_font(&fonts, Request::Auto, None, 3840)),
            Some((32, false))
        );
        // Too narrow for 110 columns of any: the smallest.
        assert_eq!(
            height(choose_font(&fonts, Request::Auto, None, 300)),
            Some((12, false))
        );
        // A height asked for, doubled if need be, whatever the columns.
        assert_eq!(
            height(choose_font(&fonts, Request::Height(48), None, 800)),
            Some((48, true))
        );
        assert_eq!(choose_font(&[], Request::Auto, None, 800), None);
    }

    #[test]
    fn the_grid_uses_the_image_s_font_file() {
        let files = zbm_testkit::fakes::FakeFiles::new();
        let terminus = include_bytes!("../../../legacy/zfsbootmenu/fonts/ter-v20b.psf");
        files.file(
            Path::new("/").join(FONTS).join("ter-v20b.psf"),
            terminus.as_slice(),
        );

        let grid = grid(&files, Path::new("/"), None, (1280, 800)).unwrap();
        let doubled = grid_cell(&files, Some(40));

        assert_eq!(grid.cell(), (10, 20));
        assert_eq!(doubled, Some((20, 40)));
        assert!(super::grid(&files, Path::new("/none"), None, (1280, 800)).is_err());
    }

    fn grid_cell(files: &dyn Files, height: Option<u16>) -> Option<(u32, u32)> {
        grid(files, Path::new("/"), height, (1280, 800))
            .ok()
            .map(|grid| grid.cell())
    }

    #[test]
    fn wrong_options_show_the_usage() {
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };

        assert_eq!(run(&args("--loud"), &mut io), USAGE);
        assert!(String::from_utf8_lossy(&stderr).starts_with("Usage: zbm debug menu-demo"));
    }
}
