//! `zbm console` itself: the display, the keyboards and the virtual
//! terminal taken in that order, the loop on them, and the relay on the
//! kernel console once the display is handed back or cannot be had, with
//! its control socket served meanwhile; and the requests to it:
//! `zbm console release`, which asks for the hand-over, `reattach`,
//! `layout`, `font` and `status`.
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

use zbm_display::input::{DeviceId, Keyboards};
use zbm_display::layout::{Layout, Source};
use zbm_display::seat::{self, Seat};
use zbm_display::vt::active_terminal;
use zbm_platform::RealFiles;
use zbm_platform::event::{CancelToken, spawn_scoped_traced};
use zbm_state::State;
use zbm_sys::socket::{Connection, Listener};

use crate::control::{Control, ControlWake};
use crate::host::{RealHost, Switch, set_size};
use crate::mirror::{Mirror, open_mirror};
use crate::relay::{Relayed, relay};
use crate::run::{Client, Ended, Shown, run_with_client};
use crate::screen::{Console, Ring};
use display::held_seat;
pub use layouts::{next_layout, switch_layout};
use setup::{console_for, settle, with_bell};

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
    /// The control socket.
    pub socket: PathBuf,
}

impl Places {
    /// The system's.
    pub fn system() -> Places {
        Places {
            sys: PathBuf::from("/sys"),
            dev: PathBuf::from("/dev"),
            base: PathBuf::from(zbm_state::BASE),
            kmsg: PathBuf::from("/dev/kmsg"),
            socket: PathBuf::from(zbm_app::console::SOCKET),
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
    /// The terminal description the programs run with, which the keys
    /// follow.
    pub flavour: crate::keys::Flavour,
    /// Num Lock at the start (`zbm.numlock`); as the keyboards' light shows
    /// it otherwise.
    pub num_lock: Option<bool>,
    /// The key repeat (`zbm.keyboard_repeat`): the delay and the period, in
    /// milliseconds; the keyboards' own otherwise.
    pub repeat: Option<(u32, u32)>,
    /// The lines of scrollback (`zbm.console_scrollback`).
    pub scrollback: usize,
    /// The colour scheme (`zbm.console_palette`); the Linux console's
    /// otherwise.
    pub scheme: Option<&'static crate::scheme::Scheme>,
    /// Whether the console has a status line (`zbm.console_status`).
    pub status_line: bool,
    /// What the bell does (`zbm.console_bell`).
    pub bell: Ring,
    /// The compose key, by code (`zbm.keyboard_compose`).
    pub compose_key: Option<u16>,
    /// The keyboards with a layout of their own (`zbm.keyboard_layouts`).
    pub device_layouts: Vec<(DeviceId, Source)>,
    /// The mode of the display (`zbm.console_mode`), when it has one of
    /// this size: width and height.
    pub mode: Option<(u16, u16)>,
    /// How long without a key blanks the screen (`zbm.console_blank`).
    pub blank: Option<std::time::Duration>,
    /// The descriptor of the console's end of the client channel, which
    /// the supervisor passed it (`--cells-fd`).
    pub cells_fd: Option<i32>,
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

/// Gives the font's grid for a display of a size in pixels, in cells as
/// high as asked, or as at the start.
pub type MakeGrid<'g> = crate::host::Fonts<'g>;

/// Loads a keyboard layout of the image by name.
pub type LoadLayout<'l> = &'l mut dyn FnMut(&str) -> Result<Layout, String>;

/// The keyboard layouts the console types with.
pub struct Layouts<'l> {
    /// The layout it starts with; the US layout without one.
    pub first: Option<Layout>,
    /// The image's layouts, which Super+Space goes through in order.
    pub names: Vec<String>,
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

/// The console of the boot image: the programs on standard input, the
/// virtual terminal shown now.
///
/// # Errors
///
/// When standard input or the virtual terminal cannot be had, or the relay
/// fails.
pub fn serve(
    settings: &Settings,
    places: &Places,
    grid: MakeGrid<'_>,
    layouts: Layouts<'_>,
) -> io::Result<()> {
    let (control, mut wake) = Control::new()?;
    // The console records itself, as hooks and users read it.
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
    let cells = settings.cells_fd.and_then(|fd| {
        let taken = Connection::inherited(fd);
        taken
            .inspect_err(|error| places.log(4, &format!("no client channel: {error}")))
            .ok()
    });
    let console = Terminals {
        master: &mut master,
        terminal: &mut terminal,
        vt: &vt,
        cells,
    };
    let control = (&control, &mut wake);
    with_socket(places, control.0, || {
        serve_on(settings, places, (grid, layouts), console, control)
    })
}

/// Runs `console` while a thread serves the control socket of `places`
/// for `control`; without the socket, as when it cannot be made, the
/// console runs and takes no request.
///
/// # Errors
///
/// What `console` fails with.
pub fn with_socket<T>(
    places: &Places,
    control: &Arc<Control>,
    console: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let listener = Listener::bind(&places.socket)
        .inspect_err(|error| {
            let socket = places.socket.display();
            places.log(4, &format!("{socket}: {error}; taking no request"));
        })
        .ok();
    let until = CancelToken::new()?;
    std::thread::scope(|scope| {
        if let Some(listener) = &listener {
            spawn_scoped_traced(scope, || crate::control::serve(control, listener, &until));
        }
        let ran = console();
        control.end();
        until.cancel();
        ran
    })
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
    /// The console's end of the client channel, if it has one: closed once
    /// the console cannot draw, so that the menu draws on its terminal.
    pub cells: Option<Connection>,
}

/// Runs the console on the display, with the font's grid and the keyboard
/// layouts of `fonts`, until the programs are gone, or until `control`
/// asks for the display back, which `wake` tells; then relays on the
/// kernel console, as it does at once when it cannot use the display.
/// Relaying after the hand-over, it takes the display again when asked
/// (`zbm console reattach`, after `kexec` failed), with the layout it had.
///
/// # Errors
///
/// When the relay fails.
pub fn serve_on(
    settings: &Settings,
    places: &Places,
    fonts: (MakeGrid<'_>, Layouts<'_>),
    terminals: Terminals<'_>,
    (control, wake): (&Arc<Control>, &mut ControlWake),
) -> io::Result<()> {
    let Terminals {
        master,
        terminal,
        vt,
        mut cells,
    } = terminals;
    let (grid, layouts) = fonts;
    let Layouts {
        mut first,
        names,
        load,
    } = layouts;
    let state = State::new(&RealFiles, &places.base);
    // The console's terminal while the display is handed back: the relay
    // keeps its screen up, which a failed `kexec` finds again; and the
    // seat handed back, to take again.
    let mut kept = None;
    let mut seat = None;
    let mut client = Client::default();
    loop {
        let layouts = Layouts {
            first: first.take(),
            names: names.clone(),
            load: &mut *load,
        };
        let shown = on_display(
            settings,
            places,
            (&mut *grid, layouts),
            (&mut *master, vt, cells.as_ref()),
            &mut client,
            (control, &mut *wake),
            (&mut kept, &mut seat),
        );
        let Some((text, released)) = after_display(settings, places, shown) else {
            return Ok(());
        };
        if let Some(layout) = released.as_deref().filter(|&layout| layout != "us") {
            first = load(layout).ok();
        }
        // Without the display, the console reads no client.
        cells = cells.filter(|_| released.is_some());
        let reattach = handed_back(&state, control, released.is_some());
        let _ = terminal.write_all(format!("\x1b[H\x1b[J{}\n", text.trim_end()).as_bytes());
        let relayed = relay(master, terminal, reattach, kept.as_mut());
        state.clear_released_console();
        if relayed? == Relayed::Hangup {
            return Ok(());
        }
        places.log(6, "taking the display again");
        let _ = state.set_console(std::process::id());
    }
}

/// Records in `state` that the console left the display, `released` when
/// it handed it back rather than failed to have it, and answers `control`:
/// what the relay waits on to take the display again, if it may.
fn handed_back<'c>(state: &State<'_>, control: &'c Control, released: bool) -> Option<&'c Control> {
    // Recorded as released, which the boot reads to know that a failed
    // `kexec` has a console to take the display again; then the hand-over
    // is answered.
    if released {
        let _ = state.set_released_console(std::process::id());
    }
    state.clear_console();
    if released {
        control.released();
        Some(control)
    } else {
        control.relaying();
        None
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

/// The keyboards there are, typing with `layout`, with Num Lock and the
/// key repeat of `settings`. A USB keyboard may enumerate after the
/// console starts: it starts without one, and the host adds it when it
/// comes.
fn open_keyboards(settings: &Settings, places: &Places, layout: Option<Layout>) -> Keyboards {
    let mut keyboards = Keyboards::find(&places.sys, &places.dev);
    if keyboards.count() == 0 {
        places.log(5, "no keyboard yet; waiting for one");
    }
    keyboards.set_compose_key(settings.compose_key);
    keyboards.set_layout(layout);
    keyboards.set_device_layouts(settings.device_layouts.clone());
    if let Some(on) = settings.num_lock {
        keyboards.set_num_lock(on);
    }
    if let Some((delay, period)) = settings.repeat {
        keyboards.set_repeat(delay, period);
    }
    keyboards
}

/// Draws the programs on `master` on the display, with the virtual
/// terminal `vt` taken over, until they are gone or `requests` asks for
/// the display back; returns why, the screen's last text and the keyboard
/// layout. The console's terminal is `kept` when the display is handed
/// back, for when it is taken again after a failed `kexec`, and so is the
/// seat, handed back.
fn on_display(
    settings: &Settings,
    places: &Places,
    (grid, layouts): (MakeGrid<'_>, Layouts<'_>),
    (master, vt, cells): (&mut File, &Path, Option<&Connection>),
    client: &mut Client,
    (control, wake): (&Arc<Control>, &mut ControlWake),
    (kept, seat): (&mut Option<Console>, &mut Option<Seat<seat::Released>>),
) -> io::Result<(Ended, String, String)> {
    let mut held = held_seat(settings, places, vt, seat)?;
    let card = held.card().to_path_buf();
    let size = held.size();
    let mut console = settle(console_for(kept, &mut *grid, size)?, settings);
    let Layouts { first, names, load } = layouts;
    let keyboards = open_keyboards(settings, places, first);
    set_size(&*master, &console.window_size())?;
    let started = Started {
        card: card.clone(),
        display: size,
        font: console.grid().name().to_owned(),
        cell: console.grid().cell(),
        grid: console.size(),
        keyboard: keyboards.layout().to_owned(),
    };
    announce(settings, places, &started);
    let _ = State::new(&RealFiles, &places.base).set_console_font(&console.font_text());
    control.shown(&started.keyboard, &console.font_text());
    let mut mirror = settings
        .mirror
        .as_deref()
        .and_then(open_mirror)
        .map(Mirror::new);
    let shown = mirror.as_mut().map(|mirror| mirror as &mut dyn Shown);
    let switch = layouts::switcher(places, names, load);
    let mut host = real_host(&mut *master, keyboards, (control, wake), places, switch);
    if let Some(cells) = cells {
        host = host.with_client(cells);
    }
    let mut host = with_bell(host, settings.bell, places).with_fonts(grid);
    let ended = run_with_client(&mut console, held.screen(), &mut host, shown, client)?;
    let layout = host.keyboard_layout().to_owned();
    drop(host);
    *seat = Some(held.release());
    let text = console.text();
    if ended == Ended::Released {
        *kept = Some(console);
    }
    Ok((ended, text, layout))
}

/// The host of the loop on the display: the programs' terminal `master`,
/// the keyboards and what `control` asks, woken by `wake` and by new
/// devices in `places` where the system allows, with its state directory.
fn real_host<'h>(
    master: &'h mut File,
    keyboards: Keyboards,
    (control, wake): (&Arc<Control>, &'h mut ControlWake),
    places: &Places,
    switch: Switch<'h>,
) -> RealHost<'h, &'h mut File> {
    let mut host = RealHost::new(master, keyboards, Arc::clone(control))
        .with_layouts(switch)
        .with_state(&places.base)
        .with_wake(wake);
    if let Some(devices) = watch_devices(&places.dev) {
        host = host.with_device_watch(devices);
    }
    host
}

mod client;
mod display;
mod layouts;
mod setup;

pub use client::{Released, Switched, reattach, release, request_font, request_layout, status};

#[cfg(test)]
mod tests;
