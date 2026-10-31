//! What was typed for the programs and not yet taken by their terminal.

use std::io::{self, Write};

/// How much typed text waits for the programs, at most: what is typed
/// beyond it while they read nothing is lost, as the kernel's terminal
/// loses it.
pub(super) const OUTGOING: usize = 64 * 1024;

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
