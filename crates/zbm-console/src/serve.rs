//! `zbm console` itself: the display, the keyboards and the virtual
//! terminal taken in that order, the loop on them, and the relay on the
//! kernel console once the display is handed back or cannot be had; and
//! `zbm console release`, which asks for the hand-over.
//!
//! The places it uses ([`Places`]) are the system's, or a test's.

#![allow(
    clippy::disallowed_methods,
    reason = "the console's own system interface: the kernel log, the mirror and the terminals"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::AsFd as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use zbm_display::display::{Display, Screen as _, first_card};
use zbm_display::grid::Grid;
use zbm_display::input::Keyboards;
use zbm_display::vt::{Terminal as VirtualTerminal, active_terminal};
use zbm_platform::RealFiles;
use zbm_state::State;

use crate::host::{RealHost, relay, set_size};
use crate::run::{Ended, run};
use crate::screen::Console;

/// What the mirror gets once the display is handed back.
pub const RELEASED: &str = "zbm console: handed the display back";

/// `O_NOCTTY`, the same on every Linux architecture.
const O_NOCTTY: i32 = 0o400;

/// Where the console finds the system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Places {
    /// sysfs: the virtual terminal shown, the keyboards.
    pub sys: PathBuf,
    /// The device files: the DRM cards, the input devices.
    pub dev: PathBuf,
    /// The runtime's state directory, which holds the console's process ID.
    pub base: PathBuf,
    /// The kernel log.
    pub kmsg: PathBuf,
}

impl Places {
    /// The system's.
    pub fn system() -> Places {
        Places {
            sys: PathBuf::from("/sys"),
            dev: PathBuf::from("/dev"),
            base: PathBuf::from(zbm_state::BASE),
            kmsg: PathBuf::from("/dev/kmsg"),
        }
    }

    /// Writes `text` to the kernel log at `level`; failures are ignored.
    pub fn log(&self, level: u8, text: &str) {
        if let Ok(mut kmsg) = OpenOptions::new().append(true).open(&self.kmsg) {
            let _ = writeln!(kmsg, "<{level}>ZFSBootMenu: console: {text}");
        }
    }
}

/// What the console is asked for.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Settings {
    /// The DRM card; the first otherwise.
    pub card: Option<PathBuf>,
    /// Where to write the screen's text, after a form feed, for the VM
    /// tests.
    pub mirror: Option<PathBuf>,
}

impl Settings {
    /// Writes `line` to the mirror, if there is one, as a screen of its
    /// own.
    fn mirror_line(&self, line: &str) {
        if let Some(path) = &self.mirror
            && let Ok(mut mirror) = OpenOptions::new().append(true).open(path)
        {
            let _ = mirror.write_all(format!("\x0c{line}\n").as_bytes());
        }
    }
}

/// Opens the mirror at `path` without blocking: when it cannot take a
/// screen, as a serial port whose line is full, the screen is lost rather
/// than the console stopped.
fn open_mirror(path: &Path) -> Option<File> {
    let mirror = OpenOptions::new().append(true).open(path).ok()?;
    rustix::io::ioctl_fionbio(&mirror, true).ok()?;
    Some(mirror)
}

/// Gives the font's grid for a display of a size in pixels.
pub type MakeGrid<'g> = &'g mut dyn FnMut((u32, u32)) -> io::Result<Grid>;

/// The console of the boot image: the programs on standard input, the
/// virtual terminal shown now.
///
/// # Errors
///
/// When the request's signal, standard input or the virtual terminal
/// cannot be had, or the relay fails.
pub fn serve(settings: &Settings, places: &Places, grid: MakeGrid<'_>) -> io::Result<()> {
    let requested = crate::release::on_request()?;
    let mut master = File::from(io::stdin().as_fd().try_clone_to_owned()?);
    let vt = active_terminal(&places.sys, &places.dev).unwrap_or_else(|| places.dev.join("tty0"));
    let mut terminal = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(O_NOCTTY)
        .open(&vt)
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", vt.display())))?;
    let console = Terminals {
        master: &mut master,
        terminal: &mut terminal,
        vt: &vt,
    };
    serve_on(settings, places, grid, console, requested)
}

/// The programs' terminal and the kernel console's.
#[derive(Debug)]
pub struct Terminals<'t> {
    /// The controlling side of the programs' pseudo-terminal.
    pub master: &'t mut File,
    /// The kernel console, for the relay.
    pub terminal: &'t mut File,
    /// The virtual terminal to take over while the console draws.
    pub vt: &'t Path,
}

/// Runs the console on the display until the programs are gone, or until
/// `requested` asks for the display back; then relays on the kernel
/// console, as it does at once when it cannot use the display.
///
/// # Errors
///
/// When the relay fails.
pub fn serve_on(
    settings: &Settings,
    places: &Places,
    grid: MakeGrid<'_>,
    terminals: Terminals<'_>,
    requested: Arc<AtomicBool>,
) -> io::Result<()> {
    let Terminals {
        master,
        terminal,
        vt,
    } = terminals;
    let text = match on_display(settings, places, grid, master, vt, requested) {
        Ok((Ended::Hangup, _)) => return Ok(()),
        Ok((Ended::Released, text)) => {
            places.log(6, "handed the display back");
            settings.mirror_line(RELEASED);
            text
        }
        Err(error) => {
            places.log(4, &format!("{error}; going on on the kernel console"));
            String::new()
        }
    };
    State::new(&RealFiles, &places.base).clear_console();
    let _ = terminal.write_all(format!("\x1b[H\x1b[J{}\n", text.trim_end()).as_bytes());
    relay(master, terminal)
}

/// Draws the programs on `master` on the display, with the virtual
/// terminal `vt` taken over, until they are gone or `requested` asks for
/// the display back; returns why, and the screen's last text.
fn on_display(
    settings: &Settings,
    places: &Places,
    grid: MakeGrid<'_>,
    master: &mut File,
    vt: &Path,
    requested: Arc<AtomicBool>,
) -> io::Result<(Ended, String)> {
    let card = match &settings.card {
        Some(card) => card.clone(),
        None => first_card(&places.dev)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no DRM card"))?,
    };
    let mut display = Display::open(&card)?;
    let size = display.size();
    let grid = grid(size)?;
    let keyboards = Keyboards::open(&places.sys, &places.dev)?;
    let taken = VirtualTerminal::take(vt)?;
    let (width, height) = grid.cell();
    let mut console = Console::new(grid, size);
    set_size(&*master, &console.window_size())?;
    let (columns, rows) = console.size();
    let (x, y) = size;
    let card = card.display();
    places.log(
        6,
        &format!("on {card}: {x}x{y} display, {width}x{height} cells, {columns}x{rows}"),
    );
    let mut mirror = settings.mirror.as_deref().and_then(open_mirror);
    let mut last = String::new();
    let mut shown = |text: &str| {
        if text != last {
            if let Some(mirror) = &mut mirror {
                let _ = mirror.write_all(format!("\x0c{text}").as_bytes());
            }
            text.clone_into(&mut last);
        }
    };
    let mut host = RealHost::new(&mut *master, keyboards, requested);
    let ended = run(&mut console, &mut display, &mut host, &mut shown)?;
    drop(taken);
    Ok((ended, console.text()))
}

/// What became of a request to hand the display back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Released {
    /// It was handed back, or no console held it.
    Done,
    /// The console did not hand it back in time.
    Late,
}

/// `zbm console release`: asks the console recorded in the state directory
/// `base` to hand the display back, and waits at most `timeout`.
///
/// # Errors
///
/// When the request cannot be sent.
pub fn release(base: &Path, timeout: Duration) -> io::Result<Released> {
    let files = RealFiles;
    let state = State::new(&files, base);
    let Some(pid) = state.console_pid() else {
        return Ok(Released::Done);
    };
    if crate::release::request(pid, timeout, &mut || state.console_pid().is_none())? {
        state.clear_console();
        Ok(Released::Done)
    } else {
        Ok(Released::Late)
    }
}

#[cfg(test)]
mod tests;
