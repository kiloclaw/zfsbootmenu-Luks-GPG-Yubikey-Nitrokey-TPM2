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
    mut shown: Option<&mut dyn FnMut(&str)>,
) -> io::Result<Ended> {
    screen.present(console.frame_mut())?;
    show(console, &mut shown);
    // When the output read since the last frame started coming.
    let mut burst: Option<Instant> = None;
    loop {
        let timeout = if burst.is_some() {
            Duration::ZERO
        } else {
            console.deadline().map_or(host.idle(), |deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(host.idle())
            })
        };
        let input = host.wait(timeout)?;
        let is_output = matches!(input, Input::Output(_));
        if let Some(ended) = take(console, host, input)? {
            return Ok(ended);
        }
        let now = Instant::now();
        console.tick(now);
        if is_output && now.duration_since(*burst.get_or_insert(now)) < BURST {
            continue;
        }
        burst = None;
        if console.draw() > 0 {
            screen.present(console.frame_mut())?;
            show(console, &mut shown);
        }
    }
}

/// Takes `input`: output to the terminal, wiped once fed, with the
/// answers it owes sent; keys to the programs. Returns why the loop ends,
/// when it does.
fn take(console: &mut Console, host: &mut dyn Host, input: Input) -> io::Result<Option<Ended>> {
    match input {
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
        Input::Hangup => return Ok(Some(Ended::Hangup)),
        Input::Release => return Ok(Some(Ended::Released)),
        Input::Nothing => {}
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
