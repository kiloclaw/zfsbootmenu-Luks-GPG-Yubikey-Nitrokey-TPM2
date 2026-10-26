//! Telling the console on the display (`zbm.console=drm`) that the menu
//! shows a secret prompt, which it cannot see: the menu reads the keys
//! itself, without the terminal's echo. While the record is in the
//! runtime's state directory, the console keeps no scrollback and shows
//! nothing being typed.

use std::path::{Path, PathBuf};

use zbm_platform::RealFiles;
use zbm_state::State;

use super::model::{Dialog, Model};

/// Whether `model` shows a dialog where a secret is typed: a passphrase or
/// a PIN, not a wait for a device.
pub fn is_asking_secret(model: &Model) -> bool {
    matches!(&model.dialog, Some(Dialog::Unlock(prompt)) if prompt.wait.is_none())
}

/// The record of the menu's secret prompt in a state directory.
#[derive(Debug)]
pub struct Marker {
    base: PathBuf,
    shown: bool,
}

impl Marker {
    /// A record in the state directory `base`, not written yet.
    pub fn new(base: &Path) -> Marker {
        Marker {
            base: base.to_owned(),
            shown: false,
        }
    }

    /// Records that a secret prompt is shown, or removes the record, when
    /// that changed; failures are ignored, as without a state directory
    /// there is no console to tell.
    pub fn show(&mut self, secret: bool) {
        if secret == self.shown {
            return;
        }
        self.shown = secret;
        let state = State::new(&RealFiles, &self.base);
        if secret {
            let _ = state.set_secret_prompt(std::process::id());
        } else {
            state.clear_secret_prompt();
        }
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        self.show(false);
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
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
        model.dialog = Some(Dialog::Unlock(prompt.clone()));
        assert!(is_asking_secret(&model));
        model.dialog = Some(Dialog::Unlock(prompt.waiting(Some(10))));
        assert!(!is_asking_secret(&model));
    }

    #[test]
    fn the_record_follows_the_secret_prompt() {
        let dir = TempDir::new("zbm-tui-secret");
        let state = State::new(&RealFiles, dir.path());
        let mut marker = Marker::new(dir.path());
        marker.show(false);
        assert_eq!(state.secret_prompt_pid(), None);
        marker.show(true);
        assert_eq!(state.secret_prompt_pid(), Some(std::process::id()));
        marker.show(true);
        marker.show(false);
        assert_eq!(state.secret_prompt_pid(), None);
        marker.show(true);
        drop(marker);
        assert_eq!(state.secret_prompt_pid(), None);
        // Without a state directory, nothing happens.
        Marker::new(&dir.path().join("none")).show(true);
    }
}
