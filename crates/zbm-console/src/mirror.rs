//! The mirror of the screen for the VM tests (`zbm.console_mirror`): the
//! text of each screen shown, after a form feed, written to a file that may
//! be slow to take it, such as a serial port.

#![allow(
    clippy::disallowed_methods,
    reason = "the mirror is a file the console writes"
)]

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

use crate::run::Shown;

/// Opens the mirror at `path` without blocking: when it cannot take a
/// screen, as a serial port whose line is full, the console goes on and
/// writes the rest later.
pub fn open_mirror(path: &Path) -> Option<File> {
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
        self.flush();
    }

    /// Writes what `out` takes now of the screens taken.
    pub fn flush(&mut self) {
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

    /// Whether some of the screens taken is left to write.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty() || self.next.is_some()
    }

    /// What it writes to.
    pub fn get_ref(&self) -> &W {
        &self.out
    }
}

impl<W: io::Write> Shown for Mirror<W> {
    fn show(&mut self, text: &str) {
        Mirror::show(self, text);
    }

    fn flush(&mut self) {
        Mirror::flush(self);
    }

    fn is_pending(&self) -> bool {
        Mirror::is_pending(self)
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A writer that takes at most `room` bytes, then would block.
    struct Narrow {
        written: Vec<u8>,
        room: usize,
    }

    impl io::Write for Narrow {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.room == 0 {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            let taken = bytes.len().min(self.room);
            self.written.extend_from_slice(&bytes[..taken]);
            self.room -= taken;
            Ok(taken)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_mirror_ends_with_the_screen_shown_when_the_line_was_full() {
        let mut mirror = Mirror::new(Narrow {
            written: Vec::new(),
            room: 3,
        });

        mirror.show("one");
        mirror.show("two");
        mirror.show("three");
        assert_eq!(mirror.get_ref().written, b"\x0con");
        mirror.flush();
        assert!(mirror.is_pending());
        // The line takes more: the screen begun ends, then the latest follows,
        // without another screen to show.
        mirror.out.room = 100;
        Shown::flush(&mut mirror);
        assert!(!Shown::is_pending(&mirror));
        assert_eq!(mirror.get_ref().written, b"\x0cone\x0cthree");
        Shown::show(&mut mirror, "three");
        mirror.show("three");
        assert_eq!(mirror.get_ref().written, b"\x0cone\x0cthree");
    }
}
