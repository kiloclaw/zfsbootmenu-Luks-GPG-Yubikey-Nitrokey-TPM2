//! How the console on the display is set up: its terminal, kept from
//! before a hand-over or new, with what the settings ask for, and the bell
//! of the host.

use std::fs::File;
use std::io;

use zbm_display::speaker::Speaker;

use super::{MakeGrid, Places, Settings};
use crate::host::RealHost;
use crate::screen::{Console, Ring};

/// The console's terminal for a display of `size`: the one `kept` from
/// before the hand-over, whose screen the relay kept up, drawn whole on the
/// new buffer; else a new one, in the font `grid` gives.
pub(super) fn console_for(
    kept: &mut Option<Console>,
    grid: MakeGrid<'_>,
    size: (u32, u32),
) -> io::Result<Console> {
    match kept.take() {
        Some(mut console) if console.pixels() == size => {
            console.frame_mut().damage_all();
            Ok(console)
        }
        _ => Ok(Console::new(grid(size, None)?, size)),
    }
}

/// Gives `console` what `settings` ask for: the keys' flavour, the
/// scrollback, the colour scheme, the bell and the status line.
pub(super) fn settle(console: Console, settings: &Settings) -> Console {
    let console = console
        .with_flavour(settings.flavour)
        .with_scrollback(settings.scrollback)
        .with_scheme(settings.scheme.unwrap_or(&crate::scheme::LINUX))
        .with_bell(settings.bell);
    if settings.status_line {
        console.with_status_line()
    } else {
        console
    }
}

/// `host`, which sounds the bell on the PC speaker when `bell` asks for it
/// and the machine has one.
pub(super) fn with_bell<'h>(
    host: RealHost<'h, &'h mut File>,
    bell: Ring,
    places: &Places,
) -> RealHost<'h, &'h mut File> {
    match Speaker::find(&places.sys, &places.dev) {
        Some(speaker) if matches!(bell, Ring::Audible | Ring::Both) => host.with_speaker(speaker),
        _ => host,
    }
}
