//! What the host keeps in the runtime's state directory: the record of a
//! secret prompt, the saved scrollback, and the font's size `zbm console
//! font` asks for, with the font the console draws with.

use std::path::{Path, PathBuf};

use zbm_platform::RealFiles;
use zbm_state::State;

use crate::screen::zoom::Zoom;

/// The runtime's state directory.
#[derive(Debug)]
pub(super) struct StateDir {
    pub base: PathBuf,
}

impl StateDir {
    fn state(&self) -> State<'_> {
        State::new(&RealFiles, &self.base)
    }

    /// Whether a living process records a secret prompt.
    pub fn is_secret_prompt_open(&self) -> bool {
        self.state()
            .secret_prompt_pid()
            .is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists())
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

    /// The font's size asked for, if a request waits; one that cannot be
    /// read asks for none, and is answered with the font as it is.
    pub fn font_request(&self) -> Option<Zoom> {
        let state = self.state();
        if !state.has_console_font_request() {
            return None;
        }
        Some(
            state
                .console_font_request()
                .as_deref()
                .and_then(Zoom::parse)
                .unwrap_or(Zoom::Height(0)),
        )
    }

    /// Records `font`, which the console draws with, and answers the
    /// request for it.
    pub fn font_shown(&self, font: &str) {
        let state = self.state();
        let _ = state.set_console_font(font);
        state.clear_console_font_request();
    }
}
