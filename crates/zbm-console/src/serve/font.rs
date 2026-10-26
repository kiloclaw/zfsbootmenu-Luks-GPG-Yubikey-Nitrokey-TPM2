//! `zbm console font`: asks the console on the display for a font's size.

use std::io;
use std::path::Path;
use std::time::Duration;

use zbm_platform::RealFiles;
use zbm_state::State;

use super::Switched;

/// Asks the console recorded in the state directory `base` for the font's
/// size `size` (`+`, `-`, `default` or a height in pixels), and waits at
/// most `timeout` until it took the request; the font it draws with then
/// is in [`Switched::Done`].
///
/// # Errors
///
/// When the request cannot be written or sent.
pub fn request_font(base: &Path, size: &str, timeout: Duration) -> io::Result<Switched> {
    let files = RealFiles;
    let state = State::new(&files, base);
    let Some(pid) = state.console_pid() else {
        return Ok(Switched::NoConsole);
    };
    state
        .request_console_font(size)
        .map_err(|error| io::Error::other(error.to_string()))?;
    // The layout's signal: the console takes every request it finds.
    let taken =
        crate::release::request_layout(pid, timeout, &mut || !state.has_console_font_request())?;
    if taken && !state.has_console_font_request() {
        Ok(Switched::Done(state.console_font().unwrap_or_default()))
    } else {
        state.clear_console_font_request();
        Ok(Switched::Late)
    }
}
