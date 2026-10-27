//! Screenshots: Print Screen asks for the screen's text and image, which
//! the caller saves in the state directory; none at a secret prompt.

use zbm_display::keys::{Key, KeyPress};

use super::Console;

impl Console {
    /// Takes `press` when it is Print Screen; `Some(press)` otherwise.
    pub(super) fn take_shot_key(&mut self, press: KeyPress) -> Option<KeyPress> {
        if press.key != Key::Print || press.ctrl || press.alt || press.shift {
            return Some(press);
        }
        if self.secret {
            self.set_notice("No screenshot at a secret prompt");
        } else {
            self.is_shot = true;
        }
        None
    }

    /// The screen's text and image, a PNG file, when Print Screen asked
    /// for them, once. The caller wipes both once they are saved.
    pub fn take_screenshot(&mut self) -> Option<(String, Vec<u8>)> {
        std::mem::take(&mut self.is_shot)
            .then(|| (self.text(), zbm_display::png::screenshot(&self.frame)))
    }
}
