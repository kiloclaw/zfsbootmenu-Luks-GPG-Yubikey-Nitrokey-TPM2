//! The character cell of a console font, from the header of its PSF file
//! (version 1 or 2, as `setfont` loads them).

/// How many bytes of a font the header needs.
pub const HEADER: usize = 32;

/// The magic number of PSF version 1.
const PSF1: [u8; 2] = [0x36, 0x04];

/// The magic number of PSF version 2.
const PSF2: [u8; 4] = [0x72, 0xb5, 0x4a, 0x86];

/// The size of a character, in pixels.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Cell {
    /// Pixels across.
    pub width: u16,
    /// Pixels down.
    pub height: u16,
}

impl Cell {
    /// The cell `setfont -d` makes of this one: twice as wide and as high.
    #[must_use]
    pub fn doubled(self) -> Cell {
        Cell {
            width: self.width.saturating_mul(2),
            height: self.height.saturating_mul(2),
        }
    }
}

/// The cell of the font whose file starts with `bytes`; `None` when they are
/// not a PSF header, or the cell is empty or larger than any console takes.
pub fn cell(bytes: &[u8]) -> Option<Cell> {
    let cell = if bytes.starts_with(&PSF1) {
        Cell {
            width: 8,
            height: u16::from(*bytes.get(3)?),
        }
    } else if bytes.starts_with(&PSF2) {
        let word = |at: usize| -> Option<u16> {
            let field: [u8; 4] = bytes.get(at..at + 4)?.try_into().ok()?;
            u16::try_from(u32::from_le_bytes(field)).ok()
        };
        Cell {
            width: word(28)?,
            height: word(24)?,
        }
    } else {
        return None;
    };
    let fits = (1..=64).contains(&cell.width) && (1..=128).contains(&cell.height);
    fits.then_some(cell)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The header of a PSF 2 font with cells of `width`×`height`.
    pub(crate) fn psf2(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PSF2.to_vec();
        for field in [
            0,
            32,
            0,
            512,
            height.saturating_mul(width.div_ceil(8)),
            height,
            width,
        ] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn psf2_and_psf1_headers_give_the_cell() {
        assert_eq!(
            cell(&psf2(16, 32)),
            Some(Cell {
                width: 16,
                height: 32
            })
        );
        assert_eq!(
            cell(&[0x36, 0x04, 0x02, 16]),
            Some(Cell {
                width: 8,
                height: 16
            })
        );
    }

    #[test]
    fn other_files_and_impossible_cells_give_nothing() {
        assert_eq!(cell(b"not a font"), None);
        assert_eq!(cell(&psf2(16, 32)[..20]), None);
        assert_eq!(cell(&psf2(0, 32)), None);
        assert_eq!(cell(&psf2(16, 200)), None);
        assert_eq!(cell(&psf2(u32::MAX, 32)), None);
        assert_eq!(cell(&[0x36, 0x04, 0x02]), None);
    }

    #[test]
    fn doubling_doubles_both_sides() {
        let cell = Cell {
            width: 12,
            height: 24,
        };

        assert_eq!(
            cell.doubled(),
            Cell {
                width: 24,
                height: 48
            }
        );
    }
}
