//! The kernel console's font (ROADMAP.md CD5): a font loaded on a virtual
//! terminal, as `setfont` loads it.

use std::fs::OpenOptions;
use std::io;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;

pub use zbm_sys::font::KernelFont;

/// Loading a font on a virtual terminal.
pub trait ConsoleFonts: Send + Sync {
    /// Loads `font` on the virtual terminal `console`, with its Unicode
    /// map.
    ///
    /// # Errors
    ///
    /// When the terminal cannot be opened, or the kernel takes no such
    /// font, as one too large.
    fn load(&self, console: &Path, font: &KernelFont) -> io::Result<()>;
}

/// The kernel's virtual terminals.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemFonts;

/// `O_NOCTTY`, the same on every Linux architecture.
const O_NOCTTY: i32 = 0o400;

impl ConsoleFonts for SystemFonts {
    #[allow(
        clippy::disallowed_methods,
        reason = "the terminal is opened for its requests, which Files has none of"
    )]
    fn load(&self, console: &Path, font: &KernelFont) -> io::Result<()> {
        let terminal = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(O_NOCTTY)
            .open(console)?;
        zbm_sys::font::set_font(&terminal, font)
    }
}

/// No virtual terminal, for programs that load no font.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoFonts;

impl ConsoleFonts for NoFonts {
    fn load(&self, _console: &Path, _font: &KernelFont) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_no_terminal_takes_no_font() {
        let font = KernelFont {
            width: 8,
            height: 16,
            count: 256,
            glyphs: vec![0; 16 * 256],
            map: Vec::new(),
        };
        assert!(SystemFonts.load(Path::new("/dev/null"), &font).is_err());
        assert!(
            SystemFonts
                .load(Path::new("/nonexistent/tty1"), &font)
                .is_err()
        );
        let error = NoFonts.load(Path::new("/dev/tty1"), &font).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}
