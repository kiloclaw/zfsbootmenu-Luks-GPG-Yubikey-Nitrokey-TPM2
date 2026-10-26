//! `zbm console` itself: the display, the keyboards and the virtual
//! terminal taken in that order, the loop on them, and the relay on the
//! kernel console once the display is handed back or cannot be had;
//! `zbm console release`, which asks for the hand-over, and
//! `zbm console layout`, which asks for another keyboard layout.
//!
//! The places it uses ([`Places`]) are the system's, or a test's.

#![allow(
    clippy::disallowed_methods,
    reason = "the console's own system interface: the kernel log, the mirror and the terminals"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Write as _};
use std::os::fd::{AsFd as _, OwnedFd};
use std::os::unix::fs::OpenOptionsExt as _;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use zbm_display::display::{Display, Screen as _, first_card};
use zbm_display::grid::Grid;
use zbm_display::input::Keyboards;
use zbm_display::layout::Layout;
use zbm_display::vt::{Terminal as VirtualTerminal, active_terminal};
use zbm_platform::RealFiles;
use zbm_state::State;

use crate::host::{RealHost, Relayed, Switch, relay, set_size};
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
/// screen, as a serial port whose line is full, the console goes on and
/// writes the rest later.
fn open_mirror(path: &Path) -> Option<File> {
    let mirror = OpenOptions::new().append(true).open(path).ok()?;
    rustix::io::ioctl_fionbio(&mirror, true).ok()?;
    Some(mirror)
}

/// The screens written to the mirror, each after a form feed. A screen is
/// written whole, even across calls; of the screens that come meanwhile,
/// only the latest is written, so that the mirror always ends with the
/// screen shown.
#[derive(Debug)]
pub struct Mirror<W> {
    out: W,
    /// What is left to write of the screen being written.
    pending: Vec<u8>,
    /// The latest screen, not written yet.
    next: Option<String>,
    /// The latest screen taken.
    last: String,
}

impl<W: io::Write> Mirror<W> {
    /// A mirror writing to `out`.
    pub fn new(out: W) -> Mirror<W> {
        Mirror {
            out,
            pending: Vec::new(),
            next: None,
            last: String::new(),
        }
    }

    /// Takes the screen `text`, and writes what `out` takes now.
    pub fn show(&mut self, text: &str) {
        if text != self.last {
            text.clone_into(&mut self.last);
            self.next = Some(format!("\x0c{text}"));
        }
        loop {
            if self.pending.is_empty() {
                match self.next.take() {
                    Some(screen) => self.pending = screen.into_bytes(),
                    None => return,
                }
            }
            match self.out.write(&self.pending) {
                Ok(0) | Err(_) => return,
                Ok(written) => {
                    self.pending.drain(..written.min(self.pending.len()));
                }
            }
        }
    }

    /// What it writes to.
    pub fn get_ref(&self) -> &W {
        &self.out
    }
}

/// Gives the font's grid for a display of a size in pixels.
pub type MakeGrid<'g> = &'g mut dyn FnMut((u32, u32)) -> io::Result<Grid>;

/// Loads a keyboard layout of the image by name.
pub type LoadLayout<'l> = &'l mut dyn FnMut(&str) -> Result<Layout, String>;

/// The keyboard layouts the console types with.
pub struct Layouts<'l> {
    /// The layout it starts with; the US layout without one.
    pub first: Option<Layout>,
    /// Loads the layouts `zbm console layout` asks for.
    pub load: LoadLayout<'l>,
}

impl std::fmt::Debug for Layouts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Layouts")
            .field("first", &self.first)
            .finish_non_exhaustive()
    }
}

/// The requests other processes send the console.
#[derive(Clone, Debug, Default)]
pub struct Requests {
    /// Set when the display must be handed back.
    pub release: Arc<AtomicBool>,
    /// Set when another keyboard layout is asked for.
    pub layout: Arc<AtomicBool>,
    /// Readable when either request came, so that the console waits on it
    /// instead of looking for requests on every tick.
    pub wake: Option<Arc<OwnedFd>>,
}

/// A descriptor that becomes readable when `zbm console release` or
/// `layout` signals this process: the signals' handlers write to it.
///
/// # Errors
///
/// When the socket or the handlers cannot be had.
fn wake_on_requests() -> io::Result<OwnedFd> {
    let (wake, alarm) = UnixStream::pair()?;
    wake.set_nonblocking(true)?;
    alarm.set_nonblocking(true)?;
    signal_hook::low_level::pipe::register(signal_hook::consts::SIGUSR1, alarm.try_clone()?)?;
    signal_hook::low_level::pipe::register(signal_hook::consts::SIGUSR2, alarm)?;
    Ok(OwnedFd::from(wake))
}

/// An inotify watch on the input devices below `dev`, readable when one
/// appears; `None` where the kernel has no inotify.
fn watch_devices(dev: &Path) -> Option<File> {
    use rustix::fs::inotify::{CreateFlags, WatchFlags, add_watch, init};
    let watch = init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK).ok()?;
    add_watch(
        &watch,
        dev.join("input"),
        WatchFlags::CREATE | WatchFlags::ATTRIB,
    )
    .ok()?;
    Some(File::from(watch))
}

/// Takes the request for a keyboard layout in `places`' state directory,
/// and gives `keyboards` the layout when `load` has it; the state records
/// the layout they type with before the request goes, which tells the
/// process that asked.
pub fn switch_layout(places: &Places, load: LoadLayout<'_>, keyboards: &mut Keyboards) {
    let state = State::new(&RealFiles, &places.base);
    if let Some(name) = state.console_layout_request() {
        match load(&name) {
            Ok(layout) => {
                keyboards.set_layout(Some(layout));
                places.log(6, &format!("keyboard {name}"));
            }
            Err(error) => {
                let kept = keyboards.layout();
                places.log(4, &format!("{error}; keeping keyboard {kept}"));
            }
        }
    }
    let _ = state.set_console_layout(keyboards.layout());
    state.clear_console_layout_request();
}

/// The console of the boot image: the programs on standard input, the
/// virtual terminal shown now.
///
/// # Errors
///
/// When the requests' signals, standard input or the virtual terminal
/// cannot be had, or the relay fails.
pub fn serve(
    settings: &Settings,
    places: &Places,
    grid: MakeGrid<'_>,
    layouts: Layouts<'_>,
) -> io::Result<()> {
    // The flags first: the handlers run in the order they were registered,
    // so a request is recorded before the console wakes for it.
    let requests = Requests {
        release: crate::release::on_request()?,
        layout: crate::release::on_layout_request()?,
        wake: wake_on_requests().ok().map(Arc::new),
    };
    // The console records itself, once it can take requests: a console
    // that ended before is never waited for by `zbm console release`.
    let state = State::new(&RealFiles, &places.base);
    let _ = state.create();
    let _ = state.set_console(std::process::id());
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
    serve_on(settings, places, (grid, layouts), console, &requests)
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

/// Runs the console on the display, with the font's grid and the keyboard
/// layouts of `fonts`, until the programs are gone, or until `requests`
/// asks for the display back; then relays on the kernel console, as it
/// does at once when it cannot use the display. Relaying after the
/// hand-over, it takes the display again when asked (`zbm console
/// reattach`, after `kexec` failed), with the layout it had.
///
/// # Errors
///
/// When the relay fails.
pub fn serve_on(
    settings: &Settings,
    places: &Places,
    fonts: (MakeGrid<'_>, Layouts<'_>),
    terminals: Terminals<'_>,
    requests: &Requests,
) -> io::Result<()> {
    let Terminals {
        master,
        terminal,
        vt,
    } = terminals;
    let (grid, Layouts { mut first, load }) = fonts;
    let state = State::new(&RealFiles, &places.base);
    loop {
        let layouts = Layouts {
            first: first.take(),
            load: &mut *load,
        };
        let shown = on_display(
            settings,
            places,
            (&mut *grid, layouts),
            master,
            vt,
            requests,
        );
        let Some((text, released)) = after_display(settings, places, shown) else {
            return Ok(());
        };
        if let Some(layout) = released.as_deref().filter(|&layout| layout != "us") {
            first = load(layout).ok();
        }
        // Recorded as released before the record that tells the hand-over
        // is done goes, so that a failed `kexec` finds the console.
        let reattach = released.is_some().then(|| {
            requests.release.store(false, Ordering::SeqCst);
            let _ = state.set_released_console(std::process::id());
            &*requests.release
        });
        state.clear_console();
        let _ = terminal.write_all(format!("\x1b[H\x1b[J{}\n", text.trim_end()).as_bytes());
        let relayed = relay(master, terminal, reattach);
        state.clear_released_console();
        if relayed? == Relayed::Hangup {
            return Ok(());
        }
        places.log(6, "taking the display again");
        let _ = state.set_console(std::process::id());
    }
}

/// What follows the console on the display, which `shown` tells: `None`
/// when the programs are gone; otherwise the screen's text, for the kernel
/// console, and the keyboard layout when the display was handed back.
fn after_display(
    settings: &Settings,
    places: &Places,
    shown: io::Result<(Ended, String, String)>,
) -> Option<(String, Option<String>)> {
    match shown {
        Ok((Ended::Hangup, ..)) => None,
        Ok((Ended::Released, text, layout)) => {
            places.log(6, "handed the display back");
            settings.mirror_line(RELEASED);
            Some((text, Some(layout)))
        }
        Err(error) => {
            places.log(4, &format!("{error}; going on on the kernel console"));
            Some((String::new(), None))
        }
    }
}

/// How the console started: on which card and display, with which font
/// and keyboard layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Started {
    /// The DRM card.
    pub card: PathBuf,
    /// The display's size in pixels.
    pub display: (u32, u32),
    /// The font's name.
    pub font: String,
    /// The size of a cell in pixels.
    pub cell: (u32, u32),
    /// The columns and rows.
    pub grid: (u16, u16),
    /// The keyboard layout.
    pub keyboard: String,
}

impl std::fmt::Display for Started {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ((x, y), (width, height)) = (self.display, self.cell);
        let (columns, rows) = self.grid;
        write!(
            f,
            "on {}: {x}x{y} display, {} {width}x{height} cells, {columns}x{rows}, keyboard {}",
            self.card.display(),
            self.font,
            self.keyboard
        )
    }
}

/// Says that the console started, in the kernel log and the mirror, and
/// records its keyboard layout in the state directory.
fn announce(settings: &Settings, places: &Places, started: &Started) {
    let text = started.to_string();
    places.log(6, &text);
    let _ = State::new(&RealFiles, &places.base).set_console_layout(&started.keyboard);
    settings.mirror_line(&format!("zbm console: {text}"));
}

/// Draws the programs on `master` on the display, with the virtual
/// terminal `vt` taken over, until they are gone or `requests` asks for
/// the display back; returns why, the screen's last text and the keyboard
/// layout.
fn on_display(
    settings: &Settings,
    places: &Places,
    (grid, layouts): (MakeGrid<'_>, Layouts<'_>),
    master: &mut File,
    vt: &Path,
    requests: &Requests,
) -> io::Result<(Ended, String, String)> {
    let card = match &settings.card {
        Some(card) => card.clone(),
        None => first_card(&places.dev)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no DRM card"))?,
    };
    let mut display = Display::open(&card)?;
    let size = display.size();
    let grid = grid(size)?;
    let mut keyboards = Keyboards::open(&places.sys, &places.dev)?;
    let Layouts { first, load } = layouts;
    keyboards.set_layout(first);
    let taken = VirtualTerminal::take(vt)?;
    let started = Started {
        card: card.clone(),
        display: size,
        font: grid.name().to_owned(),
        cell: grid.cell(),
        grid: (0, 0),
        keyboard: keyboards.layout().to_owned(),
    };
    let mut console = Console::new(grid, size);
    set_size(&*master, &console.window_size())?;
    announce(
        settings,
        places,
        &Started {
            grid: console.size(),
            ..started
        },
    );
    let mut mirror = settings
        .mirror
        .as_deref()
        .and_then(open_mirror)
        .map(Mirror::new);
    let mut show = mirror
        .as_mut()
        .map(|mirror| move |text: &str| mirror.show(text));
    let shown = show.as_mut().map(|show| show as &mut dyn FnMut(&str));
    let switch: Switch<'_> = Box::new(|keyboards: &mut Keyboards| {
        switch_layout(places, &mut *load, keyboards);
    });
    let mut host = real_host(&mut *master, keyboards, requests, &places.dev, switch);
    let ended = run(&mut console, &mut display, &mut host, shown)?;
    let layout = host.keyboard_layout().to_owned();
    drop(host);
    drop(taken);
    Ok((ended, console.text(), layout))
}

/// The host of the loop on the display: the programs' terminal `master`,
/// the keyboards and the requests, woken by them and by new devices below
/// `dev` where the system allows.
fn real_host<'h>(
    master: &'h mut File,
    keyboards: Keyboards,
    requests: &Requests,
    dev: &Path,
    switch: Switch<'h>,
) -> RealHost<'h, &'h mut File> {
    let mut host = RealHost::new(master, keyboards, Arc::clone(&requests.release))
        .with_layouts(Arc::clone(&requests.layout), switch);
    if let Some(wake) = requests.wake.as_deref().and_then(|fd| fd.try_clone().ok()) {
        host = host.with_wake(File::from(wake));
    }
    if let Some(devices) = watch_devices(dev) {
        host = host.with_device_watch(devices);
    }
    host
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

/// `zbm console reattach`, after a failed `kexec`: asks the console that
/// handed the display back to take it again, and waits at most `timeout`
/// until it did. Returns whether it did; without such a console there is
/// nothing to do, and it is `true`.
///
/// # Errors
///
/// When the request cannot be sent.
pub fn reattach(base: &Path, timeout: Duration) -> io::Result<bool> {
    let files = RealFiles;
    let state = State::new(&files, base);
    let Some(pid) = state.released_console_pid() else {
        return Ok(true);
    };
    crate::release::request(pid, timeout, &mut || state.console_pid().is_some())
}

/// What became of a request for a keyboard layout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Switched {
    /// The console took it, and types with this layout now: the one asked
    /// for, or the one it kept.
    Done(String),
    /// No console reads the keyboards.
    NoConsole,
    /// The console did not take it in time.
    Late,
}

/// `zbm console layout <name>`: asks the console recorded in the state
/// directory `base` for the keyboard layout `name`, and waits at most
/// `timeout` until it took the request.
///
/// # Errors
///
/// When the request cannot be written or sent.
pub fn request_layout(base: &Path, name: &str, timeout: Duration) -> io::Result<Switched> {
    let files = RealFiles;
    let state = State::new(&files, base);
    let Some(pid) = state.console_pid() else {
        return Ok(Switched::NoConsole);
    };
    state
        .request_console_layout(name)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let taken =
        crate::release::request_layout(pid, timeout, &mut || !state.has_console_layout_request())?;
    if taken && !state.has_console_layout_request() {
        Ok(Switched::Done(state.console_layout().unwrap_or_default()))
    } else {
        state.clear_console_layout_request();
        Ok(Switched::Late)
    }
}

#[cfg(test)]
mod tests;
