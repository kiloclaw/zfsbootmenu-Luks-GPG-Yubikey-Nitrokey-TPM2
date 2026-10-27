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
//!             [--numlock on|off] [--repeat <delay>,<rate>]
//!             [--scrollback <lines>] [--palette <name>] [--large]
//!             [--status] [--bell none|visual|audible|both]
//!             [--compose ralt|rwin|menu|rctrl|caps]
//!             [--keyboard <vendor>:<product>=<layout>]... [--mirror <file>]
//! zbm console release [--timeout <milliseconds>]
//! zbm console reattach [--timeout <milliseconds>]
//! zbm console layout [<name>]
//! zbm console font [<pixels>|+|-|default]
//! ```
//!
//! `zbm console reattach`, when `kexec` failed after the hand-over, has the
//! console take the display again, so that the error and the menu after it
//! are shown.
//!
//! Keys are translated with the image's first keyboard layout, or the one
//! `--layout` names (ROADMAP.md #19, step 4); `zbm console layout` names
//! the layout and the others of the image, or switches to another, as
//! Super+Space does. `--numlock` turns Num Lock on or off at the start, and
//! `--repeat` sets the keyboards' repeat: a delay in milliseconds and a
//! rate a second. `--scrollback` sets the lines of history kept for
//! Shift+PgUp (2000; 0 keeps none), never while a secret prompt is open.
//!
//! The console draws with the image's TrueType font, in cells as high as
//! the console font `zbm console-autosize` would choose, with its bold,
//! oblique and bold oblique faces where the image has them, and the
//! fallback fonts of the image (`fonts/fallback/`, in name order) for the
//! characters it lacks; `--psf` draws with that console font itself.
//! Ctrl+Alt+`=` and Ctrl+Alt+`-` make the cells larger and smaller, and
//! Ctrl+Alt+`0` gives them their first size again; `zbm console font`
//! says which font the console draws with, or asks for a size: a height
//! in pixels, the next one up (`+`) or down (`-`), or the first
//! (`default`).
//!
//! `--palette` names the colour scheme: `linux`, the Linux console's, by
//! default, `high-contrast`, `solarized-dark`, `solarized-light` or
//! `tango`. `--large` draws cells twice as high, or half again as high
//! when that would leave fewer than 40 columns, in the high-contrast
//! scheme unless `--palette` names another.
//!
//! `--status` adds a status line under the programs' screen: the title
//! they set, the keyboard layout, the locks that are on, the time (UTC)
//! and the battery's charge. `--bell` says what the bell does: the
//! frame's border flashes (`visual`, the default), the PC speaker sounds
//! (`audible`), both, or nothing (`none`). `--compose` makes a key the
//! compose key: it starts the sequences of the image's compose rules, as
//! Compose a e for æ. `--keyboard` gives the keyboard of a USB vendor and
//! product (`046d:c31c`) a layout of its own, with its own key state and
//! lights; the others type with the console's layout.
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

use zbm_console::scheme::{HIGH_CONTRAST, Scheme};
use zbm_console::screen::Ring;
use zbm_console::screen::zoom::Zoom;
use zbm_console::serve::{
    Layouts, Places, Released, Settings, Switched, reattach, release, request_font, serve,
};
use zbm_display::input::DeviceId;
use zbm_display::keys::compose_key;
use zbm_platform::RealFiles;

use crate::{Io, USAGE};

/// The usage text.
const USAGE_TEXT: &str = "\
Usage: zbm console [--card <path>] [--height <pixels>] [--psf] [--layout <name>]
                   [--numlock on|off] [--repeat <delay>,<rate>] [--scrollback <lines>]
                   [--palette <name>] [--large] [--status]
                   [--bell none|visual|audible|both] [--compose ralt|rwin|menu|rctrl|caps]
                   [--keyboard <vendor>:<product>=<layout>]... [--mirror <file>]
       zbm console release [--timeout <milliseconds>]
       zbm console reattach [--timeout <milliseconds>]
       zbm console layout [<name>]
       zbm console font [<pixels>|+|-|default]
";

/// The lines of scrollback the console keeps by default: about 9 MB on a
/// display of 192 columns.
pub const SCROLLBACK: usize = 2000;

/// The most lines of scrollback that may be asked for.
pub const MAX_SCROLLBACK: usize = 100_000;

/// How long `zbm console release` waits by default.
const RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

/// What the command line asks for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Options {
    /// The DRM card; the first otherwise.
    pub card: Option<PathBuf>,
    /// The font height asked for, as `zbm.autosize=<pixels>`.
    pub height: Option<u16>,
    /// Whether to draw with the console fonts instead of the TrueType font.
    pub psf: bool,
    /// The keyboard layout asked for; the image's first otherwise.
    pub layout: Option<String>,
    /// Num Lock at the start; as the keyboards show it otherwise.
    pub num_lock: Option<bool>,
    /// The key repeat: the delay and the period, in milliseconds.
    pub repeat: Option<(u32, u32)>,
    /// The lines of scrollback; [`SCROLLBACK`] otherwise.
    pub scrollback: Option<usize>,
    /// Where to write the screen's text.
    pub mirror: Option<PathBuf>,
    /// The colour scheme asked for.
    pub scheme: Option<&'static Scheme>,
    /// Whether to draw large text, in the high-contrast scheme unless
    /// another is asked for.
    pub large: bool,
    /// Whether to show a status line.
    pub status: bool,
    /// What the bell does.
    pub bell: Ring,
    /// The compose key, by code.
    pub compose_key: Option<u16>,
    /// The keyboards with a layout of their own, by USB id.
    pub device_layouts: Vec<(DeviceId, String)>,
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
            "--numlock" => {
                options.num_lock = match args.next()?.as_str() {
                    "on" => Some(true),
                    "off" => Some(false),
                    _ => return None,
                };
            }
            "--repeat" => options.repeat = Some(zbm_startup::console::repeat(args.next()?)?),
            "--scrollback" => {
                options.scrollback =
                    Some(args.next()?.parse().ok().filter(|&n| n <= MAX_SCROLLBACK)?);
            }
            "--psf" => options.psf = true,
            "--palette" => options.scheme = Some(zbm_console::scheme::by_name(args.next()?)?),
            "--large" => options.large = true,
            "--status" => options.status = true,
            "--bell" => options.bell = Ring::parse(args.next()?)?,
            "--compose" => options.compose_key = Some(compose_key(args.next()?)?),
            "--keyboard" => options.device_layouts.push(device_layout(args.next()?)?),
            _ => return None,
        }
    }
    Some(options)
}

/// The keyboard and its layout that `rule` (`046d:c31c=de`) names.
fn device_layout(rule: &str) -> Option<(DeviceId, String)> {
    let (id, name) = rule.split_once('=')?;
    zbm_display::layout::compile::split(name).ok()?;
    Some((DeviceId::parse(id)?, name.to_owned()))
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
/// itself, with `--psf` or when the image has no TrueType font. A `height`
/// asked for while the console runs comes before `--height`: the console
/// font nearest it, or the TrueType font in cells that high.
///
/// # Errors
///
/// When the image has no console font.
pub fn console_grid(
    files: &dyn Files,
    root: &Path,
    options: &Options,
    size: (u32, u32),
    height: Option<u16>,
) -> io::Result<Grid> {
    if options.large && height.is_none() {
        return large_grid(files, root, options, size);
    }
    let asked = height.or(options.height);
    let bitmap = crate::menu_demo::grid(files, root, asked, size)?;
    if options.psf {
        return Ok(bitmap);
    }
    let fonts = root.join(FONTS);
    let Ok(data) = files.read(&fonts.join(TTF)) else {
        return Ok(bitmap);
    };
    let height = height.map_or(bitmap.cell().1, u32::from);
    Ok(match Outline::new(data, height) {
        Ok(outline) => Grid::faces(faces(files, &fonts, outline, height)).named(TTF),
        Err(_) => bitmap,
    })
}

/// The fewest columns large text may leave.
const LARGE_COLUMNS: u16 = 40;

/// The grid of large text on a display of `size`: cells twice as high as
/// the console would choose, or half again as high when that leaves fewer
/// than [`LARGE_COLUMNS`], or as high when that does too.
fn large_grid(
    files: &dyn Files,
    root: &Path,
    options: &Options,
    size: (u32, u32),
) -> io::Result<Grid> {
    let normal = Options {
        large: false,
        ..options.clone()
    };
    let base = console_grid(files, root, &normal, size, None)?;
    let height = base.cell().1;
    for larger in [height * 2, height * 3 / 2] {
        let Ok(larger) = u16::try_from(larger) else {
            continue;
        };
        let grid = console_grid(files, root, &normal, size, Some(larger))?;
        if grid.dimensions(size).0 >= LARGE_COLUMNS {
            return Ok(grid);
        }
    }
    Ok(base)
}

/// The console's settings from `options`, with the layouts of the
/// keyboards that have their own from `directory`; one that cannot be
/// loaded is said in `places`' log.
fn settings(options: &Options, files: &dyn Files, places: &Places, directory: &Path) -> Settings {
    Settings {
        card: options.card.clone(),
        mirror: options.mirror.clone(),
        flavour: zbm_console::keys::Flavour::of_image(|path| files.exists(path)),
        num_lock: options.num_lock,
        repeat: options.repeat,
        scrollback: options.scrollback.unwrap_or(SCROLLBACK),
        scheme: options
            .scheme
            .or_else(|| options.large.then_some(&HIGH_CONTRAST)),
        status_line: options.status,
        bell: options.bell,
        compose_key: options.compose_key,
        device_layouts: options
            .device_layouts
            .iter()
            .filter_map(|(id, name)| {
                crate::layouts::source(files, directory, name)
                    .map(|source| (*id, source))
                    .map_err(|error| places.log(4, &format!("keyboard {id}: {error}")))
                    .ok()
            })
            .collect(),
    }
}

/// `zbm console`, or `zbm console release`, `reattach` or `layout`.
pub fn run(args: &[String], io: &mut Io) -> i32 {
    match args.first().map(String::as_str) {
        Some("release") => return run_release(args.get(1..).unwrap_or(&[]), io),
        Some("reattach") => return run_reattach(args.get(1..).unwrap_or(&[]), io),
        Some("layout") => return crate::layouts::run_switch(args.get(1..).unwrap_or(&[]), io),
        Some("font") => return run_font(args.get(1..).unwrap_or(&[]), io),
        _ => {}
    }
    let Some(options) = parse(args) else {
        let _ = io.stderr.write_all(USAGE_TEXT.as_bytes());
        return USAGE;
    };
    let files = RealFiles;
    let places = Places::system();
    let directory = Path::new(crate::layouts::DIRECTORY);
    let settings = settings(&options, &files, &places, directory);
    // The screen and its history stay out of swap.
    if let Err(error) = zbm_sys::memory::lock_all_memory() {
        places.log(5, &format!("memory not locked: {error}"));
    }
    let mut grid = |size, height| console_grid(&files, Path::new("/"), &options, size, height);
    let mut warn = |text: String| places.log(4, &text);
    let first = crate::layouts::first(&files, directory, options.layout.as_deref(), &mut warn);
    let mut load = |name: &str| crate::layouts::load(&files, directory, name);
    let layouts = Layouts {
        first,
        names: crate::layouts::available(&files, directory),
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

/// How long `zbm console font` waits for the console.
const FONT_TIMEOUT: Duration = Duration::from_secs(3);

/// `zbm console font`: says which font the console draws with, or asks it
/// for a size.
fn run_font(args: &[String], io: &mut Io) -> i32 {
    let base = Places::system().base;
    let size = match args {
        [] => {
            let state = zbm_state::State::new(&RealFiles, &base);
            let font = state.console_font();
            let font = font.as_deref().unwrap_or("none, no console on the display");
            let _ = writeln!(io.stdout, "font: {font}");
            return 0;
        }
        [size] if Zoom::parse(size).is_some() => size,
        _ => {
            let _ = writeln!(io.stderr, "Usage: zbm console font [<pixels>|+|-|default]");
            return USAGE;
        }
    };
    let failure = match request_font(&base, size, FONT_TIMEOUT) {
        Ok(Switched::Done(font)) => {
            let _ = writeln!(io.stdout, "font: {font}");
            return 0;
        }
        Ok(Switched::NoConsole) => "no console on the display".to_owned(),
        Ok(Switched::Late) => "the console did not answer".to_owned(),
        Err(error) => error.to_string(),
    };
    let _ = writeln!(io.stderr, "zbm console: {failure}");
    1
}

#[cfg(test)]
mod tests;
