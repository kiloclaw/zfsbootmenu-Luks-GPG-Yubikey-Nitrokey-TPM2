//! The early-setup hook `30-console-autosize` of the Rust runtime: sizes the
//! console font to the display, for high-density (HiDPI) and ordinary
//! screens alike (`zbm.autosize` in `zfsbootmenu(7)`).
//!
//! The legacy hook takes the largest of the image's Terminus fonts (12 to 32
//! pixels) that leaves 110 columns, whatever the size of the screen. This one
//! reads the display's physical size from its EDID, when a DRM driver offers
//! it, and takes the font closest to [`choice::TEXT_HEIGHT_MM`] tall, so that
//! a 4K laptop and a 4K monitor both get readable text; fonts doubled by
//! `setfont -d` reach 64 pixels for the densest panels. Without an EDID it
//! does what the legacy hook does. `zbm.autosize=<pixels>` asks for a height.

pub mod choice;
pub mod edid;
pub mod font;

use std::path::{Path, PathBuf};

use zbm_config::args::{Args, ZbmArgs};
use zbm_config::conf::Conf;
use zbm_config::spec::cmdline::{AUTOSIZE, VCONSOLE_FONT};
use zbm_platform::System;
use zbm_platform::error::Result;
use zbm_platform::files::Files;
use zbm_tools::terminal;

use self::choice::{Candidate, Density, Request};
use self::edid::Edid;
use self::font::Cell;
use crate::preinit::{self, is_virtual_console};

/// Where the image keeps the fonts, below the root.
pub const FONTS: &str = "usr/share/zfsbootmenu/fonts";

/// The DRM devices and connectors, below the root.
const DRM: &str = "sys/class/drm";

/// The size of the first framebuffer, below the root: `width,height`.
const FRAMEBUFFER: &str = "sys/class/graphics/fb0/virtual_size";

/// The fonts mkinitcpio's `consolefont` hook loads, below the root.
const CONSOLEFONTS: [&str; 2] = ["consolefont.psfu", "consolefont.psf"];

/// What the hook is asked to do.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// The control terminal (`console=`).
    pub console: String,
    /// `zbm.autosize`.
    pub request: Request,
    /// Whether `rd.vconsole.font` names a font, which dracut loaded.
    pub font_given: bool,
}

impl Settings {
    /// The settings of the image below `root`: the control terminal of
    /// `conf`, and the options of the command line that start-up saved.
    pub fn from_image(system: &System<'_>, root: &Path, conf: &Conf) -> Settings {
        let options = preinit::saved_options(system, root, conf);
        let kcl = preinit::saved_commandline(system, root, conf);
        let args = ZbmArgs::from_cmdline(&kcl);
        let mut messages = Vec::new();
        let request = Request::parse(args.lookup(&AUTOSIZE, &mut messages).as_deref());
        let font = args.lookup(&VCONSOLE_FONT, &mut messages);
        Settings {
            console: options.control_term,
            request,
            font_given: font.is_some_and(|font| !font.is_empty()),
        }
    }
}

/// What the hook did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// It left the font alone, for this reason.
    Skipped(&'static str),
    /// It loaded this font, and the console is this size (columns, rows).
    Set {
        /// The font.
        font: Candidate,
        /// The size of the console with it, when the kernel could tell.
        size: Option<(u16, u16)>,
    },
    /// No font could be loaded.
    Failed,
}

/// The hook `30-console-autosize` (`zbm console-autosize`): reads the
/// configuration of the image below `root`, logs to the kernel log as the
/// image is configured, and sizes the font of `console`, or of the control
/// terminal.
///
/// # Errors
///
/// When the configuration of the image cannot be read.
pub fn hook(system: &System<'_>, root: &Path, console: Option<String>) -> Result<Outcome> {
    let conf = preinit::read_conf(system, root)?;
    let base = preinit::base(&conf);
    let _log = system.kernel_log(conf.get("loglevel").unwrap_or_default(), &base);
    let mut settings = Settings::from_image(system, root, &conf);
    if let Some(console) = console {
        settings.console = console;
    }
    Ok(run(system, root, &settings))
}

/// Sizes the font of the console of `settings` on the system below `root`.
pub fn run(system: &System<'_>, root: &Path, settings: &Settings) -> Outcome {
    if let Some(reason) = reason_to_skip(system.files, root, settings) {
        zbm_trace::debug!("console autosize skipped: {reason}"; %reason);
        return Outcome::Skipped(reason);
    }
    let fonts = fonts(system.files, &root.join(FONTS));
    if fonts.is_empty() {
        return Outcome::Skipped("no fonts in the image");
    }
    let screen = framebuffer(system.files, root);
    let density = density(system.files, root);
    if let Some(density) = density {
        zbm_trace::debug!("display density {dpi} dpi"; dpi = %density.dpi());
    } else {
        zbm_trace::debug!("display density unknown");
    }
    let console = settings.console.as_str();
    for candidate in choice::order(&fonts, settings.request, density) {
        if !terminal::setfont(system.runner, console, &candidate.path, candidate.doubled)
            .is_ok_and(|output| output.status.is_success())
        {
            continue;
        }
        let size = terminal_size(system, console);
        let columns = size
            .map(|(columns, _)| columns)
            .or_else(|| screen.map(|(width, _)| width / candidate.cell.width));
        if settings.request.fits(columns) {
            return set(console, candidate, size);
        }
    }
    let Some(smallest) = choice::smallest(&fonts) else {
        return Outcome::Failed;
    };
    if terminal::setfont(system.runner, console, &smallest.path, false)
        .is_ok_and(|output| output.status.is_success())
    {
        let size = terminal_size(system, console);
        return set(console, smallest, size);
    }
    zbm_trace::warn!("unable to load a console font on {console}"; %console);
    Outcome::Failed
}

/// The columns and rows of the terminal `console`, or `None` when it
/// cannot tell.
fn terminal_size(system: &System<'_>, console: &str) -> Option<(u16, u16)> {
    let size = system.files.terminal_size(Path::new(console)).ok()?;
    Some((size.columns, size.rows))
}

/// Logs the font that was kept.
fn set(console: &str, font: Candidate, size: Option<(u16, u16)>) -> Outcome {
    let doubled = if font.doubled { ", doubled" } else { "" };
    let shown = size.map_or_else(
        || "of unknown size".to_owned(),
        |(columns, rows)| format!("{columns}x{rows}"),
    );
    zbm_trace::info!(
        "font set to {font}{doubled}, {console} is {shown}";
        font = %font.path.display(), %doubled, %console, %shown
    );
    Outcome::Set { font, size }
}

/// Why the font stays as it is: off, a serial console, or a font the user
/// chose.
fn reason_to_skip(files: &dyn Files, root: &Path, settings: &Settings) -> Option<&'static str> {
    if settings.request == Request::Off {
        return Some("zbm.autosize is off");
    }
    if !is_virtual_console(&settings.console) {
        return Some("the control terminal is not a virtual console");
    }
    if CONSOLEFONTS
        .iter()
        .any(|name| files.exists(&root.join(name)))
    {
        return Some("mkinitcpio's consolefont hook loaded a font");
    }
    if settings.font_given {
        return Some("rd.vconsole.font names a font");
    }
    None
}

/// The PSF fonts in `dir`, by name, with their cells.
pub fn fonts(files: &dyn Files, dir: &Path) -> Vec<(PathBuf, Cell)> {
    let mut names = files.read_dir(dir).unwrap_or_default();
    names.sort();
    names
        .into_iter()
        .map(|name| dir.join(name))
        .filter(|path| path.extension().is_some_and(|ext| ext == "psf"))
        .filter_map(|path| {
            let header = files.read_prefix(&path, font::HEADER).ok()?;
            Some((path.clone(), font::cell(&header)?))
        })
        .collect()
}

/// The pixel density of the display: the framebuffer's size on the display
/// of the first connected DRM connector with a usable EDID.
pub fn density(files: &dyn Files, root: &Path) -> Option<Density> {
    let edid = connected_edid(files, &root.join(DRM))?;
    let screen = framebuffer(files, root).or(edid.mode)?;
    Density::new(screen, &edid)
}

/// The EDID of the first connected connector, by name, that has one.
fn connected_edid(files: &dyn Files, drm: &Path) -> Option<Edid> {
    let mut connectors = files.read_dir(drm).ok()?;
    connectors.sort();
    connectors.into_iter().find_map(|name| {
        let connector = drm.join(&name);
        let status = files.read_to_string(&connector.join("status")).ok()?;
        if status.trim() != "connected" {
            return None;
        }
        edid::parse(&files.read(&connector.join("edid")).ok()?)
    })
}

/// The size of the first framebuffer, in pixels.
fn framebuffer(files: &dyn Files, root: &Path) -> Option<(u16, u16)> {
    let text = files.read_to_string(&root.join(FRAMEBUFFER)).ok()?;
    let (width, height) = text.trim().split_once(',')?;
    let size = (width.parse().ok()?, height.parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

#[cfg(test)]
mod tests;
