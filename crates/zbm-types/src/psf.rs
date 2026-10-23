//! Console fonts in the PSF format that `setfont` loads, version 1 or 2, and
//! the 256-glyph subsets the boot image installs (ROADMAP.md #20).
//!
//! A console font with more than 256 glyphs costs colours: the console takes
//! a bit of each cell's attribute for the ninth bit of the glyph number. The
//! menu draws fewer than 256 different characters, so [`Font::subset`] keeps
//! the glyphs of [`repertoire`] and nothing else, and the console keeps all
//! 16 colours. The subset maps each character the font lacks to a look-alike
//! the font has ([`FALLBACKS`]), keeps ASCII at its own codes and the line
//! characters at their codes of code page 437, and keeps every character the
//! source font maps to a glyph it keeps.

use std::collections::BTreeMap;

/// The magic number of PSF version 1.
const PSF1: [u8; 2] = [0x36, 0x04];

/// The magic number of PSF version 2.
const PSF2: [u8; 4] = [0x72, 0xb5, 0x4a, 0x86];

/// The size of a PSF 2 header.
const PSF2_HEADER: u32 = 32;

/// The glyphs of a subset: what a console can draw with all 16 colours.
pub const SUBSET_GLYPHS: usize = 256;

/// The most glyphs a font may have: fonts are files of a boot image, but
/// nothing should make the parser allocate without bound.
const MAX_GLYPHS: usize = 65_536;

/// Why bytes are not a console font.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PsfError {
    /// Neither PSF magic number.
    #[error("not a PSF font")]
    NotPsf,
    /// The file ends before its header, glyphs or table do.
    #[error("the font is cut short")]
    Truncated,
    /// The header gives a cell or a number of glyphs no console takes.
    #[error("the font's header is not one a console takes")]
    BadHeader,
}

/// A console font: glyphs of `width` by `height` pixels, each with the
/// characters it draws.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Font {
    width: u32,
    height: u32,
    glyphs: Vec<Vec<u8>>,
    chars: Vec<Vec<char>>,
}

/// A subset and the characters of the repertoire it could not draw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Subset {
    /// The font, with [`SUBSET_GLYPHS`] glyphs.
    pub font: Font,
    /// Characters the source font had neither a glyph nor a look-alike for.
    pub missing: Vec<char>,
}

impl Font {
    /// The font in `bytes`, a PSF file of version 1 or 2.
    ///
    /// # Errors
    ///
    /// [`PsfError`] when the bytes are not a whole PSF font.
    pub fn parse(bytes: &[u8]) -> Result<Font, PsfError> {
        if bytes.starts_with(&PSF2) {
            parse_psf2(bytes)
        } else if bytes.starts_with(&PSF1) {
            parse_psf1(bytes)
        } else {
            Err(PsfError::NotPsf)
        }
    }

    /// The cell, as (width, height) in pixels.
    pub fn cell(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// How many glyphs it has.
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// The glyph that draws `c`, if any.
    pub fn glyph_of(&self, c: char) -> Option<usize> {
        self.chars.iter().position(|chars| chars.contains(&c))
    }

    /// Every character it draws.
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.chars.iter().flatten().copied()
    }

    /// Whether it can draw `c`.
    pub fn has(&self, c: char) -> bool {
        self.glyph_of(c).is_some()
    }

    /// The font as a PSF 2 file with a Unicode table.
    pub fn to_bytes(&self) -> Vec<u8> {
        let count = u32::try_from(self.glyphs.len()).unwrap_or(u32::MAX);
        let mut bytes = PSF2.to_vec();
        let size = row_bytes(self.width).saturating_mul(self.height);
        for field in [0, PSF2_HEADER, 1, count, size, self.height, self.width] {
            bytes.extend_from_slice(&field.to_le_bytes());
        }
        for glyph in &self.glyphs {
            bytes.extend_from_slice(glyph);
        }
        for chars in &self.chars {
            for c in chars {
                let mut utf8 = [0_u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
            }
            bytes.push(0xff);
        }
        bytes
    }

    /// The font with the glyphs of `repertoire` only, in [`SUBSET_GLYPHS`]
    /// glyphs (see the module's description).
    pub fn subset(&self, repertoire: &[char]) -> Subset {
        let mut slots: Vec<Option<usize>> = vec![None; SUBSET_GLYPHS];
        let mut placed: BTreeMap<usize, usize> = BTreeMap::new();
        let mut chars: Vec<Vec<char>> = vec![Vec::new(); SUBSET_GLYPHS];
        let mut missing = Vec::new();
        for &c in repertoire {
            let Some(glyph) = self.glyph_or_look_alike(c) else {
                missing.push(c);
                continue;
            };
            let slot = if let Some(&slot) = placed.get(&glyph) {
                slot
            } else {
                let Some(slot) = free_slot(&slots, c) else {
                    missing.push(c);
                    continue;
                };
                if let Some(entry) = slots.get_mut(slot) {
                    *entry = Some(glyph);
                }
                placed.insert(glyph, slot);
                slot
            };
            if let Some(list) = chars.get_mut(slot) {
                list.push(c);
            }
        }
        self.add_source_chars(&placed, &mut chars);
        let blank = vec![0; self.glyph_size()];
        let glyphs = slots
            .iter()
            .map(|slot| {
                slot.and_then(|glyph| self.glyphs.get(glyph).cloned())
                    .unwrap_or_else(|| blank.clone())
            })
            .collect();
        let font = Font {
            width: self.width,
            height: self.height,
            glyphs,
            chars,
        };
        Subset { font, missing }
    }

    /// The glyph of `c`, or of its first look-alike the font has.
    fn glyph_or_look_alike(&self, c: char) -> Option<usize> {
        self.glyph_of(c).or_else(|| {
            look_alikes(c)
                .iter()
                .find_map(|&look_alike| self.glyph_of(look_alike))
        })
    }

    /// Adds to each kept glyph the other characters the source font draws
    /// with it, unless the subset draws them with another glyph.
    fn add_source_chars(&self, placed: &BTreeMap<usize, usize>, chars: &mut [Vec<char>]) {
        let mut taken: Vec<char> = chars.iter().flatten().copied().collect();
        for (&glyph, &slot) in placed {
            let source = self.chars.get(glyph).map_or(&[][..], Vec::as_slice);
            for &c in source {
                if !taken.contains(&c)
                    && let Some(list) = chars.get_mut(slot)
                {
                    list.push(c);
                    taken.push(c);
                }
            }
        }
    }

    /// The bytes of one glyph.
    fn glyph_size(&self) -> usize {
        usize::try_from(row_bytes(self.width).saturating_mul(self.height)).unwrap_or(0)
    }
}

/// The bytes of one row of a glyph `width` pixels wide.
fn row_bytes(width: u32) -> u32 {
    width.div_ceil(8)
}

/// The slot for the glyph of `c`: its own code for ASCII, its code of code
/// page 437 for a line character, else the first free slot outside ASCII,
/// those codes of code page 437 last.
fn free_slot(slots: &[Option<usize>], c: char) -> Option<usize> {
    let wanted = if c.is_ascii_graphic() || c == ' ' {
        usize::try_from(u32::from(c)).ok()
    } else {
        CP437
            .iter()
            .find(|(line, _)| *line == c)
            .map(|(_, code)| usize::from(*code))
    };
    if let Some(slot) = wanted
        && slots.get(slot).is_some_and(Option::is_none)
    {
        return Some(slot);
    }
    let free = |slot: &usize| slots.get(*slot).is_some_and(Option::is_none);
    let reserved = |slot: &usize| CP437.iter().any(|(_, code)| usize::from(*code) == *slot);
    let others = || (0x80..SUBSET_GLYPHS).chain(0..0x20).chain([0x7f]);
    others()
        .filter(|slot| !reserved(slot))
        .chain(others().filter(reserved))
        .find(free)
}

/// A little-endian `u32` at `at`.
fn word(bytes: &[u8], at: usize) -> Result<u32, PsfError> {
    let field: [u8; 4] = bytes
        .get(at..at + 4)
        .and_then(|field| field.try_into().ok())
        .ok_or(PsfError::Truncated)?;
    Ok(u32::from_le_bytes(field))
}

/// A PSF 2 font.
fn parse_psf2(bytes: &[u8]) -> Result<Font, PsfError> {
    let header = usize::try_from(word(bytes, 8)?).map_err(|_| PsfError::BadHeader)?;
    let flags = word(bytes, 12)?;
    let count = usize::try_from(word(bytes, 16)?).map_err(|_| PsfError::BadHeader)?;
    let size = word(bytes, 20)?;
    let (height, width) = (word(bytes, 24)?, word(bytes, 28)?);
    let valid = (1..=64).contains(&width)
        && (1..=128).contains(&height)
        && size == row_bytes(width).saturating_mul(height)
        && (1..=MAX_GLYPHS).contains(&count)
        && header >= 32;
    if !valid {
        return Err(PsfError::BadHeader);
    }
    let size = usize::try_from(size).map_err(|_| PsfError::BadHeader)?;
    let glyphs = glyphs(bytes, header, count, size)?;
    let table_at = header + count * size;
    let chars = if flags & 1 == 1 {
        let table = bytes.get(table_at..).ok_or(PsfError::Truncated)?;
        utf8_table(table, count)
    } else {
        identity_table(count)
    };
    Ok(Font {
        width,
        height,
        glyphs,
        chars,
    })
}

/// A PSF 1 font: 8 pixels wide, 256 or 512 glyphs.
fn parse_psf1(bytes: &[u8]) -> Result<Font, PsfError> {
    let mode = *bytes.get(2).ok_or(PsfError::Truncated)?;
    let height = *bytes.get(3).ok_or(PsfError::Truncated)?;
    if height == 0 {
        return Err(PsfError::BadHeader);
    }
    let count = if mode & 1 == 1 { 512 } else { 256 };
    let size = usize::from(height);
    let glyphs = glyphs(bytes, 4, count, size)?;
    let chars = if mode & 2 == 2 {
        let table = bytes.get(4 + count * size..).ok_or(PsfError::Truncated)?;
        ucs2_table(table, count)
    } else {
        identity_table(count)
    };
    Ok(Font {
        width: 8,
        height: u32::from(height),
        glyphs,
        chars,
    })
}

/// `count` glyphs of `size` bytes from `at`.
fn glyphs(bytes: &[u8], at: usize, count: usize, size: usize) -> Result<Vec<Vec<u8>>, PsfError> {
    let end = count
        .checked_mul(size)
        .and_then(|length| length.checked_add(at))
        .ok_or(PsfError::BadHeader)?;
    let data = bytes.get(at..end).ok_or(PsfError::Truncated)?;
    Ok(data.chunks_exact(size).map(<[u8]>::to_vec).collect())
}

/// The characters of `count` glyphs from a PSF 2 table: UTF-8 characters,
/// then sequences after `0xfe` (left out), each entry ended by `0xff`.
fn utf8_table(table: &[u8], count: usize) -> Vec<Vec<char>> {
    let mut chars: Vec<Vec<char>> = table
        .split(|&byte| byte == 0xff)
        .take(count)
        .map(|entry| {
            let single = entry.split(|&byte| byte == 0xfe).next().unwrap_or_default();
            // Invalid bytes are left out; a U+FFFD the table names is kept.
            single
                .utf8_chunks()
                .flat_map(|chunk| chunk.valid().chars())
                .collect()
        })
        .collect();
    chars.resize(count, Vec::new());
    chars
}

/// The characters of `count` glyphs from a PSF 1 table: 16-bit code points,
/// then sequences after `0xfffe` (left out), each entry ended by `0xffff`.
fn ucs2_table(table: &[u8], count: usize) -> Vec<Vec<char>> {
    let points: Vec<u16> = table
        .chunks_exact(2)
        .map(|pair| {
            u16::from_le_bytes([
                pair.first().copied().unwrap_or(0),
                pair.get(1).copied().unwrap_or(0),
            ])
        })
        .collect();
    let mut chars: Vec<Vec<char>> = points
        .split(|&point| point == 0xffff)
        .take(count)
        .map(|entry| {
            let single = entry
                .split(|&point| point == 0xfffe)
                .next()
                .unwrap_or_default();
            single
                .iter()
                .filter_map(|&point| char::from_u32(u32::from(point)))
                .collect()
        })
        .collect();
    chars.resize(count, Vec::new());
    chars
}

/// Without a table, glyph `n` draws character `n`, as for code page 437
/// fonts of the console.
fn identity_table(count: usize) -> Vec<Vec<char>> {
    (0..count)
        .map(|n| {
            u32::try_from(n)
                .ok()
                .and_then(char::from_u32)
                .into_iter()
                .collect()
        })
        .collect()
}

/// Line and block characters at their codes of code page 437. In VGA text
/// mode, codes 0xc0 to 0xdf repeat their last column into the ninth, so
/// horizontal lines join there.
const CP437: &[(char, u8)] = &[
    ('─', 0xc4),
    ('│', 0xb3),
    ('┌', 0xda),
    ('┐', 0xbf),
    ('└', 0xc0),
    ('┘', 0xd9),
    ('├', 0xc3),
    ('┤', 0xb4),
    ('┬', 0xc2),
    ('┴', 0xc1),
    ('┼', 0xc5),
    ('█', 0xdb),
    ('░', 0xb0),
    ('▒', 0xb1),
];

/// Look-alikes for characters a font may lack, best first.
pub const FALLBACKS: &[(char, &[char])] = &[
    ('▶', &['►', '→', '>']),
    ('►', &['▶', '>']),
    ('◀', &['◄', '<']),
    ('‹', &['<']),
    ('›', &['>']),
    ('←', &['<']),
    ('→', &['>']),
    ('↑', &['^']),
    ('↓', &['v']),
    ('“', &['"']),
    ('”', &['"']),
    ('„', &['"']),
    ('‘', &['\'']),
    ('’', &['\'']),
    ('–', &['-']),
    ('—', &['-']),
    ('•', &['·', '*']),
    ('…', &['.']),
    ('√', &['v']),
    ('×', &['x']),
    ('€', &['E']),
    ('\u{a0}', &[' ']),
    ('\u{fffd}', &['?']),
];

/// The look-alikes of `c`.
fn look_alikes(c: char) -> &'static [char] {
    FALLBACKS
        .iter()
        .find(|(of, _)| *of == c)
        .map_or(&[], |(_, look_alikes)| look_alikes)
}

/// The characters a subset keeps: ASCII, Latin-1 and the few letters
/// beyond it that Spanish, French, Dutch and Brazilian Portuguese use
/// (ROADMAP.md #18), the menu's symbols, and line and block characters.
/// About 230 characters, which share fewer glyphs.
pub fn repertoire() -> Vec<char> {
    let ascii = (0x20_u8..0x7f).map(char::from);
    let latin1 = (0xa0_u8..=0xff).map(char::from);
    let extra = "ŒœŸ€‹›‘’“”„–—•…√←↑→↓▶►\u{fffd}".chars();
    let lines = CP437.iter().map(|(c, _)| *c);
    ascii.chain(latin1).chain(extra).chain(lines).collect()
}

#[cfg(test)]
mod tests;
