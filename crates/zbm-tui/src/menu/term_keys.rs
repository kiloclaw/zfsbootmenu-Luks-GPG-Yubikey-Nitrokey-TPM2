//! The key presses of the terminal, read from its bytes into buffers that
//! are wiped once decoded: what is typed may be a passphrase.
//!
//! The start-up view reads keys with this rather than with the terminal
//! library's event reader, which keeps the last bytes it read in a buffer
//! of its own until more come: a passphrase typed at start-up would stay in
//! the memory of the start-up program, which runs until the boot
//! (`crates/DESIGN.md`, "Secrets"). Keys decode into the presses the
//! display's keyboards give ([`KeyPress`]), as that reader would decode
//! them: Shift with capitals, Ctrl with the control characters, Alt with
//! Escape before a key, and the sequences of the Linux console and of xterm
//! for the keys that type nothing.

use std::fs::File;
use std::io::{self, Read as _};
use std::os::fd::{AsFd as _, BorrowedFd};
use std::time::{Duration, Instant};

use ratatui::crossterm::terminal;
use zbm_display::keys::KeyPress;
use zbm_sys::console::Watch;

use super::frontend::Input;

/// How much is read at once.
const BUFFER: usize = 256;

pub use zbm_display::term_keys::{
    CONSOLE_ESCAPE_WAIT, SERIAL_ESCAPE_WAIT, decode, ends_with_carriage_return,
};

/// What a decoded key is overwritten with.
fn blank() -> KeyPress {
    KeyPress::BLANK
}

/// The keys of the terminal on standard input, which is in raw mode while
/// the start-up view has it.
pub struct TerminalKeys {
    /// Standard input, read without a buffer of the standard library.
    input: File,
    /// Whether this set raw mode, which it ends.
    raw: bool,
    /// What was read and not decoded yet: the start of a key's sequence.
    pending: Vec<u8>,
    /// The keys decoded, and the next one to give.
    keys: Vec<KeyPress>,
    next: usize,
    /// How long the rest of a sequence may take
    /// ([`CONSOLE_ESCAPE_WAIT`], [`SERIAL_ESCAPE_WAIT`]).
    escape_wait: Duration,
    /// When the last bytes came.
    last_read: Option<Instant>,
    /// Whether the last byte decoded was a CR, whose LF may come in the
    /// next read: then it is part of the same Enter.
    after_cr: bool,
}

impl TerminalKeys {
    /// The keys of the terminal, which goes into raw mode; the rest of a
    /// key's sequence may take `escape_wait` to come.
    ///
    /// # Errors
    ///
    /// When standard input cannot be used, or raw mode set.
    pub fn new(escape_wait: Duration) -> io::Result<TerminalKeys> {
        let input = File::from(io::stdin().as_fd().try_clone_to_owned()?);
        let mut keys = TerminalKeys::on(input, escape_wait);
        keys.take()?;
        Ok(keys)
    }

    /// The keys read from `input`, whose mode this leaves alone.
    pub fn on(input: File, escape_wait: Duration) -> TerminalKeys {
        TerminalKeys {
            input,
            raw: false,
            pending: Vec::with_capacity(2 * BUFFER),
            // Each key takes a byte at least, so the keys of what is
            // pending fit without the list growing, which would leave a
            // copy of them behind unwiped.
            keys: Vec::with_capacity(2 * BUFFER),
            next: 0,
            escape_wait,
            last_read: None,
            after_cr: false,
        }
    }

    /// Reads and decodes what comes within `timeout`. The start of a
    /// sequence is decoded alone, as the Escape key, only once nothing more
    /// came for the escape wait, however short `timeout` is.
    ///
    /// # Errors
    ///
    /// When the input cannot be read, or ended.
    #[allow(
        clippy::disallowed_methods,
        reason = "the key reader is the terminal's adapter; gaps between bytes are wall-clock time"
    )]
    fn fill(&mut self, timeout: Duration, wake: Option<BorrowedFd<'_>>) -> io::Result<()> {
        let since = self.last_read.map(|at| at.elapsed());
        let wait = match since {
            Some(since) if !self.pending.is_empty() => {
                self.escape_wait.saturating_sub(since).min(timeout)
            }
            _ => timeout,
        };
        if self.read(wait, wake)? {
            self.last_read = Some(Instant::now());
        }
        let waited = self
            .last_read
            .is_none_or(|at| at.elapsed() >= self.escape_wait);
        self.decode(waited);
        Ok(())
    }

    /// Reads what came within `timeout`, or until `wake` is readable:
    /// whether anything came.
    ///
    /// # Errors
    ///
    /// When the input cannot be read, or ended.
    fn read(&mut self, timeout: Duration, wake: Option<BorrowedFd<'_>>) -> io::Result<bool> {
        let mut watches = vec![Watch::read(self.input.as_fd())];
        watches.extend(wake.map(Watch::read));
        let ready = zbm_sys::console::wait_ready(&watches, Some(timeout))?;
        if !ready.first().is_some_and(|input| input.is_ready()) {
            return Ok(false);
        }
        let mut buffer = [0_u8; BUFFER];
        let read = self.input.read(&mut buffer);
        if let Ok(count) = read {
            self.pending
                .extend_from_slice(buffer.get(..count).unwrap_or_default());
        }
        zbm_sys::wipe(&mut buffer);
        match read? {
            0 => Err(io::ErrorKind::UnexpectedEof.into()),
            _ => Ok(true),
        }
    }

    /// Decodes what was read, the start of a sequence too when `last`; the
    /// bytes decoded are wiped.
    fn decode(&mut self, last: bool) {
        // The carriage return counts for the byte right after it alone.
        if self.after_cr && !self.pending.is_empty() {
            self.after_cr = false;
            if self.pending.first() == Some(&b'\n') {
                self.pending.copy_within(1.., 0);
                let end = self.pending.len() - 1;
                zbm_sys::wipe(self.pending.get_mut(end..).unwrap_or_default());
                self.pending.truncate(end);
            }
        }
        let decoded = self.keys.len();
        let used = decode(&self.pending, last, &mut self.keys);
        if used > 0 {
            let bytes = self.pending.get(..used).unwrap_or_default();
            let keys = self.keys.get(decoded..).unwrap_or_default();
            self.after_cr = ends_with_carriage_return(bytes, keys);
        }
        let rest = self.pending.len() - used;
        self.pending.copy_within(used.., 0);
        zbm_sys::wipe(self.pending.get_mut(rest..).unwrap_or_default());
        self.pending.truncate(rest);
    }

    /// Wipes the keys decoded and given.
    fn forget(&mut self) {
        zbm_sys::wipe_values(&mut self.keys, blank());
        self.next = 0;
    }

    /// Wipes everything read.
    fn forget_all(&mut self) {
        self.forget();
        zbm_sys::wipe_vec(&mut self.pending);
        self.last_read = None;
        self.after_cr = false;
    }
}

impl Input for TerminalKeys {
    /// The next key press, waiting at most `timeout` for it.
    fn key(&mut self, timeout: Duration) -> io::Result<Option<KeyPress>> {
        self.key_or_wake(timeout, None)
    }

    /// The next key press, waiting at most `timeout` for it, and no longer
    /// once `wake` is readable, as when the machine sent something.
    fn key_or_wake(
        &mut self,
        timeout: Duration,
        wake: Option<BorrowedFd<'_>>,
    ) -> io::Result<Option<KeyPress>> {
        if self.next >= self.keys.len() {
            self.forget();
            self.fill(timeout, wake)?;
        }
        let key = self.keys.get(self.next).copied();
        if key.is_some() {
            self.next += 1;
        }
        Ok(key)
    }

    /// Ends raw mode; what was typed and not used is dropped.
    fn release(&mut self) -> io::Result<()> {
        self.forget_all();
        if self.raw {
            self.raw = false;
            terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    fn take(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        self.raw = true;
        Ok(())
    }

    /// The terminal's size, as the kernel knows it: what `stty` set on a
    /// serial console, the font's grid on a virtual console.
    fn size(&mut self) -> Option<(u16, u16)> {
        let size = terminal::window_size().ok()?;
        Some((size.columns, size.rows))
    }
}

impl Drop for TerminalKeys {
    fn drop(&mut self) {
        self.forget_all();
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

#[cfg(test)]
mod tests;
