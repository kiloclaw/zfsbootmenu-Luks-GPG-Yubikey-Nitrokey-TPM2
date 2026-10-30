//! The real host of the console's loop: the controlling side of the
//! pseudo-terminal, the keyboards, and the requests of the control socket
//! ([`Control`]): the display handed back, a keyboard layout, a font's
//! size, a secret prompt. The relay that carries the terminal over to the
//! kernel console afterwards is in [`crate::relay`].

#![allow(
    clippy::disallowed_methods,
    reason = "the pseudo-terminal and the kernel console are this module's system interface"
)]

use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::WindowSize;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{self, Winsize};
use zbm_display::grid::Grid;
use zbm_display::input::Keyboards;
use zbm_sys::console::{Ready, Watch, wait_ready};

use crate::control::{Control, Wake};
use crate::run::{Host, Input};
use crate::screen::zoom::Zoom;
use crate::screen::{Locks, Status};
use state::StateDir;

mod cells;
mod modes;
mod state;
mod status;

pub use modes::Modes;

pub use status::POWER_SUPPLY;

/// Makes the grid of the image's fonts for a display of a size in pixels,
/// in cells as high as asked, or as at the start.
pub type Fonts<'f> = &'f mut dyn FnMut((u32, u32), Option<u16>) -> io::Result<Grid>;

/// How much is read at once.
pub(crate) const CHUNK: usize = 16 * 1024;

/// Reads what the programs wrote on `master`; at end of file, or `EIO`,
/// which Linux reports once no program holds the other side, they are
/// gone.
///
/// # Errors
///
/// When `master` cannot be read otherwise.
pub fn read_output(master: &mut impl Read) -> io::Result<Input> {
    let mut buffer = vec![0_u8; CHUNK];
    match master.read(&mut buffer) {
        Ok(0) => Ok(Input::Hangup),
        Ok(length) => {
            buffer.truncate(length);
            Ok(Input::Output(buffer))
        }
        Err(error) if error.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => {
            Ok(Input::Hangup)
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) =>
        {
            Ok(Input::Nothing)
        }
        Err(error) => Err(error),
    }
}

/// How much typed text waits for the programs, at most: what is typed
/// beyond it while they read nothing is lost, as the kernel's terminal
/// loses it.
const OUTGOING: usize = 64 * 1024;

/// What was typed for the programs and not yet taken by the terminal. The
/// controlling side of the pseudo-terminal does not block, so that a
/// program that reads nothing cannot stop the console; the bytes wait here
/// and are wiped: they may be a passphrase.
#[derive(Debug)]
pub struct Outgoing {
    bytes: Vec<u8>,
}

impl Default for Outgoing {
    fn default() -> Outgoing {
        Outgoing {
            bytes: Vec::with_capacity(OUTGOING),
        }
    }
}

impl Drop for Outgoing {
    fn drop(&mut self) {
        zbm_sys::wipe_vec(&mut self.bytes);
    }
}

impl Outgoing {
    /// Whether nothing waits.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Queues `bytes`, as much as there is room for.
    pub fn push(&mut self, bytes: &[u8]) {
        let room = OUTGOING.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(bytes.get(..room.min(bytes.len())).unwrap_or(&[]));
    }

    /// Writes what waits to `master`, as much as it takes now.
    ///
    /// # Errors
    ///
    /// When `master` cannot be written other than for want of room.
    pub fn flush(&mut self, master: &mut impl Write) -> io::Result<()> {
        let mut written = 0;
        let outcome = loop {
            let Some(rest) = self.bytes.get(written..).filter(|rest| !rest.is_empty()) else {
                break Ok(());
            };
            match master.write(rest) {
                Ok(0) => break Ok(()),
                Ok(length) => written += length,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break Ok(()),
                Err(error) => break Err(error),
            }
        };
        if written > 0 {
            self.bytes.drain(..written);
            // Draining moved the rest down; the old bytes behind it are
            // overwritten.
            for slot in self.bytes.spare_capacity_mut() {
                slot.write(0);
            }
        }
        outcome
    }
}

/// Opens a pseudo-terminal: its controlling side, for the console, and the
/// programs' side. Neither becomes this process's controlling terminal,
/// and neither is inherited by programs started with other files.
///
/// # Errors
///
/// When the kernel has no pseudo-terminals to give.
pub fn open_pty() -> io::Result<(File, File)> {
    let flags = OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC;
    let master = openpt(flags)?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let name = ptsname(&master, Vec::new())?;
    let slave = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(O_NOCTTY)
        .open(OsStr::from_bytes(name.as_bytes()))?;
    Ok((File::from(master), slave))
}

/// `O_NOCTTY`, the same on every Linux architecture.
const O_NOCTTY: i32 = 0o400;

/// Tells the programs on `master` the size of their terminal.
///
/// # Errors
///
/// When `master` is not a terminal.
pub fn set_size(master: impl AsFd, size: &WindowSize) -> io::Result<()> {
    let winsize = Winsize {
        ws_row: size.num_lines,
        ws_col: size.num_cols,
        ws_xpixel: size.num_cols.saturating_mul(size.cell_width),
        ws_ypixel: size.num_lines.saturating_mul(size.cell_height),
    };
    termios::tcsetwinsize(master, winsize).map_err(io::Error::from)
}

/// Which keyboard layout is asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change<'n> {
    /// The one `zbm console layout` asked for, by name.
    Requested(&'n str),
    /// The image's next one (Super+Space).
    Next,
}

/// What the console does when another keyboard layout is asked for: gives
/// the keyboards the layout, when the image has it.
pub type Switch<'h> = Box<dyn FnMut(&mut Keyboards, Change<'_>) + 'h>;

/// How long the loop waits when the host is woken by everything it waits
/// for: once a minute is a safety net, not a poll.
pub const IDLE: Duration = Duration::from_secs(60);

/// How long the keyboards are looked for on every tick after a device
/// appeared: its keys may be described in sysfs a moment after its node.
const RESCAN_WINDOW: Duration = Duration::from_secs(2);

/// Reads `file` until it has nothing more, which acknowledges a wake-up or
/// the news of a device.
fn drain(file: &mut File) {
    let mut buffer = [0_u8; 512];
    while matches!(file.read(&mut buffer), Ok(length) if length > 0) {}
}

/// The pseudo-terminal, the keyboards, and the requests of the control
/// socket.
pub struct RealHost<'h, M> {
    master: M,
    outgoing: Outgoing,
    keyboards: Keyboards,
    /// What the control socket asks.
    control: Arc<Control>,
    /// Gives the keyboards another layout.
    switch: Option<Switch<'h>>,
    /// Readable when a request came.
    wake: Option<Box<dyn Wake + 'h>>,
    /// Readable when a device appeared in `/dev/input`.
    devices: Option<File>,
    /// Until when keyboards are looked for on every tick.
    rescan_until: Option<Instant>,
    /// The runtime's state directory: the scrollback and screenshots
    /// saved, and the font the console draws with.
    state: Option<StateDir>,
    /// Makes the grids of the image's fonts.
    fonts: Option<Fonts<'h>>,
    /// The font's size `zbm console font` asked for.
    font_request: Option<Zoom>,
    /// The battery's charge, for the status line.
    battery: status::Battery,
    /// The PC speaker, for the bell.
    beeper: status::Beeper,
    /// The client of the channel.
    client: Option<&'h zbm_sys::socket::Connection>,
    /// Whether the client was found gone, which the next wait says.
    is_client_gone: bool,
}

impl<M: std::fmt::Debug> std::fmt::Debug for RealHost<'_, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RealHost")
            .field("master", &self.master)
            .field("keyboards", &self.keyboards)
            .finish_non_exhaustive()
    }
}

impl<'h, M: Read + Write + AsFd> RealHost<'h, M> {
    /// A host on `master`, with keys from `keyboards`, which does what
    /// `control` asks. `master` stops blocking.
    pub fn new(master: M, keyboards: Keyboards, control: Arc<Control>) -> RealHost<'h, M> {
        let _ = rustix::io::ioctl_fionbio(&master, true);
        RealHost {
            master,
            outgoing: Outgoing::default(),
            keyboards,
            control,
            switch: None,
            wake: None,
            devices: None,
            rescan_until: None,
            state: None,
            fonts: None,
            font_request: None,
            battery: status::Battery::default(),
            beeper: status::Beeper::default(),
            client: None,
            is_client_gone: false,
        }
    }

    /// The host, which saves the scrollback and screenshots in the
    /// runtime's state directory `base`, and records the font there.
    #[must_use]
    pub fn with_state(mut self, base: &std::path::Path) -> Self {
        self.state = Some(StateDir {
            base: base.to_owned(),
        });
        self
    }

    /// The host, which sounds the bell on `speaker`.
    #[must_use]
    pub fn with_speaker(mut self, speaker: zbm_display::speaker::Speaker) -> Self {
        self.beeper.speaker = Some(speaker);
        self
    }

    /// The host, which makes grids of the image's fonts with `fonts` when
    /// the font's size changes.
    #[must_use]
    pub fn with_fonts(mut self, fonts: Fonts<'h>) -> Self {
        self.fonts = Some(fonts);
        self
    }

    /// The host, woken when `wake` becomes readable, as a request makes
    /// it, rather than looking for requests on every tick.
    #[must_use]
    pub fn with_wake(mut self, wake: impl Wake + 'h) -> Self {
        self.wake = Some(Box::new(wake));
        self
    }

    /// The host, which looks for new keyboards when `devices`, an inotify
    /// watch on `/dev/input`, becomes readable, rather than on every tick.
    #[must_use]
    pub fn with_device_watch(mut self, devices: File) -> Self {
        self.devices = Some(devices);
        self
    }

    /// Whether keyboards are looked for on every tick: without a watch on
    /// the devices, or for a moment after one appeared.
    fn is_rescanning(&self) -> bool {
        self.devices.is_none()
            || self
                .rescan_until
                .is_some_and(|until| Instant::now() < until)
    }

    /// The host, which runs `switch` when another keyboard layout is asked
    /// for.
    #[must_use]
    pub fn with_layouts(mut self, switch: Switch<'h>) -> Self {
        self.switch = Some(switch);
        self
    }

    /// The keyboard layout keys are translated with.
    pub fn keyboard_layout(&self) -> &str {
        self.keyboards.layout()
    }

    /// The controlling side of the pseudo-terminal, given back; what was
    /// typed and not taken is dropped.
    pub fn into_master(self) -> M {
        self.master
    }

    /// Keys from the first keyboard that has some: the keyboards come
    /// before the programs' output, so that Ctrl-C reaches a program that
    /// floods the terminal. A keyboard that was ready with no key, as one
    /// that also moves a mouse is, gives `None`: the output is read then,
    /// so that moving the mouse holds up nothing.
    fn keys(&mut self, ready: &[Ready]) -> io::Result<Option<Input>> {
        let Some(index) = ready.iter().position(|ready| ready.is_ready()) else {
            return Ok(None);
        };
        let presses = self.keyboards.read_device(index)?;
        Ok((!presses.is_empty()).then_some(Input::Keys(presses)))
    }

    /// What the host waits on: the keyboards, the programs' terminal, and
    /// the wake-up and the device watch when it has them; and how many
    /// keyboards come first.
    fn watches(&self) -> (Vec<Watch<'_>>, usize) {
        let mut watches: Vec<Watch<'_>> =
            self.keyboards.fds().into_iter().map(Watch::read).collect();
        let count = watches.len();
        // Scroll Lock holds the programs' output: it waits in the terminal,
        // and they wait once it is full, as on the kernel console.
        watches.push(Watch {
            fd: self.master.as_fd(),
            read: !self.keyboards.is_scroll_lock(),
            write: !self.outgoing.is_empty(),
        });
        if let Some(wake) = &self.wake {
            watches.push(Watch::read(wake.fd()));
        }
        if let Some(devices) = &self.devices {
            watches.push(Watch::read(devices.as_fd()));
        }
        // Last, as `is_client_ready` finds it.
        if let Some(client) = self.client {
            watches.push(Watch::read(client.as_fd()));
        }
        (watches, count)
    }

    /// Takes the wake-up and the news of devices that `extra`, the
    /// readiness of the files after the programs' terminal, report; looks
    /// for keyboards when a device appeared.
    fn take_wake_ups(&mut self, extra: &[Ready]) {
        let mut extra = extra.iter().copied();
        let woken = self.wake.is_some() && extra.next().is_some_and(Ready::is_ready);
        let appeared = self.devices.is_some() && extra.next().is_some_and(Ready::is_ready);
        if woken && let Some(wake) = &mut self.wake {
            wake.take();
        }
        if appeared && let Some(devices) = &mut self.devices {
            drain(devices);
            self.keyboards.rescan();
            self.rescan_until = Some(Instant::now() + RESCAN_WINDOW);
        }
    }

    /// Takes the requests that came: gives the keyboards the layout asked
    /// for, if one was, and answers; keeps the font's size asked for.
    fn take_requests(&mut self) {
        if let Some(name) = self.control.take_layout() {
            if let Some(switch) = &mut self.switch {
                switch(&mut self.keyboards, Change::Requested(&name));
            }
            self.control.layout_answered(&name, self.keyboards.layout());
        }
        if let Some(zoom) = self.control.take_font() {
            self.font_request = Some(zoom);
        }
    }
}

impl<M: Read + Write + AsFd> Host for RealHost<'_, M> {
    fn wait(&mut self, timeout: Duration) -> io::Result<Input> {
        self.beeper.quiet(Instant::now());
        if self.control.is_release_asked() {
            // The request's wake-up is taken with it.
            if let Some(wake) = &mut self.wake {
                wake.take();
            }
            return Ok(Input::Release);
        }
        if let Some(gone) = self.take_client_gone() {
            return Ok(gone);
        }
        self.take_requests();
        let (watches, count) = self.watches();
        let ready = wait_ready(&watches, Some(timeout))?;
        let (keyboards, rest) = ready.split_at(count.min(ready.len()));
        let master = rest.first().copied().unwrap_or_default();
        self.take_wake_ups(rest.get(1..).unwrap_or_default());
        if master.write {
            self.outgoing.flush(&mut self.master)?;
        }
        if let Some(keys) = self.keys(keyboards)? {
            return Ok(keys);
        }
        if self.is_client_ready(&ready) {
            return Ok(self.read_client());
        }
        if (master.read && !self.keyboards.is_scroll_lock()) || master.closed {
            return read_output(&mut self.master);
        }
        if self.control.is_release_asked() {
            return Ok(Input::Release);
        }
        if !master.write && self.is_rescanning() {
            // Nothing happened for a while: a keyboard may have come.
            self.keyboards.rescan();
        }
        self.take_requests();
        Ok(Input::Nothing)
    }

    /// [`IDLE`] when requests wake the host and new devices are watched,
    /// else the tick that looks for them.
    fn idle(&self) -> Duration {
        let idle = if self.wake.is_some() && !self.is_rescanning() {
            IDLE
        } else {
            crate::run::TICK
        };
        // A tone stops on time.
        self.beeper
            .remaining(Instant::now())
            .map_or(idle, |remaining| remaining.min(idle))
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.outgoing.push(bytes);
        self.outgoing.flush(&mut self.master)
    }

    fn next_layout(&mut self) {
        if let Some(switch) = &mut self.switch {
            switch(&mut self.keyboards, Change::Next);
            self.control.layout_set(self.keyboards.layout());
        }
    }

    fn switch_layout(&mut self, name: &str) {
        if let Some(switch) = &mut self.switch {
            switch(&mut self.keyboards, Change::Requested(name));
            self.control.layout_set(self.keyboards.layout());
        }
    }

    fn layout_name(&self) -> String {
        self.keyboards.layout().to_owned()
    }

    fn send_client(&mut self, message: &zbm_display::channel::FromConsole) {
        self.send_to_client(message);
    }

    fn pending_accent(&self) -> Option<char> {
        self.keyboards.pending_accent()
    }

    fn save_scrollback(&mut self, text: &[u8]) -> io::Result<std::path::PathBuf> {
        let state = self.state.as_ref().ok_or(io::ErrorKind::Unsupported)?;
        state.save_scrollback(text)
    }

    fn grid(&mut self, pixels: (u32, u32), height: Option<u16>) -> Option<Grid> {
        self.fonts.as_mut()?(pixels, height).ok()
    }

    fn font_request(&mut self) -> Option<Zoom> {
        self.font_request.take()
    }

    fn status(&mut self) -> Option<Status> {
        let keyboards = &self.keyboards;
        Some(Status {
            layout: keyboards.layout().to_owned(),
            locks: Locks {
                caps: keyboards.is_caps_lock(),
                num: keyboards.is_num_lock(),
                scroll: keyboards.is_scroll_lock(),
            },
            clock: status::clock(),
            battery: self
                .battery
                .charge(std::path::Path::new(POWER_SUPPLY), Instant::now()),
        })
    }

    fn ring(&mut self) {
        self.beeper.ring(Instant::now());
    }

    fn save_screenshot(&mut self, text: &[u8], image: &[u8]) -> io::Result<std::path::PathBuf> {
        let state = self.state.as_ref().ok_or(io::ErrorKind::Unsupported)?;
        state.save_screenshot(text, image)
    }

    fn font_shown(&mut self, font: &str, size: &WindowSize) -> io::Result<()> {
        if let Some(state) = &self.state {
            state.font_shown(font);
        }
        self.control.font_shown(font);
        set_size(&self.master, size)
    }

    fn is_secret(&self) -> bool {
        let modes = termios::tcgetattr(&self.master).map(|modes| modes.local_modes);
        // Lines read without echo: `read -s`, `getpass`, `cryptsetup`; a
        // line editor or a full screen program reads keys without either.
        let is_password = modes.is_ok_and(|modes| {
            modes.contains(termios::LocalModes::ICANON)
                && !modes.contains(termios::LocalModes::ECHO)
        });
        is_password || self.control.is_secret()
    }
}

#[cfg(test)]
mod tests;
