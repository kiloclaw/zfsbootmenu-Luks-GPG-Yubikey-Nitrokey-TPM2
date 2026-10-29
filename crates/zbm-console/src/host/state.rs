//! What the host keeps in the runtime's state directory: the saved
//! scrollback and screenshots, and the font the console draws with, for
//! those who read it there.

use std::path::PathBuf;

use zbm_platform::RealFiles;
use zbm_state::State;

/// The runtime's state directory.
#[derive(Debug)]
pub(super) struct StateDir {
    pub base: PathBuf,
}

impl StateDir {
    fn state(&self) -> State<'_> {
        State::new(&RealFiles, &self.base)
    }

    /// Saves `text`, the scrollback; returns where.
    pub fn save_scrollback(&self, text: &[u8]) -> std::io::Result<PathBuf> {
        self.state()
            .save_scrollback(text)
            .map_err(std::io::Error::other)
    }

    /// Saves a screenshot, its `text` and its `image`; returns where the
    /// image went.
    pub fn save_screenshot(&self, text: &[u8], image: &[u8]) -> std::io::Result<PathBuf> {
        self.state()
            .save_screenshot(text, image)
            .map_err(std::io::Error::other)
    }

    /// Records `font`, which the console draws with.
    pub fn font_shown(&self, font: &str) {
        let _ = self.state().set_console_font(font);
    }
}
