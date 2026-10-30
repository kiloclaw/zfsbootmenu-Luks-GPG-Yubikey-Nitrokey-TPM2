//! The client of the channel on the real host (ADR 0004, UI6): its socket
//! read with the keyboards and the programs' terminal, and written without
//! waiting, so that a client that reads nothing cannot stop the console.

use std::io::{self, Read, Write};
use std::os::fd::AsFd;
use std::time::Duration;

use zbm_display::channel::{FromClient, FromConsole};
use zbm_sys::console::Ready;
use zbm_sys::socket::Connection;

use super::RealHost;
use crate::run::Input;

impl<'h, M: Read + Write + AsFd> RealHost<'h, M> {
    /// The host, whose client of the channel is at the other end of
    /// `client`, which stops blocking.
    #[must_use]
    pub fn with_client(mut self, client: &'h Connection) -> Self {
        let _ = client.set_nonblocking();
        self.client = Some(client);
        self
    }

    /// The host, which says what happens on the channel with `notes`, a
    /// line each, as in the kernel log.
    #[must_use]
    pub fn with_notes(mut self, notes: impl FnMut(&str) + 'h) -> Self {
        self.notes = Some(Box::new(notes));
        self
    }

    /// Whether the client's socket is ready, which it is when it is the
    /// last of `ready`, the readiness of what the host waits on.
    pub(super) fn is_client_ready(&self, ready: &[Ready]) -> bool {
        self.client.is_some() && ready.last().copied().is_some_and(Ready::is_ready)
    }

    /// What the client sent: a message, nothing when it was not one, or
    /// that the client is gone, which is not waited on again.
    pub(super) fn read_client(&mut self) -> Input {
        let Some(client) = self.client else {
            return Input::Nothing;
        };
        match client.receive(Some(Duration::ZERO)) {
            Ok(Some(packet)) if packet.is_empty() => self.client_gone(),
            Ok(Some(packet)) => FromClient::decode(&packet).map_or(Input::Nothing, Input::Client),
            // A packet too long was read and dropped.
            Ok(None) => Input::Nothing,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => Input::Nothing,
            Err(_) => self.client_gone(),
        }
    }

    /// The client is gone.
    fn client_gone(&mut self) -> Input {
        self.client = None;
        self.is_client_gone = false;
        Input::ClientGone
    }

    /// Whether the client was found gone since the host last said so; it
    /// is said once.
    pub(super) fn take_client_gone(&mut self) -> Option<Input> {
        std::mem::take(&mut self.is_client_gone).then(|| self.client_gone())
    }

    /// Sends `message` to the client: one it has no room for is lost, as
    /// keys typed while a program reads nothing are; a client that cannot
    /// be written is gone. The bytes are wiped: a key may be a
    /// passphrase's.
    pub(super) fn send_to_client(&mut self, message: &FromConsole) {
        let Some(client) = self.client else {
            return;
        };
        let mut packet = message.encode();
        let sent = client.send(&packet);
        zbm_sys::wipe_vec(&mut packet);
        if sent.is_err_and(|error| error.kind() != io::ErrorKind::WouldBlock) {
            self.is_client_gone = true;
        }
    }
}
