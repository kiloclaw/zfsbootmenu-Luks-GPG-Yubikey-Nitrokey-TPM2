//! Which console fonts to try, in which order, and which to keep: no I/O.
//!
//! With the display's pixel density known, the fonts closest to
//! [`TEXT_HEIGHT_MM`] tall come first, doubled fonts included. Without it,
//! the order is the legacy hook's: the largest font up to [`GUESS_HEIGHT`]
//! first, and the first that leaves [`MIN_COLUMNS`] columns is kept.

use std::cmp::Reverse;
use std::path::PathBuf;

use super::edid::Edid;
use super::font::Cell;

/// The height of a line of text the automatic size aims for, in tenths of a
/// millimetre: about the size of 13-point text, readable at arm's length.
pub const TEXT_HEIGHT_MM: u32 = 45;

/// The fewest columns an automatic size leaves, as the legacy hook: the
/// snapshot view of the `fzf` menu needs 110.
pub const MIN_COLUMNS: u16 = 110;

/// The tallest font tried without a density, as the legacy hook, whose
/// largest font is Terminus 16×32: Spleen's 32×64 would leave 120 columns
/// of 64-pixel text on any 4K display, however large.
pub const GUESS_HEIGHT: u16 = 32;

/// The pixel densities taken as real, in dots per inch; an EDID outside them
/// is wrong (a size given as an aspect ratio, a projector).
const PLAUSIBLE_DPI: std::ops::RangeInclusive<u32> = 50..=600;

/// What `zbm.autosize` asks for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    /// `zbm.autosize=0`: leave the font alone.
    Off,
    /// The default: a size from the display.
    Auto,
    /// `zbm.autosize=<pixels>`: the font closest to this height.
    Height(u16),
}

impl Request {
    /// The request of the value of `zbm.autosize`, if given. A number of 2 or
    /// more is a height in pixels; otherwise the value is on or off, as every
    /// boolean option: empty, `0`, `no` and `off` mean off.
    pub fn parse(value: Option<&str>) -> Request {
        let Some(value) = value.map(str::trim) else {
            return Request::Auto;
        };
        match value.parse::<u16>() {
            Ok(pixels) if pixels >= 2 => Request::Height(pixels),
            _ if matches!(value.to_lowercase().as_str(), "" | "0" | "no" | "off") => Request::Off,
            _ => Request::Auto,
        }
    }

    /// Whether a font that leaves `columns` columns (`None`: unknown) fits
    /// the request, and is kept: a height asked for is kept whatever the
    /// columns.
    pub fn fits(self, columns: Option<u16>) -> bool {
        match self {
            Request::Off => false,
            Request::Auto => columns.is_some_and(|columns| columns >= MIN_COLUMNS),
            Request::Height(_) => true,
        }
    }
}

/// How many pixels make how many millimetres, across the screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Density {
    pixels: u32,
    mm: u32,
}

impl Density {
    /// The density of a `screen` of pixels (columns, rows) on the display
    /// that `edid` describes; `None` when the two directions disagree by
    /// more than a quarter, or the density is not a real display's.
    pub fn new(screen: (u16, u16), edid: &Edid) -> Option<Density> {
        let (width, height) = (u32::from(screen.0), u32::from(screen.1));
        let (width_mm, height_mm) = (u32::from(edid.width_mm), u32::from(edid.height_mm));
        if width == 0 || height == 0 || width_mm == 0 || height_mm == 0 {
            return None;
        }
        let across = u64::from(width) * u64::from(height_mm);
        let down = u64::from(height) * u64::from(width_mm);
        if across.abs_diff(down) * 4 > across.max(down) {
            return None;
        }
        let density = Density {
            pixels: width,
            mm: width_mm,
        };
        PLAUSIBLE_DPI.contains(&density.dpi()).then_some(density)
    }

    /// Dots per inch, rounded.
    pub fn dpi(self) -> u32 {
        (self.pixels * 254 + self.mm * 5) / (self.mm * 10)
    }

    /// The pixels that make `tenths` tenths of a millimetre, rounded.
    pub fn pixels(self, tenths: u32) -> u32 {
        (tenths * self.pixels + self.mm * 5) / (self.mm * 10)
    }
}

/// A font to try: a file, whether `setfont` doubles it, and its cell then.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    /// The font file.
    pub path: PathBuf,
    /// Whether `setfont -d` doubles it.
    pub doubled: bool,
    /// The cell on the screen.
    pub cell: Cell,
}

/// The fonts to try for `request`, best first, from `fonts` (each file with
/// its cell); `density` is the display's, when known.
pub fn order(
    fonts: &[(PathBuf, Cell)],
    request: Request,
    density: Option<Density>,
) -> Vec<Candidate> {
    let target = match (request, density) {
        (Request::Off, _) => return Vec::new(),
        (Request::Auto, None) => return guessed(fonts),
        (Request::Auto, Some(density)) => density.pixels(TEXT_HEIGHT_MM),
        (Request::Height(pixels), _) => u32::from(pixels),
    };
    let mut candidates = plain(fonts);
    let heights: Vec<u16> = candidates.iter().map(|c| c.cell.height).collect();
    candidates.extend(
        fonts
            .iter()
            .map(|(path, cell)| Candidate {
                path: path.clone(),
                doubled: true,
                cell: cell.doubled(),
            })
            .filter(|doubled| !heights.contains(&doubled.cell.height)),
    );
    candidates.sort_by_key(|c| {
        let height = u32::from(c.cell.height);
        (height.abs_diff(target), c.doubled, Reverse(c.cell))
    });
    candidates
}

/// The fonts up to [`GUESS_HEIGHT`] tall, largest first; all of them when
/// none is that small.
fn guessed(fonts: &[(PathBuf, Cell)]) -> Vec<Candidate> {
    let mut candidates = largest_first(fonts);
    if candidates.iter().any(|c| c.cell.height <= GUESS_HEIGHT) {
        candidates.retain(|c| c.cell.height <= GUESS_HEIGHT);
    }
    candidates
}

/// The fonts as they are, largest first: the legacy hook's order.
fn largest_first(fonts: &[(PathBuf, Cell)]) -> Vec<Candidate> {
    let mut candidates = plain(fonts);
    candidates.sort_by_key(|c| Reverse(c.cell));
    candidates
}

fn plain(fonts: &[(PathBuf, Cell)]) -> Vec<Candidate> {
    fonts
        .iter()
        .map(|(path, cell)| Candidate {
            path: path.clone(),
            doubled: false,
            cell: *cell,
        })
        .collect()
}

/// The font to fall back to when none leaves enough columns: the smallest.
pub fn smallest(fonts: &[(PathBuf, Cell)]) -> Option<Candidate> {
    largest_first(fonts).pop()
}
