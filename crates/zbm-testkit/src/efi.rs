//! Descriptions of EFI images that do not depend on the host.
//!
//! The parity tests of `generate-zbm` and `zbm-kcl` build EFI images from the
//! host's systemd stub. The stub differs between machines, so the bytes of an
//! image cannot be recorded. What the tools put into the image can: the
//! payload sections, their sizes and contents, and where each lies relative
//! to the first.

use std::fmt::Write as _;

/// The sections the image tools add to the stub.
pub const PAYLOAD_SECTIONS: &[&str] = &[
    ".osrel", ".cmdline", ".dtb", ".uname", ".splash", ".linux", ".initrd",
];

/// One section of a PE image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    /// The name, without trailing NUL bytes.
    pub name: String,
    /// The address the section is loaded at, relative to the image base.
    pub virtual_address: u32,
    /// The bytes of the section in the file.
    pub data: Vec<u8>,
}

/// The sections of the PE image `bytes`, or `None` when it is not one.
pub fn sections(bytes: &[u8]) -> Option<Vec<Section>> {
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
    };
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };
    if bytes.get(..2)? != b"MZ" {
        return None;
    }
    let pe = usize::try_from(u32_at(0x3c)?).ok()?;
    if bytes.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let count = usize::from(u16_at(coff + 2)?);
    let optional = usize::from(u16_at(coff + 16)?);
    let table = coff + 20 + optional;
    (0..count)
        .map(|index| {
            let entry = table + index * 40;
            let raw_name = bytes.get(entry..entry + 8)?;
            let name = String::from_utf8_lossy(raw_name)
                .trim_end_matches('\0')
                .to_owned();
            let virtual_size = usize::try_from(u32_at(entry + 8)?).ok()?;
            let virtual_address = u32_at(entry + 12)?;
            let raw_size = usize::try_from(u32_at(entry + 16)?).ok()?;
            let raw_start = usize::try_from(u32_at(entry + 20)?).ok()?;
            let len = raw_size.min(virtual_size.max(1));
            let data = bytes.get(raw_start..raw_start + len)?.to_vec();
            Some(Section {
                name,
                virtual_address,
                data,
            })
        })
        .collect()
}

/// A 64-bit FNV-1a hash, to name contents briefly.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The payload of the EFI image `bytes` as text: each section of
/// [`PAYLOAD_SECTIONS`] in the order of the image, with its offset from the
/// first payload section, its size and the hash of its contents; `.osrel`
/// and `.cmdline` are shown as text. `None` when `bytes` is not a PE image.
///
/// ```
/// assert_eq!(zbm_testkit::efi::describe(b"not an image"), None);
/// ```
pub fn describe(bytes: &[u8]) -> Option<String> {
    let sections = sections(bytes)?;
    let payload: Vec<&Section> = sections
        .iter()
        .filter(|section| PAYLOAD_SECTIONS.contains(&section.name.as_str()))
        .collect();
    let base = payload.first().map_or(0, |section| section.virtual_address);
    let mut text = String::from("EFI image:");
    for section in payload {
        let _ = write!(
            text,
            " {} at +0x{:x}, {} bytes, {:016x}",
            section.name,
            section.virtual_address - base,
            section.data.len(),
            fnv1a(&section.data)
        );
        if matches!(section.name.as_str(), ".osrel" | ".cmdline") {
            let shown = String::from_utf8_lossy(&section.data);
            let _ = write!(text, " {:?}", shown.trim_end_matches('\0'));
        }
        text.push(';');
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal PE image with the given sections at the given addresses.
    fn image(sections: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut bytes = vec![0_u8; 0x80];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40_u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        let count = u16::try_from(sections.len()).unwrap();
        bytes[0x46..0x48].copy_from_slice(&count.to_le_bytes());
        // No optional header: the table follows the COFF header at 0x58.
        bytes.resize(0x58 + sections.len() * 40, 0);
        let mut data_at = bytes.len();
        let mut data = Vec::new();
        for (index, (name, address, contents)) in sections.iter().enumerate() {
            let entry = 0x58 + index * 40;
            bytes[entry..entry + name.len()].copy_from_slice(name.as_bytes());
            let len = u32::try_from(contents.len()).unwrap();
            let at = u32::try_from(data_at).unwrap();
            bytes[entry + 8..entry + 12].copy_from_slice(&len.to_le_bytes());
            bytes[entry + 12..entry + 16].copy_from_slice(&address.to_le_bytes());
            bytes[entry + 16..entry + 20].copy_from_slice(&len.to_le_bytes());
            bytes[entry + 20..entry + 24].copy_from_slice(&at.to_le_bytes());
            data.extend_from_slice(contents);
            data_at += contents.len();
        }
        bytes.extend(data);
        bytes
    }

    #[test]
    fn the_payload_is_described_relative_to_the_first_payload_section() {
        let stub_a = image(&[
            (".text", 0x1000, b"stub a"),
            (".cmdline", 0x8000, b"quiet\0"),
            (".linux", 0x9000, b"LLLL"),
        ]);
        let stub_b = image(&[
            (".text", 0x1000, b"a longer stub"),
            (".sbat", 0x3000, b"sbat"),
            (".cmdline", 0xa000, b"quiet\0"),
            (".linux", 0xb000, b"LLLL"),
        ]);

        let described = describe(&stub_a).unwrap();

        assert_eq!(describe(&stub_b).unwrap(), described);
        assert!(described.starts_with("EFI image: .cmdline at +0x0, 6 bytes, "));
        assert!(described.contains(" \"quiet\"; .linux at +0x1000, 4 bytes, "));
    }

    #[test]
    fn other_files_are_not_images() {
        assert_eq!(describe(b""), None);
        assert_eq!(describe(b"MZ but short"), None);
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    }
}
