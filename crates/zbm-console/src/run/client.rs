//! The client of the channel (ADR 0004, UI6): the menu, which says hello,
//! takes the screen to draw its cells on it, lends it to the programs'
//! terminal, and gets the keys while its cells are shown, each with the
//! layout it was typed on.

use zbm_display::channel::{Caps, FromClient, FromConsole, VERSION};
use zbm_display::keys::KeyPress;

use super::Host;
use crate::screen::Console;

/// What the console knows of its client: whether it said hello, and asks
/// for a secret.
#[derive(Debug, Default)]
pub struct Client {
    /// Whether it said hello in the console's version.
    is_greeted: bool,
    /// Whether the keys that come belong to a secret.
    is_secret: bool,
}

impl Client {
    /// Whether the keys that come belong to a secret the client asks for.
    pub(super) fn is_secret(&self) -> bool {
        self.is_secret
    }

    /// Takes `message`: what a client of another version or without a
    /// hello sends is ignored, but its hello, which is answered with the
    /// console's version, so that it draws on its terminal instead.
    pub(super) fn take(&mut self, console: &mut Console, host: &mut dyn Host, message: FromClient) {
        match message {
            FromClient::Hello { version } => {
                // A client that starts again starts afresh.
                self.leave(console);
                self.is_greeted = version == VERSION;
                host.send_client(&FromConsole::Hello { version: VERSION });
                if self.is_greeted {
                    host.send_client(&FromConsole::Caps(caps(console)));
                }
            }
            _ if !self.is_greeted => {}
            FromClient::Cells(runs) => console.put_cells(&runs),
            FromClient::Cursor(cursor) => console.end_cells_frame(cursor),
            FromClient::Secret(secret) => self.is_secret = secret,
            FromClient::SwitchLayout(name) => host.switch_layout(&name),
            // For a screen reader, which the console does not have yet
            // (UI8).
            FromClient::Announce(_) => {}
            FromClient::Lend => self.leave(console),
            FromClient::Take => console.show_cells(),
        }
    }

    /// The client is gone: the terminal shows again.
    pub(super) fn gone(&mut self, console: &mut Console) {
        self.leave(console);
        self.is_greeted = false;
    }

    /// The client no longer draws: the terminal shows, and the keys are
    /// the programs'.
    fn leave(&mut self, console: &mut Console) {
        self.is_secret = false;
        console.show_terminal();
    }

    /// Sends `presses` to the client, with the layout they were typed on.
    pub(super) fn send_keys(host: &mut dyn Host, presses: &[KeyPress]) {
        let layout = host.layout_name();
        for &press in presses {
            let key = FromConsole::Key {
                press,
                layout: layout.clone(),
            };
            host.send_client(&key);
        }
    }

    /// Tells the client the console's size, after the font's size
    /// changed.
    pub(super) fn resized(&self, console: &Console, host: &mut dyn Host) {
        if self.is_greeted {
            let (columns, rows) = console.size();
            host.send_client(&FromConsole::Resize { columns, rows });
        }
    }
}

/// What `console` shows: its cells, in colours as they are.
fn caps(console: &Console) -> Caps {
    let (columns, rows) = console.size();
    Caps {
        columns,
        rows,
        true_colour: true,
    }
}
