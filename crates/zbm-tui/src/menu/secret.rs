//! Telling the console on the display (`zbm.console=drm`) that the menu
//! shows a secret prompt, which it cannot see: the menu reads the keys
//! itself, without the terminal's echo. While the menu holds a `secret`
//! request open on the console's control socket
//! ([`zbm_app::console::hold_secret`]), the console keeps no scrollback and
//! shows nothing being typed; the request ends when the prompt does, or
//! the menu ends, however it ends.

use std::path::{Path, PathBuf};

use zbm_app::console::{SecretHold, hold_secret};

use super::model::{Dialog, Model};

/// Whether `model` shows a dialog where a secret is typed: a passphrase or
/// a PIN, not a wait for a device.
pub fn is_asking_secret(model: &Model) -> bool {
    matches!(&model.dialog, Some(Dialog::Unlock(prompt)) if prompt.wait.is_none())
}

/// The menu's secret prompt, as the console listening at a socket knows
/// it.
#[derive(Debug)]
pub struct Marker {
    socket: PathBuf,
    shown: bool,
    held: Option<SecretHold>,
}

impl Marker {
    /// The marker for the console listening at `socket`, which knows of no
    /// prompt yet.
    pub fn new(socket: &Path) -> Marker {
        Marker {
            socket: socket.to_owned(),
            shown: false,
            held: None,
        }
    }

    /// Tells the console that a secret prompt is shown, or no longer is,
    /// when that changed; without a console there is nobody to tell.
    pub fn show(&mut self, secret: bool) {
        if secret == self.shown {
            return;
        }
        self.shown = secret;
        self.held = if secret {
            hold_secret(&self.socket)
        } else {
            None
        };
    }

    /// Whether the console noted the prompt.
    pub fn is_held(&self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use zbm_testkit::TempDir;

    use super::*;

    #[test]
    fn a_dialog_for_a_passphrase_asks_a_secret_and_a_wait_does_not() {
        use crate::menu::caps::Capabilities;
        use crate::menu::demo::{DemoOptions, data};
        use crate::menu::model::Prompt;
        use crate::menu::theme::Theme;
        use crate::menu::world::Method;

        let snapshot = data::machine(&DemoOptions::default());
        let mut model = Model::new(snapshot, Theme::new(Capabilities::FULL));
        assert!(!is_asking_secret(&model));
        let prompt = Prompt::new("luks-tank".into(), Method::Passphrase, None, None);
        model.dialog = Some(Dialog::Unlock(prompt));
        assert!(is_asking_secret(&model));
        let waiting = Prompt::new("luks-tank".into(), Method::Passphrase, None, None);
        model.dialog = Some(Dialog::Unlock(waiting.waiting(Some(10))));
        assert!(!is_asking_secret(&model));
    }

    #[test]
    fn without_a_console_nobody_is_told() {
        let dir = TempDir::new("zbm-tui-secret");
        let mut marker = Marker::new(&dir.path().join("console.sock"));
        marker.show(true);
        assert!(!marker.is_held());
        marker.show(false);
        assert!(!marker.is_held());
    }
}
