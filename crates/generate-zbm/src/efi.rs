//! Laying out a unified EFI bundle, as `createUEFIBundle` in `generate-zbm`
//! does: the stub's section alignment and the end of its last section, from
//! `objdump`, decide where each added section goes.

use crate::perl::split_whitespace;

/// The section alignment in a line of `objdump -p` output:
/// `m/SectionAlignment\s+(\d+)/`, whose decimal digits are then read as hex, as
/// the Perl program does with `hex($1)`. objdump prints the value in hex with
/// leading zeros, so the digits are hex digits in practice.
pub fn section_alignment(line: &[u8]) -> Option<u64> {
    const KEY: &[u8] = b"SectionAlignment";
    (0..line.len()).find_map(|at| {
        let rest = line[at..].strip_prefix(KEY)?;
        let spaces = rest
            .iter()
            .take_while(|byte| crate::perl::is_space(**byte))
            .count();
        let digits: Vec<u8> = rest[spaces..]
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .collect();
        if spaces == 0 || digits.is_empty() {
            return None;
        }
        u64::from_str_radix(std::str::from_utf8(&digits).ok()?, 16).ok()
    })
}

/// The end of the stub's last section, from the last line of `objdump -w -h`:
/// its size plus its VMA, the fourth and fifth fields of `split /\s+/` (the
/// line starts with white space, so the first field is empty).
pub fn last_section_end(last_line: &[u8]) -> Option<u128> {
    let fields = split_whitespace(last_line);
    let hex = |field: &[u8]| -> Option<u128> {
        if field.is_empty() || !field.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        u128::from_str_radix(std::str::from_utf8(field).ok()?, 16).ok()
    };
    let size = hex(fields.get(3)?)?;
    let vma = hex(fields.get(4)?)?;
    size.checked_add(vma)
}

/// `increaseBundleOffset`: the offset after a section of `size` bytes, rounded
/// up to the alignment.
pub fn next_offset(size: u128, offset: u128, alignment: u128) -> u128 {
    offset + size.div_ceil(alignment) * alignment
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::exec::{Command, Runner, SystemRunner};
    use zbm_testkit::have_program;

    const STUB: &str = "/usr/lib/systemd/boot/efi/linuxx64.efi.stub";

    #[test]
    fn parsing() {
        assert_eq!(
            section_alignment(b"SectionAlignment\t00001000\n"),
            Some(4096)
        );
        assert_eq!(section_alignment(b"SectionAlignment00001000"), None);
        assert_eq!(section_alignment(b"FileAlignment\t00000200"), None);
        assert_eq!(
            last_section_end(b"  9 .sbat 00000075  000000000001d000  0000000000 CONTENTS\n"),
            Some(0x1d075)
        );
        assert_eq!(last_section_end(b"Idx Name Size VMA"), None);
        assert_eq!(next_offset(0x1d075, 0, 0x1000), 0x1e000);
        assert_eq!(next_offset(0x1000, 0x1e000, 0x1000), 0x1f000);
        assert_eq!(next_offset(0, 0x1e000, 0x1000), 0x1e000);
    }

    #[test]
    fn reads_the_systemd_stub() {
        if !std::path::Path::new(STUB).is_file() || !have_program("objdump") {
            eprintln!("skipped: the systemd EFI stub or objdump is missing");
            return;
        }
        let private = SystemRunner
            .run(&Command::new("objdump").args(["-p", STUB]))
            .unwrap()
            .stdout;
        let alignment = private
            .split(|byte| *byte == b'\n')
            .find_map(section_alignment);
        assert_eq!(alignment, Some(4096));
        let headers = SystemRunner
            .run(&Command::new("objdump").args(["-w", "-h", STUB]))
            .unwrap()
            .stdout;
        let last = headers
            .split_inclusive(|byte| *byte == b'\n')
            .next_back()
            .unwrap();
        assert!(last_section_end(last).is_some_and(|end| end > 0));
    }
}
