//! The terminal's settings.

use alacritty_terminal::term::{Config, Osc52};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};

/// The terminal's name, which XTVERSION reports with its version.
pub const NAME: &str = "ZFSBootMenu";

/// The terminal's settings: no scrollback until one is asked for, no
/// clipboard, a blinking block for a cursor, which blinks as the Linux
/// console's does, and a steady one in vi mode, the kitty
/// keyboard protocol for the programs that ask for it, and ZFSBootMenu's
/// name and version for those that ask who the terminal is.
pub(super) fn config() -> Config {
    Config {
        scrolling_history: 0,
        osc52: Osc52::Disabled,
        kitty_keyboard: true,
        default_cursor_style: CursorStyle {
            shape: CursorShape::Block,
            blinking: true,
        },
        vi_mode_cursor_style: Some(CursorStyle {
            shape: CursorShape::Block,
            blinking: false,
        }),
        name: NAME.to_owned(),
        version: zbm_platform::version::ZBM_VERSION.to_owned(),
        ..Config::default()
    }
}
