//! `zbm console-font [-C <console>] [-d] <font>`: loads a PSF font on a
//! virtual terminal, doubled with `-d`, as `setfont` does, which images
//! with the Rust runtime no longer carry (ROADMAP.md CD5). The console is
//! `/dev/tty0`, the one shown, unless `-C` names another.

use std::path::{Path, PathBuf};

use zbm_display::psf::Font;
use zbm_platform::{ConsoleFonts, Files, RealFiles, SystemFonts};

use crate::Io;

/// What the command line asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Request {
    console: PathBuf,
    doubled: bool,
    font: PathBuf,
}

/// The request of `args`; `None` when they are not one.
fn parse(args: &[String]) -> Option<Request> {
    let mut console = PathBuf::from("/dev/tty0");
    let mut doubled = false;
    let mut font = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-C" => console = PathBuf::from(args.next()?),
            "-d" => doubled = true,
            other if other.starts_with('-') || font.is_some() => return None,
            other => font = Some(PathBuf::from(other)),
        }
    }
    Some(Request {
        console,
        doubled,
        font: font?,
    })
}

/// Loads the font `request` names with `files` and `fonts`.
///
/// # Errors
///
/// Why it was not loaded, for the user.
fn load(files: &dyn Files, fonts: &dyn ConsoleFonts, request: &Request) -> Result<(), String> {
    let shown = |path: &Path| path.display().to_string();
    let bytes = files
        .read(&request.font)
        .map_err(|error| format!("{}: {error}", shown(&request.font)))?;
    let font = Font::parse(&bytes).map_err(|error| format!("{}: {error}", shown(&request.font)))?;
    fonts
        .load(&request.console, &font.kernel_font(request.doubled))
        .map_err(|error| format!("{}: {error}", shown(&request.console)))
}

/// `zbm console-font`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    let Some(request) = parse(args) else {
        let _ = writeln!(
            io.stderr,
            "Usage: zbm console-font [-C <console>] [-d] <font>"
        );
        return crate::USAGE;
    };
    match load(&RealFiles, &SystemFonts, &request) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(io.stderr, "zbm console-font: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests;
