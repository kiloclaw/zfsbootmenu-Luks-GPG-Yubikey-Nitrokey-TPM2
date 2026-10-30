//! The console's loop: what the programs write goes to the terminal and,
//! drawn, to the display; what is typed goes to the programs; until the
//! programs are gone or the display must be handed back.

#![allow(
    clippy::disallowed_methods,
    reason = "a synchronized update ends after real time, as Alacritty's parser counts it"
)]

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use alacritty_terminal::event::WindowSize;
use zbm_display::display::Screen;
use zbm_display::grid::Grid;
use zbm_display::keys::KeyPress;

use crate::keys;
use crate::screen::zoom::Zoom;
use crate::screen::{Console, Status};
use crate::typing::{Preview, Taken, Typing};

/// How long the loop waits when nothing is due, so that a request to hand
/// the display back is seen soon.
pub const TICK: Duration = Duration::from_millis(200);

/// How long output that keeps coming is read before it is drawn: a program
/// that writes a lot is drawn once per burst, not once per read.
pub const BURST: Duration = Duration::from_millis(8);

/// What happened while the loop waited.
#[derive(Debug, Eq, PartialEq)]
pub enum Input {
    /// The programs wrote this.
    Output(Vec<u8>),
    /// These keys were pressed.
    Keys(Vec<KeyPress>),
    /// No program holds the terminal any more.
    Hangup,
    /// The display must be handed back (`zbm console release`).
    Release,
    /// The time ran out.
    Nothing,
}

/// Where the programs and the keys are.
pub trait Host {
    /// Waits at most `timeout` for something to happen.
    ///
    /// # Errors
    ///
    /// When the terminal or the keyboards cannot be read.
    fn wait(&mut self, timeout: Duration) -> io::Result<Input>;

    /// Sends `bytes` to the programs.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be written.
    fn send(&mut self, bytes: &[u8]) -> io::Result<()>;

    /// How long the loop waits when nothing is due: [`TICK`], unless the
    /// host is woken by what it waits for, requests included.
    fn idle(&self) -> Duration {
        TICK
    }

    /// Switches the keyboards to the image's next layout (Super+Space).
    fn next_layout(&mut self) {}

    /// The accent of a dead key typed, while it waits for its next key.
    fn pending_accent(&self) -> Option<char> {
        None
    }

    /// Whether a secret prompt is open: a program of the runtime says so,
    /// or the programs' terminal reads lines without echoing them, as
    /// password prompts do. Nothing being typed is shown then, and no
    /// scrollback kept. A host that cannot tell says there is one.
    fn is_secret(&self) -> bool {
        true
    }

    /// Saves `text`, the console's scrollback, for `zbm report`; returns
    /// where.
    ///
    /// # Errors
    ///
    /// When it cannot be written; always for a host without a state
    /// directory.
    fn save_scrollback(&mut self, text: &[u8]) -> io::Result<PathBuf> {
        let _ = text;
        Err(io::ErrorKind::Unsupported.into())
    }

    /// The grid of the image's fonts for a display of `pixels`, in cells
    /// `height` pixels high, or as at the start without one. A host without
    /// fonts has none.
    fn grid(&mut self, pixels: (u32, u32), height: Option<u16>) -> Option<Grid> {
        let _ = (pixels, height);
        None
    }

    /// The font's size `zbm console font` asked for, if it did.
    fn font_request(&mut self) -> Option<Zoom> {
        None
    }

    /// Tells the programs that the terminal is `size` now, in the font
    /// described by `font`, and answers a request for it.
    ///
    /// # Errors
    ///
    /// When the programs' terminal cannot be told.
    fn font_shown(&mut self, font: &str, size: &WindowSize) -> io::Result<()> {
        let _ = (font, size);
        Ok(())
    }

    /// What the status line says: the layout, the locks, the time and the
    /// battery. A host that cannot tell says nothing.
    fn status(&mut self) -> Option<Status> {
        None
    }

    /// Sounds the bell on the PC speaker, if there is one.
    fn ring(&mut self) {}

    /// Saves a screenshot: the screen's `text` and its `image`, a PNG file,
    /// for `zbm report`; returns where the image went.
    ///
    /// # Errors
    ///
    /// When they cannot be written; always for a host without a state
    /// directory.
    fn save_screenshot(&mut self, text: &[u8], image: &[u8]) -> io::Result<PathBuf> {
        let _ = (text, image);
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Sounds the bell, says the status, and saves a screenshot, when the
/// console asks for them.
fn tell(console: &mut Console, host: &mut dyn Host) {
    // Bells that come together sound once.
    if console.take_sounds() > 0 {
        host.ring();
    }
    if console.has_status_line()
        && let Some(status) = host.status()
    {
        console.set_status(&status, Instant::now());
    }
    if let Some((text, mut image)) = console.take_screenshot() {
        let mut text = text.into_bytes();
        let notice = match host.save_screenshot(&text, &image) {
            Ok(path) => format!("Screenshot saved in {}", path.display()),
            Err(error) => format!("No screenshot: {error}"),
        };
        zbm_sys::wipe_vec(&mut text);
        zbm_sys::wipe_vec(&mut image);
        console.set_notice(&notice);
    }
}

/// How many sizes a step up or down tries.
const ZOOM_TRIES: usize = 8;

/// Changes the font's size when the keys or `zbm console font` asked for
/// it: the console draws in the grid the host makes, says which on its
/// bar, and the programs are told their new size.
fn zoom(console: &mut Console, host: &mut dyn Host) -> io::Result<()> {
    let Some(zoom) = console.take_zoom().or_else(|| host.font_request()) else {
        return Ok(());
    };
    let pixels = console.pixels();
    let current = console.grid().cell().1;
    let grid = zoom
        .heights(current)
        .into_iter()
        .take(ZOOM_TRIES)
        .filter_map(|height| host.grid(pixels, height))
        .find(|grid| zoom.is_met_by(grid, pixels, current));
    let is_new = grid.is_some();
    if let Some(grid) = grid {
        console.set_grid(grid);
    }
    let font = console.font_text();
    if is_new {
        console.set_notice(&font);
    } else {
        console.set_notice(&format!("No other size: {font}"));
    }
    host.font_shown(&font, &console.window_size())
}

/// Why the loop ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ended {
    /// The programs are gone.
    Hangup,
    /// The display must be handed back.
    Released,
}

/// Sends what `presses` type, then wipes it: it may be a passphrase.
fn type_keys(host: &mut dyn Host, presses: &[KeyPress], modes: keys::Modes) -> io::Result<()> {
    let mut typed = Vec::with_capacity(16 * presses.len());
    for &press in presses {
        keys::encode(press, modes, &mut typed);
    }
    let sent = host.send(&typed);
    zbm_sys::wipe_vec(&mut typed);
    sent
}

/// How long the loop waits at most while the text of a screen waits to be
/// written to the mirror: it is tried again then.
pub const MIRROR_RETRY: Duration = Duration::from_millis(50);

/// What gets the text of the screen after each frame: the VM tests'
/// mirror, or any function of the text.
pub trait Shown {
    /// Takes the text of the screen.
    fn show(&mut self, text: &str);

    /// Writes what is left of the screens taken.
    fn flush(&mut self) {}

    /// Whether some of the screens taken is still left to write: the loop
    /// tries again soon.
    fn is_pending(&self) -> bool {
        false
    }
}

impl<F: FnMut(&str)> Shown for F {
    fn show(&mut self, text: &str) {
        self(text);
    }
}

/// Gives the text of the screen to `shown`, when there is one: building
/// it for nothing would cost a frame's worth of work, and leave a copy of
/// the screen in freed memory.
fn show(console: &Console, shown: &mut Option<&mut dyn Shown>) {
    if let Some(shown) = shown {
        let text = console.text();
        shown.show(&text);
        zbm_sys::wipe_vec(&mut text.into_bytes());
    }
}

/// Runs `console` on `screen` with `host` until the programs are gone or
/// the display must be handed back. After each frame, `shown`, if given,
/// gets the text of the screen; what it could not write is tried again
/// every [`MIRROR_RETRY`]. What the programs wrote is wiped once
/// drawn: it may hold a secret they echoed. Output that keeps coming is
/// read for up to [`BURST`] before it is drawn.
///
/// # Errors
///
/// When the screen cannot be drawn on, or the host fails.
pub fn run(
    console: &mut Console,
    screen: &mut dyn Screen,
    host: &mut dyn Host,
    mut shown: Option<&mut dyn Shown>,
) -> io::Result<Ended> {
    screen.present(console.frame_mut())?;
    show(console, &mut shown);
    // When the output read since the last frame started coming.
    let mut burst: Option<Instant> = None;
    let mut typing = Typing::default();
    let mut pacing = Pacing::new(screen.interval());
    loop {
        if let Some(shown) = shown.as_deref_mut() {
            shown.flush();
        }
        let idle = if shown.as_deref().is_some_and(Shown::is_pending) {
            host.idle().min(MIRROR_RETRY)
        } else {
            host.idle()
        };
        let timeout = if burst.is_some() {
            Duration::ZERO
        } else {
            let deadline = console.deadline().into_iter().chain(pacing.due()).min();
            deadline.map_or(idle, |deadline| {
                deadline.saturating_duration_since(Instant::now()).min(idle)
            })
        };
        let input = host.wait(timeout)?;
        console.set_secret(host.is_secret());
        let is_output = matches!(input, Input::Output(_));
        if let Some(ended) = take(console, host, &mut typing, input)? {
            // A frame that waited for its time is not lost.
            pacing.present(console, screen, &mut shown, None)?;
            return Ok(ended);
        }
        zoom(console, host)?;
        tell(console, host);
        let now = Instant::now();
        console.tick(now);
        // A synchronized update whose time ran out answers the questions
        // it held, which the programs may be waiting for.
        send_replies(console, host)?;
        if is_output && now.duration_since(*burst.get_or_insert(now)) < BURST {
            continue;
        }
        burst = None;
        pacing.waiting |= console.draw() > 0;
        pacing.present(console, screen, &mut shown, Some(now))?;
    }
}

/// Frames presented at the display's refresh rate at most (C75): when the
/// last was, and whether one drawn since waits for its time.
struct Pacing {
    interval: Duration,
    presented: Instant,
    waiting: bool,
}

impl Pacing {
    /// Frames `interval` apart; the first is presented at once.
    fn new(interval: Duration) -> Pacing {
        let presented = Instant::now()
            .checked_sub(interval)
            .unwrap_or_else(Instant::now);
        Pacing {
            interval,
            presented,
            waiting: false,
        }
    }

    /// When the frame that waits is due, if one does.
    fn due(&self) -> Option<Instant> {
        self.waiting.then(|| self.presented + self.interval)
    }

    /// Presents the frame that waits, if its time came at `now`, or at once
    /// without `now`, and shows its text.
    fn present(
        &mut self,
        console: &mut Console,
        screen: &mut dyn Screen,
        shown: &mut Option<&mut dyn Shown>,
        now: Option<Instant>,
    ) -> io::Result<()> {
        let is_due = now.is_none_or(|now| now.duration_since(self.presented) >= self.interval);
        if !self.waiting || !is_due {
            return Ok(());
        }
        screen.present(console.frame_mut())?;
        show(console, shown);
        self.presented = now.unwrap_or_else(Instant::now);
        self.waiting = false;
        Ok(())
    }
}

/// Takes `presses`: the console's own keys through `typing`, the rest to
/// the programs; then shows what is being typed at the cursor, when the
/// programs' terminal echoes.
fn take_keys(
    console: &mut Console,
    host: &mut dyn Host,
    typing: &mut Typing,
    presses: &[KeyPress],
) -> io::Result<()> {
    let mut kept = Vec::with_capacity(presses.len());
    for &press in presses {
        match typing.take(press) {
            Taken::Type(press) => kept.extend(console.take_key(press)),
            Taken::NextLayout => host.next_layout(),
            Taken::Nothing => {}
        }
    }
    if !kept.is_empty() {
        console.scroll_to_bottom();
    }
    if let Some(text) = console.take_save() {
        let mut bytes = text.into_bytes();
        let notice = match host.save_scrollback(&bytes) {
            Ok(path) => format!("Saved in {}", path.display()),
            Err(error) => format!("Not saved: {error}"),
        };
        zbm_sys::wipe_vec(&mut bytes);
        console.set_notice(&notice);
    }
    let typed = type_keys(host, &kept, console.key_modes());
    zbm_sys::wipe_values(&mut kept, KeyPress::BLANK);
    let preview = typing
        .preview()
        .or_else(|| host.pending_accent().map(Preview::of))
        .filter(|_| !host.is_secret());
    console.set_preview(preview);
    typed
}

/// Sends the programs the answers the terminal owes them, if any.
fn send_replies(console: &mut Console, host: &mut dyn Host) -> io::Result<()> {
    let replies = console.take_replies();
    if replies.is_empty() {
        Ok(())
    } else {
        host.send(&replies)
    }
}

/// Takes `input`: output to the terminal, wiped once fed, with the
/// answers it owes sent; keys to the programs, those of the console
/// through `typing`. Returns why the loop ends, when it does.
fn take(
    console: &mut Console,
    host: &mut dyn Host,
    typing: &mut Typing,
    input: Input,
) -> io::Result<Option<Ended>> {
    match input {
        Input::Output(mut bytes) => {
            console.feed(&bytes);
            zbm_sys::wipe_vec(&mut bytes);
            send_replies(console, host)?;
        }
        Input::Keys(mut presses) => {
            let typed = take_keys(console, host, typing, &presses);
            zbm_sys::wipe_values(&mut presses, KeyPress::BLANK);
            typed?;
        }
        Input::Hangup => return Ok(Some(Ended::Hangup)),
        Input::Release => return Ok(Some(Ended::Released)),
        Input::Nothing => {}
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
