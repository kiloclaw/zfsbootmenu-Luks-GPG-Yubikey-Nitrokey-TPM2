//! The two facts `xtask` needs from an ELF file: its GNU build ID, and how
//! offsets from its load address map to the addresses it was linked for.
//!
//! The runtime reads build IDs with `zbm_trace::crash::build_id`; developer
//! tools depend on no crate of the workspace, so this is a copy for the host.

use std::fmt::Write as _;

/// `PT_LOAD`.
const PT_LOAD: u32 = 1;
/// `PT_NOTE`.
const PT_NOTE: u32 = 4;
/// `NT_GNU_BUILD_ID`.
const NT_GNU_BUILD_ID: u32 = 3;

/// One program header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Segment {
    /// `p_type`.
    kind: u32,
    /// `p_offset`.
    offset: u64,
    /// `p_vaddr`.
    vaddr: u64,
    /// `p_filesz`.
    size: u64,
}

/// The program headers of `elf`, or `None` if it is not an ELF file.
fn segments(elf: &[u8]) -> Option<Vec<Segment>> {
    if elf.get(..4)? != b"\x7fELF" {
        return None;
    }
    let wide = *elf.get(4)? == 2;
    let big = *elf.get(5)? == 2;
    let int = |at: usize, size: usize| -> Option<u64> {
        let bytes = elf.get(at..at.checked_add(size)?)?;
        let mut value = 0_u64;
        for (index, byte) in bytes.iter().enumerate() {
            let shift = if big { size - 1 - index } else { index };
            value |= u64::from(*byte) << (8 * shift);
        }
        Some(value)
    };
    let to = |value: u64| usize::try_from(value).ok();
    let (phoff, phentsize, phnum) = if wide {
        (to(int(0x20, 8)?)?, to(int(0x36, 2)?)?, to(int(0x38, 2)?)?)
    } else {
        (to(int(0x1c, 4)?)?, to(int(0x2a, 2)?)?, to(int(0x2c, 2)?)?)
    };
    (0..phnum)
        .map(|index| {
            let at = phoff.checked_add(index.checked_mul(phentsize)?)?;
            let kind = u32::try_from(int(at, 4)?).ok()?;
            Some(if wide {
                Segment {
                    kind,
                    offset: int(at + 8, 8)?,
                    vaddr: int(at + 16, 8)?,
                    size: int(at + 32, 8)?,
                }
            } else {
                Segment {
                    kind,
                    offset: int(at + 4, 4)?,
                    vaddr: int(at + 8, 4)?,
                    size: int(at + 16, 4)?,
                }
            })
        })
        .collect()
}

/// Reads a 32-bit integer of the file's byte order.
fn u32_at(elf: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = elf.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(if elf.get(5) == Some(&2) {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    })
}

/// The GNU build ID of `elf` in lower-case hexadecimal.
pub fn build_id(elf: &[u8]) -> Option<String> {
    segments(elf)?
        .into_iter()
        .filter(|segment| segment.kind == PT_NOTE)
        .find_map(|segment| {
            let mut at = usize::try_from(segment.offset).ok()?;
            let end = at.checked_add(usize::try_from(segment.size).ok()?)?;
            while at.checked_add(12)? <= end {
                let name_size = usize::try_from(u32_at(elf, at)?).ok()?;
                let desc_size = usize::try_from(u32_at(elf, at + 4)?).ok()?;
                let kind = u32_at(elf, at + 8)?;
                let name_at = at + 12;
                let desc_at = name_at.checked_add(name_size.next_multiple_of(4))?;
                let name = elf.get(name_at..name_at.checked_add(name_size)?)?;
                if kind == NT_GNU_BUILD_ID && name == b"GNU\0" {
                    let desc = elf.get(desc_at..desc_at.checked_add(desc_size)?)?;
                    return Some(desc.iter().fold(String::new(), |mut hex, byte| {
                        let _ = write!(hex, "{byte:02x}");
                        hex
                    }));
                }
                at = desc_at.checked_add(desc_size.next_multiple_of(4))?;
            }
            None
        })
}

/// What to add to an offset from the load address to get the address the
/// file was linked for: the first loaded segment's address less its file
/// offset (0 for most position-independent executables).
pub fn load_bias(elf: &[u8]) -> Option<u64> {
    let first = segments(elf)?
        .into_iter()
        .filter(|segment| segment.kind == PT_LOAD)
        .min_by_key(|segment| segment.vaddr)?;
    Some(first.vaddr.wrapping_sub(first.offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_test_binary_has_a_build_id_and_a_bias() {
        let exe = std::fs::read("/proc/self/exe").unwrap();

        let id = build_id(&exe).unwrap();

        assert!(id.len() >= 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(load_bias(&exe).is_some());
    }

    #[test]
    fn other_files_have_neither() {
        assert_eq!(build_id(b"#!/bin/sh\n"), None);
        assert_eq!(load_bias(b""), None);
        assert_eq!(build_id(b"\x7fELF\x02\x01"), None);
    }
}
