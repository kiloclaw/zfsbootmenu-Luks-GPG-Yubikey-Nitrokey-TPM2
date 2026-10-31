//! The console's text in its virtual terminal, for `brltty` (ROADMAP.md
//! UI8, C85): the device open, its size read from its header, and each
//! whole frame written over it when it changed ([`crate::vcsa`]).

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt as _;
use std::path::Path;
use std::time::Instant;

use super::RealHost;
use crate::screen::Console;
use crate::vcsa::{HEADER, frame};

/// The text device of the console's virtual terminal.
pub struct VcsaText {
    file: File,
    /// The terminal's rows and columns, as the kernel keeps them.
    size: (u8, u8),
    /// What was written last.
    written: Vec<u8>,
    /// Whether it failed: it is not written any more.
    is_gone: bool,
}

impl std::fmt::Debug for VcsaText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VcsaText")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl VcsaText {
    /// The text device at `path`, `/dev/vcsaN`.
    ///
    /// # Errors
    ///
    /// When it cannot be opened, or its header read.
    #[allow(
        clippy::disallowed_methods,
        reason = "the virtual terminal's text is this host's own device, as the terminal is"
    )]
    pub fn open(path: &Path) -> io::Result<VcsaText> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let mut header = [0_u8; HEADER];
        file.read_exact_at(&mut header, 0)?;
        Ok(VcsaText {
            file,
            size: (header[0], header[1]),
            written: Vec::new(),
            is_gone: false,
        })
    }

    /// The terminal's rows and columns.
    pub fn size(&self) -> (u8, u8) {
        self.size
    }

    /// Writes the screen of `console` at `now`, when it changed.
    pub fn show(&mut self, console: &Console, now: Instant) {
        if self.is_gone {
            return;
        }
        let bytes = frame(
            &console.styled(),
            console.scheme(),
            self.size,
            console.announcement(now),
        );
        if bytes == self.written {
            return;
        }
        if self.file.write_all_at(&bytes, 0).is_err() {
            self.is_gone = true;
            return;
        }
        self.written = bytes;
    }
}

impl<M> RealHost<'_, M> {
    /// The host, which writes the console's text into `text` too.
    #[must_use]
    pub fn with_vcsa(mut self, text: VcsaText) -> Self {
        self.vcsa = Some(text);
        self
    }
}

#[cfg(test)]
mod tests;
