//! The console's text for a Braille display (ROADMAP.md UI8, C85, ADR
//! 0011): `brltty` reads a virtual terminal's text from `/dev/vcsaN`, a
//! header of four bytes (rows, columns, the cursor's column and row), then
//! a character and an attribute for each cell. The console draws its own
//! cells, with its terminal in `KD_GRAPHICS`, so the kernel's copy holds
//! nothing; it writes its screen there instead, in the terminal's size, the
//! latest announcement over the last row, so that `brltty` reads it as it
//! reads any kernel console. The kernel draws none of it.

use std::path::{Path, PathBuf};

use zbm_display::decor::Decoration;
use zbm_display::faces::FontFace;
use zbm_display::grid::CellStyle;

use crate::scheme::Scheme;
use crate::screen::styled::Styled;
use crate::serial::nearest;

/// The bytes of the header.
pub const HEADER: usize = 4;

/// The VGA colour of each ANSI one: the kernel keeps attributes in VGA's
/// order, in which blue and red, cyan and yellow trade places.
const VGA: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

/// The text device of the virtual terminal `vt`: `/dev/vcsaN` for
/// `/dev/ttyN`; `None` for `/dev/tty0` and anything else.
pub fn device(vt: &Path) -> Option<PathBuf> {
    let name = vt.file_name()?.to_str()?;
    let number = name.strip_prefix("tty")?;
    let valid = !number.is_empty() && number != "0" && number.bytes().all(|b| b.is_ascii_digit());
    valid.then(|| vt.with_file_name(format!("vcsa{number}")))
}

/// The attribute of `style`: its colours as the 16 of the scheme, in VGA's
/// order, the background's in the high four bits.
fn attribute(style: &CellStyle, scheme: &Scheme) -> u8 {
    let vga = |index: u8| {
        let bright = index & 8;
        VGA.get(usize::from(index & 7)).copied().unwrap_or(7) | bright
    };
    let foreground = vga(nearest(style.fg, scheme));
    let background = vga(nearest(style.bg, scheme)) & 7;
    foreground | (background << 4)
}

/// The byte a character is written as: itself in ASCII, else `?`, which
/// every console font has.
fn byte(text: &str) -> u8 {
    match text.chars().next() {
        Some(c) if c.is_ascii() && !c.is_ascii_control() => {
            u8::try_from(u32::from(c)).unwrap_or(b'?')
        }
        Some(_) => b'?',
        None => b' ',
    }
}

/// The bytes of `styled` for a virtual terminal of `(rows, columns)`: the
/// header, then each cell, cut or filled with blanks in the scheme's
/// colours; `announced`, when there is something to say, over the last
/// row.
pub fn frame(
    styled: &Styled,
    scheme: &Scheme,
    (rows, columns): (u8, u8),
    announced: Option<&str>,
) -> Vec<u8> {
    let plain = CellStyle {
        fg: scheme.foreground,
        bg: scheme.background,
        decoration: Decoration::default(),
        face: FontFace::Regular,
    };
    let blank = [b' ', attribute(&plain, scheme)];
    let (column, row) = styled.cursor.unwrap_or_default();
    let clamp = |value: u16, limit: u8| {
        u8::try_from(value)
            .unwrap_or(u8::MAX)
            .min(limit.saturating_sub(1))
    };
    let mut bytes = vec![rows, columns, clamp(column, columns), clamp(row, rows)];
    let last = usize::from(rows).saturating_sub(1);
    for index in 0..usize::from(rows) {
        let mut written = 0;
        if let (true, Some(text)) = (index == last, announced) {
            for c in text.chars().take(usize::from(columns)) {
                bytes.extend([byte(&c.to_string()), blank[1]]);
                written += 1;
            }
        } else if let Some(cells) = styled.rows.get(index) {
            // The second cell of a wide character is empty: it keeps its
            // column, as a blank.
            for cell in cells.iter().take(usize::from(columns)) {
                bytes.extend([byte(&cell.text), attribute(&cell.style, scheme)]);
                written += 1;
            }
        }
        for _ in written..usize::from(columns) {
            bytes.extend(blank);
        }
    }
    bytes
}

#[cfg(test)]
mod tests;
