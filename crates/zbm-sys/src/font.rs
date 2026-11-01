//! The kernel console's font (ROADMAP.md CD5): its glyphs through
//! `KDFONTOP` and the characters each draws through `PIO_UNIMAP`, as
//! `setfont` loads them, without `setfont`.

use std::ffi::{c_uint, c_ushort};
use std::io;
use std::os::fd::AsFd;

use rustix::ioctl::{Opcode, Setter, ioctl};

/// `KDFONTOP` (`linux/kd.h`), numbered before `_IOC` and the same
/// everywhere.
const KDFONTOP: Opcode = 0x4B72;

/// `KD_FONT_OP_SET`: glyphs of at most 32 rows, each in 32.
const OP_SET: c_uint = 0;

/// `KD_FONT_OP_SET_TALL` (Linux 6.2): glyphs of as many rows as they have.
const OP_SET_TALL: c_uint = 4;

/// The rows `KD_FONT_OP_SET` gives each glyph.
const PITCH: u32 = 32;

/// `PIO_UNIMAPCLR`: clears the console's Unicode map.
const PIO_UNIMAPCLR: Opcode = 0x4B68;

/// `PIO_UNIMAP`: adds pairs to it.
const PIO_UNIMAP: Opcode = 0x4B67;

/// The most glyphs a console font has.
pub const MAX_GLYPHS: u32 = 512;

/// `struct console_font_op`.
#[repr(C)]
#[derive(Debug)]
struct FontOp {
    op: c_uint,
    flags: c_uint,
    width: c_uint,
    height: c_uint,
    charcount: c_uint,
    data: *mut u8,
}

/// `struct unimapinit`, its fields named as the kernel names them.
#[repr(C)]
#[allow(clippy::struct_field_names, reason = "the kernel's names")]
#[derive(Debug, Default)]
struct UnimapInit {
    advised_hashsize: c_ushort,
    advised_hashstep: c_ushort,
    advised_hashlevel: c_ushort,
}

/// `struct unipair`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct UniPair {
    unicode: c_ushort,
    fontpos: c_ushort,
}

/// `struct unimapdesc`.
#[repr(C)]
#[derive(Debug)]
struct UnimapDesc {
    entry_ct: c_ushort,
    entries: *mut UniPair,
}

/// A font as the kernel console takes it: `count` glyphs of `width` by
/// `height` pixels, one after the other, each of `height` rows of
/// `width.div_ceil(8)` bytes, the leftmost pixel in the highest bit; and
/// the characters each draws, as (code point, glyph).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct KernelFont {
    /// The cell's width in pixels.
    pub width: u32,
    /// Its height.
    pub height: u32,
    /// How many glyphs.
    pub count: u32,
    /// The glyphs' rows.
    pub glyphs: Vec<u8>,
    /// The characters of the Basic Multilingual Plane each glyph draws.
    pub map: Vec<(u16, u16)>,
}

impl KernelFont {
    /// The bytes of a row.
    fn row(&self) -> usize {
        usize::try_from(self.width.div_ceil(8)).unwrap_or(usize::MAX)
    }

    /// The glyphs laid out `pitch` rows apart, the rows below the glyph's
    /// left blank; `None` when the font is not whole.
    fn laid_out(&self, pitch: u32) -> Option<Vec<u8>> {
        let row = self.row();
        let height = usize::try_from(self.height).ok()?;
        let pitch = usize::try_from(pitch).ok()?;
        let count = usize::try_from(self.count).ok()?;
        let glyph = row.checked_mul(height)?;
        if self.glyphs.len() != glyph.checked_mul(count)? || pitch < height {
            return None;
        }
        let mut data = vec![0_u8; row.checked_mul(pitch)?.checked_mul(count)?];
        for (index, rows) in self.glyphs.chunks(glyph).enumerate() {
            let at = index * row * pitch;
            data.get_mut(at..at + glyph)?.copy_from_slice(rows);
        }
        Some(data)
    }
}

/// Loads `font` on the virtual terminal open as `fd`, then its Unicode
/// map: a font of more than 32 rows only on Linux 6.2 and later. The font
/// and its map are checked whole before the terminal is changed.
///
/// # Errors
///
/// When the font is not whole, has more than [`MAX_GLYPHS`] glyphs or its
/// map more than 65,535 pairs (`EINVAL`), or the kernel refuses it: not a
/// virtual terminal, a font it cannot show, or a process that may not
/// change the terminal.
pub fn set_font(fd: impl AsFd, font: &KernelFont) -> io::Result<()> {
    let (op, pitch) = if font.height <= PITCH {
        (OP_SET, PITCH)
    } else {
        (OP_SET_TALL, font.height)
    };
    let mut data = font.laid_out(pitch).ok_or_else(invalid)?;
    let mut pairs = pairs(&font.map)?;
    if font.count == 0 || font.count > MAX_GLYPHS {
        return Err(invalid());
    }
    let request = FontOp {
        op,
        flags: 0,
        width: font.width,
        height: font.height,
        charcount: font.count,
        data: data.as_mut_ptr(),
    };
    let fd = fd.as_fd();
    // SAFETY: `KDFONTOP` reads the `console_font_op` that `Setter` points
    // at, and through its `data` the glyphs, which `data` holds for as
    // long as the call: `charcount` glyphs of `pitch` rows of the row's
    // bytes, the size `laid_out` made it.
    unsafe { ioctl(fd, Setter::<KDFONTOP, FontOp>::new(request)) }?;
    // SAFETY: `PIO_UNIMAPCLR` reads the `unimapinit` that `Setter` points
    // at, and nothing else.
    unsafe {
        ioctl(
            fd,
            Setter::<PIO_UNIMAPCLR, UnimapInit>::new(UnimapInit::default()),
        )
    }?;
    let request = UnimapDesc {
        entry_ct: c_ushort::try_from(pairs.len()).map_err(|_| invalid())?,
        entries: pairs.as_mut_ptr(),
    };
    // SAFETY: `PIO_UNIMAP` reads the `unimapdesc` that `Setter` points at,
    // and through `entries` its `entry_ct` pairs, which `pairs` holds for
    // as long as the call.
    unsafe { ioctl(fd, Setter::<PIO_UNIMAP, UnimapDesc>::new(request)) }?;
    Ok(())
}

/// The kernel's pairs of `map`, (code point, glyph).
///
/// # Errors
///
/// `EINVAL` for more than 65,535, which the kernel cannot be told.
fn pairs(map: &[(u16, u16)]) -> io::Result<Vec<UniPair>> {
    if map.len() > usize::from(c_ushort::MAX) {
        return Err(invalid());
    }
    Ok(map
        .iter()
        .map(|&(unicode, fontpos)| UniPair { unicode, fontpos })
        .collect())
}

/// `EINVAL`.
fn invalid() -> io::Error {
    io::Error::from_raw_os_error(EINVAL)
}

/// `EINVAL`, the same on every Linux architecture.
const EINVAL: i32 = 22;

#[cfg(test)]
mod tests {
    #![allow(
        clippy::disallowed_methods,
        reason = "the null device, opened as a terminal is"
    )]

    use super::*;

    fn font(height: u32, count: u32) -> KernelFont {
        let rows = usize::try_from(height * count).unwrap();
        KernelFont {
            width: 8,
            height,
            count,
            glyphs: (0..rows)
                .map(|row| u8::try_from(row % 251).unwrap())
                .collect(),
            map: vec![(0x41, 1)],
        }
    }

    #[test]
    fn glyphs_are_laid_out_as_many_rows_apart_as_the_request_takes() {
        let small = font(2, 2);
        assert_eq!(small.laid_out(4).unwrap(), [0, 1, 0, 0, 2, 3, 0, 0]);
        assert_eq!(small.laid_out(2).unwrap(), [0, 1, 2, 3]);
        assert_eq!(small.laid_out(1), None);
        let cut = KernelFont {
            glyphs: vec![0; 3],
            ..small
        };
        assert_eq!(cut.laid_out(4), None);
        let wide = KernelFont {
            width: 12,
            height: 1,
            count: 1,
            glyphs: vec![0xff, 0xf0],
            map: Vec::new(),
        };
        assert_eq!(wide.laid_out(2).unwrap(), [0xff, 0xf0, 0, 0]);
    }

    #[test]
    fn a_font_that_is_not_one_or_a_file_that_is_no_terminal_is_refused() {
        let null = std::fs::File::open("/dev/null").unwrap();
        for wrong in [
            font(16, 0),
            font(16, 513),
            KernelFont {
                glyphs: vec![1],
                ..font(16, 1)
            },
        ] {
            let error = set_font(&null, &wrong).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(EINVAL), "{wrong:?}");
        }
        let crowded = KernelFont {
            map: vec![(0, 0); 65_536],
            ..font(16, 256)
        };
        assert_eq!(
            set_font(&null, &crowded).unwrap_err().raw_os_error(),
            Some(EINVAL)
        );
        let paired = pairs(&[(0x41, 3)]).unwrap();
        assert_eq!((paired[0].unicode, paired[0].fontpos), (0x41, 3));
        // Not a virtual terminal: the kernel refuses, short or tall.
        assert!(set_font(&null, &font(16, 256)).is_err());
        assert!(set_font(&null, &font(64, 256)).is_err());
    }
}
