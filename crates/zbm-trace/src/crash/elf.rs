//! The GNU build ID of an ELF binary.

use std::fmt::Write as _;

/// `PT_NOTE`: a program header of notes.
const PT_NOTE: u32 = 4;

/// `NT_GNU_BUILD_ID`: the type of the build ID note.
const NT_GNU_BUILD_ID: u32 = 3;

/// Reads integers of the file's byte order.
#[derive(Clone, Copy)]
struct Reader<'a> {
    /// The file.
    bytes: &'a [u8],
    /// Whether it is big-endian.
    big: bool,
}

impl Reader<'_> {
    /// The `N` bytes at `at`, in the file's order.
    fn bytes<const N: usize>(self, at: usize) -> Option<[u8; N]> {
        let slice = self.bytes.get(at..at.checked_add(N)?)?;
        let mut array: [u8; N] = slice.try_into().ok()?;
        if self.big {
            array.reverse();
        }
        Some(array)
    }

    /// The 16-bit integer at `at`.
    fn u16(self, at: usize) -> Option<usize> {
        Some(usize::from(u16::from_le_bytes(self.bytes(at)?)))
    }

    /// The 32-bit integer at `at`.
    fn u32(self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(at)?))
    }

    /// The address-sized integer at `at`: 32 or 64 bits.
    fn word(self, at: usize, wide: bool) -> Option<usize> {
        if wide {
            usize::try_from(u64::from_le_bytes(self.bytes(at)?)).ok()
        } else {
            usize::try_from(self.u32(at)?).ok()
        }
    }
}

/// The GNU build ID of the ELF file `elf`, in lower-case hexadecimal, or
/// `None` if it is not an ELF file or has none. Reads only the program
/// headers and their notes, and never panics, whatever the bytes.
///
/// ```
/// assert_eq!(zbm_trace::crash::build_id(b"not an ELF file"), None);
/// ```
pub fn build_id(elf: &[u8]) -> Option<String> {
    if elf.get(..4)? != b"\x7fELF" {
        return None;
    }
    let wide = *elf.get(4)? == 2;
    let reader = Reader {
        bytes: elf,
        big: *elf.get(5)? == 2,
    };
    let (phoff, phentsize, phnum) = if wide {
        (
            reader.word(0x20, true)?,
            reader.u16(0x36)?,
            reader.u16(0x38)?,
        )
    } else {
        (
            reader.word(0x1c, false)?,
            reader.u16(0x2a)?,
            reader.u16(0x2c)?,
        )
    };
    (0..phnum).find_map(|index| {
        let header = phoff.checked_add(index.checked_mul(phentsize)?)?;
        if reader.u32(header)? != PT_NOTE {
            return None;
        }
        let (offset, size) = if wide {
            (
                reader.word(header + 8, true)?,
                reader.word(header + 32, true)?,
            )
        } else {
            (
                reader.word(header + 4, false)?,
                reader.word(header + 16, false)?,
            )
        };
        note(reader, offset, offset.checked_add(size)?)
    })
}

/// The build ID among the notes between `start` and `end`.
fn note(reader: Reader<'_>, start: usize, end: usize) -> Option<String> {
    let align = |size: usize| size.checked_add(3).map(|size| size & !3);
    let mut at = start;
    while at.checked_add(12)? <= end {
        let name_size = usize::try_from(reader.u32(at)?).ok()?;
        let desc_size = usize::try_from(reader.u32(at + 4)?).ok()?;
        let kind = reader.u32(at + 8)?;
        let name_at = at + 12;
        let desc_at = name_at.checked_add(align(name_size)?)?;
        let name = reader.bytes.get(name_at..name_at.checked_add(name_size)?)?;
        if kind == NT_GNU_BUILD_ID && name == b"GNU\0" {
            let desc = reader.bytes.get(desc_at..desc_at.checked_add(desc_size)?)?;
            return Some(desc.iter().fold(String::new(), |mut hex, byte| {
                let _ = write!(hex, "{byte:02x}");
                hex
            }));
        }
        at = desc_at.checked_add(align(desc_size)?)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit little-endian ELF header with one `PT_NOTE` program header
    /// holding `notes`.
    fn elf64(notes: &[u8]) -> Vec<u8> {
        let mut file = vec![0_u8; 64 + 56];
        file[..6].copy_from_slice(b"\x7fELF\x02\x01");
        file[0x20..0x28].copy_from_slice(&64_u64.to_le_bytes());
        file[0x36..0x38].copy_from_slice(&56_u16.to_le_bytes());
        file[0x38..0x3a].copy_from_slice(&1_u16.to_le_bytes());
        let header = 64;
        file[header..header + 4].copy_from_slice(&PT_NOTE.to_le_bytes());
        let offset = file.len() as u64;
        file[header + 8..header + 16].copy_from_slice(&offset.to_le_bytes());
        file[header + 32..header + 40].copy_from_slice(&(notes.len() as u64).to_le_bytes());
        file.extend_from_slice(notes);
        file
    }

    /// A note of `kind` with `name` and `desc`, padded to four bytes.
    fn note(name: &[u8], kind: u32, desc: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(desc.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        bytes.extend_from_slice(desc);
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        bytes
    }

    #[test]
    fn the_build_id_note_is_found_after_other_notes() {
        let mut notes = note(
            b"GNU\0",
            1,
            &[0, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0],
        );
        notes.extend(note(
            b"GNU\0",
            NT_GNU_BUILD_ID,
            &[0xde, 0xad, 0xbe, 0xef, 0x01],
        ));

        assert_eq!(build_id(&elf64(&notes)).as_deref(), Some("deadbeef01"));
    }

    /// A 32-bit big-endian ELF header with one `PT_NOTE` program header
    /// holding a build ID note.
    fn elf32_big(id: &[u8]) -> Vec<u8> {
        let be32 = |value: usize| u32::try_from(value).unwrap().to_be_bytes();
        let mut notes = Vec::new();
        notes.extend_from_slice(&be32(4));
        notes.extend_from_slice(&be32(id.len()));
        notes.extend_from_slice(&NT_GNU_BUILD_ID.to_be_bytes());
        notes.extend_from_slice(b"GNU\0");
        notes.extend_from_slice(id);
        let mut file = vec![0_u8; 52 + 32];
        file[..6].copy_from_slice(b"\x7fELF\x01\x02");
        file[0x1c..0x20].copy_from_slice(&be32(52));
        file[0x2a..0x2c].copy_from_slice(&32_u16.to_be_bytes());
        file[0x2c..0x2e].copy_from_slice(&1_u16.to_be_bytes());
        file[52..56].copy_from_slice(&PT_NOTE.to_be_bytes());
        file[56..60].copy_from_slice(&be32(84));
        file[68..72].copy_from_slice(&be32(notes.len()));
        file.extend_from_slice(&notes);
        file
    }

    #[test]
    fn thirty_two_bit_big_endian_files_are_read_too() {
        assert_eq!(build_id(&elf32_big(&[0xab, 0xcd])).as_deref(), Some("abcd"));
    }

    #[test]
    fn files_without_a_build_id_have_none() {
        let other = note(b"Go\0\0", NT_GNU_BUILD_ID, b"abcd");

        assert_eq!(build_id(&elf64(&other)), None);
        assert_eq!(build_id(&elf64(&[])), None);
        assert_eq!(build_id(b"\x7fEL"), None);
        assert_eq!(build_id(&[]), None);
    }

    #[test]
    fn truncated_files_are_not_a_panic() {
        let file = elf64(&note(b"GNU\0", NT_GNU_BUILD_ID, &[1; 20]));

        for length in 0..file.len() {
            let _ = build_id(&file[..length]);
        }
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "the test reads its own binary, which only the real file can give"
    )]
    fn the_test_binary_has_a_build_id() {
        let exe = std::fs::read("/proc/self/exe").unwrap();

        let id = build_id(&exe).unwrap();

        assert!(
            id.len() >= 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{id}"
        );
    }
}
