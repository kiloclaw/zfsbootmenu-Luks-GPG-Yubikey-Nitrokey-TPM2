//! The console's loop: what the programs write goes to the terminal and,
//! drawn, to the display; what is typed goes to the programs; until the
//! programs are gone or the display must be handed back.

#![allow(
    clippy::disallowed_methods,
    reason = "a synchronized update ends after real time, as Alacritty's parser counts it"
)]

use std::io;
use std::time::{Duration, Instant};

use zbm_display::display::Screen;
use zbm_display::keys::KeyPress;

use crate::keys;
use crate::screen::Console;

/// How long the loop waits when nothing is due, so that a request to hand
/// the display back is seen soon.
pub const TICK: Duration = Duration::from_millis(200);

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
fn type_keys(
    host: &mut dyn Host,
    presses: &[KeyPress],
    application_cursor: bool,
) -> io::Result<()> {
    let mut typed = Vec::with_capacity(16 * presses.len());
    for &press in presses {
        keys::encode(press, application_cursor, &mut typed);
    }
    let sent = host.send(&typed);
    zbm_sys::wipe_vec(&mut typed);
    sent
}

/// Gives the text of the screen to `shown`, when there is one: building
/// it for nothing would cost a frame's worth of work, and leave a copy of
/// the screen in freed memory.
fn show(console: &Console, shown: &mut Option<&mut dyn FnMut(&str)>) {
    if let Some(shown) = shown {
        shown(&console.text());
    }
}

/// Runs `console` on `screen` with `host` until the programs are gone or
/// the display must be handed back. After each frame, `shown`, if given,
/// gets the text of the screen. What the programs wrote is wiped once
/// drawn: it may hold a secret they echoed.
///
/// # Errors
///
/// When the screen cannot be drawn on, or the host fails.
pub fn run(
    console: &mut Console,
    screen: &mut dyn Screen,
    host: &mut dyn Host,
    mut shown: Option<&mut dyn FnMut(&str)>,
) -> io::Result<Ended> {
    screen.present(console.frame_mut())?;
    show(console, &mut shown);
    loop {
        let timeout = console.deadline().map_or(host.idle(), |deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .min(host.idle())
        });
        match host.wait(timeout)? {
            Input::Output(mut bytes) => {
                console.feed(&bytes);
                zbm_sys::wipe_vec(&mut bytes);
                let replies = console.take_replies();
                if !replies.is_empty() {
                    host.send(&replies)?;
                }
            }
            Input::Keys(mut presses) => {
                let typed = type_keys(host, &presses, console.is_application_cursor());
                zbm_sys::wipe_values(&mut presses, KeyPress::BLANK);
                typed?;
            }
            Input::Hangup => return Ok(Ended::Hangup),
            Input::Release => return Ok(Ended::Released),
            Input::Nothing => console.tick(Instant::now()),
        }
        if console.draw() > 0 {
            screen.present(console.frame_mut())?;
            show(console, &mut shown);
        }
    }
}

#[cfg(test)]
mod tests;
