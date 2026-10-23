//! A GUID partition table for any logical sector size: `sfdisk` before
//! util-linux 2.40 and `sgdisk` cannot write one for 4 KiB sectors into a
//! file.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// The partition type of an EFI system partition.
pub const ESP: &str = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B";

/// The partition type of Linux data.
pub const LINUX: &str = "0FC63DAF-8483-4772-8E79-3D69D8477DE4";

/// The number of partition entries.
const ENTRIES: u64 = 128;

/// The size of a partition entry.
const ENTRY_SIZE: u64 = 128;

/// One partition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Partition<'a> {
    /// Its name.
    pub name: &'a str,
    /// Its type GUID, as text.
    pub type_guid: &'a str,
    /// Its first sector.
    pub first: u64,
    /// Its number of sectors; `None`: up to the last usable sector.
    pub sectors: Option<u64>,
}

/// A GUID as text, in the mixed-endian layout of the disk.
fn guid_bytes(text: &str) -> io::Result<[u8; 16]> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, format!("bad GUID {text}"));
    if hex.len() != 32 {
        return Err(invalid());
    }
    let mut bytes = [0_u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let pair = hex.get(index * 2..index * 2 + 2).ok_or_else(invalid)?;
        *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
    }
    bytes[..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Ok(bytes)
}

/// A random GUID (version 4), in the layout of the disk.
fn random_guid() -> io::Result<[u8; 16]> {
    let mut bytes = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[7] = (bytes[7] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(bytes)
}

/// The CRC-32 of the GPT (IEEE 802.3, reflected).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// The layout of a disk's table.
struct Layout {
    sector: u64,
    last: u64,
    entry_sectors: u64,
    first_usable: u64,
    last_usable: u64,
}

impl Layout {
    fn new(size: u64, sector: u64) -> Layout {
        let last = size / sector - 1;
        let entry_sectors = ENTRIES * ENTRY_SIZE / sector;
        Layout {
            sector,
            last,
            entry_sectors,
            first_usable: 2 + entry_sectors,
            last_usable: last - entry_sectors - 1,
        }
    }
}

/// The partition entries.
fn entries(layout: &Layout, parts: &[Partition<'_>]) -> io::Result<Vec<u8>> {
    let mut entries = vec![0_u8; usize::try_from(ENTRIES * ENTRY_SIZE).unwrap_or_default()];
    for (part, entry) in parts.iter().zip(entries.chunks_mut(128)) {
        let end = part
            .sectors
            .map_or(layout.last_usable, |sectors| part.first + sectors - 1);
        entry[..16].copy_from_slice(&guid_bytes(part.type_guid)?);
        entry[16..32].copy_from_slice(&random_guid()?);
        entry[32..40].copy_from_slice(&part.first.to_le_bytes());
        entry[40..48].copy_from_slice(&end.to_le_bytes());
        let name: Vec<u8> = part
            .name
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let length = name.len().min(72);
        entry[56..56 + length].copy_from_slice(&name[..length]);
    }
    Ok(entries)
}

/// A table header, padded to a sector.
fn header(layout: &Layout, disk: &[u8; 16], at: (u64, u64, u64), entries_crc: u32) -> Vec<u8> {
    let (current, other, entries_lba) = at;
    let mut header = Vec::with_capacity(92);
    header.extend_from_slice(b"EFI PART");
    header.extend_from_slice(&0x0001_0000_u32.to_le_bytes());
    header.extend_from_slice(&92_u32.to_le_bytes());
    header.extend_from_slice(&[0; 8]);
    for lba in [current, other, layout.first_usable, layout.last_usable] {
        header.extend_from_slice(&lba.to_le_bytes());
    }
    header.extend_from_slice(disk);
    header.extend_from_slice(&entries_lba.to_le_bytes());
    header.extend_from_slice(&u32::try_from(ENTRIES).unwrap_or_default().to_le_bytes());
    header.extend_from_slice(&u32::try_from(ENTRY_SIZE).unwrap_or_default().to_le_bytes());
    header.extend_from_slice(&entries_crc.to_le_bytes());
    let crc = crc32(&header);
    header[16..20].copy_from_slice(&crc.to_le_bytes());
    header.resize(usize::try_from(layout.sector).unwrap_or_default(), 0);
    header
}

/// The protective MBR.
fn protective_mbr(layout: &Layout) -> Vec<u8> {
    let mut mbr = vec![0_u8; usize::try_from(layout.sector).unwrap_or_default()];
    mbr[446..454].copy_from_slice(&[0, 0, 2, 0, 0xee, 0xff, 0xff, 0xff]);
    mbr[454..458].copy_from_slice(&1_u32.to_le_bytes());
    let sectors = u32::try_from(layout.last).unwrap_or(u32::MAX);
    mbr[458..462].copy_from_slice(&sectors.to_le_bytes());
    mbr[510..512].copy_from_slice(&[0x55, 0xaa]);
    mbr
}

/// Writes a sparse disk image of `size` bytes at `path` with a GPT of
/// `sector`-byte sectors holding `parts`.
///
/// # Errors
///
/// When the file cannot be written, or a type GUID is not valid.
pub fn write(path: &Path, size: u64, sector: u64, parts: &[Partition<'_>]) -> io::Result<()> {
    let layout = Layout::new(size, sector);
    let entries = entries(&layout, parts)?;
    let entries_crc = crc32(&entries);
    let disk = random_guid()?;
    let backup_entries = layout.last - layout.entry_sectors;
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.set_len(size)?;
    file.write_all(&protective_mbr(&layout))?;
    file.write_all(&header(&layout, &disk, (1, layout.last, 2), entries_crc))?;
    file.write_all(&entries)?;
    file.seek(SeekFrom::Start(backup_entries * sector))?;
    file.write_all(&entries)?;
    let backup = (layout.last, 1, backup_entries);
    file.write_all(&header(&layout, &disk, backup, entries_crc))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_crc_is_that_of_ieee_802_3() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn guids_are_stored_mixed_endian() {
        let bytes = guid_bytes(ESP).unwrap();

        assert_eq!(bytes[..8], [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11]);
        assert_eq!(bytes[8..], [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B]);
        assert!(guid_bytes("C12A7328").is_err());
    }

    #[test]
    fn a_table_with_4k_sectors_has_both_headers_and_its_entries() {
        let path = std::env::temp_dir().join(format!("zbm-vmtest-gpt-{}", std::process::id()));
        let parts = [
            Partition {
                name: "ESP",
                type_guid: ESP,
                first: 256,
                sectors: Some(1024),
            },
            Partition {
                name: "zroot-luks",
                type_guid: LINUX,
                first: 1280,
                sectors: None,
            },
        ];

        write(&path, 16 << 20, 4096, &parts).unwrap();

        let disk = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(&disk[510..512], &[0x55, 0xaa]);
        assert_eq!(&disk[4096..4104], b"EFI PART");
        let last = (16_usize << 20) - 4096;
        assert_eq!(&disk[last..last + 8], b"EFI PART");
        let header = &disk[4096..4096 + 92];
        let mut zeroed = header.to_vec();
        zeroed[16..20].fill(0);
        assert_eq!(header[16..20], crc32(&zeroed).to_le_bytes());
        let second = &disk[8192 + 128..8192 + 256];
        assert_eq!(second[32..40], 1280_u64.to_le_bytes());
        let last_usable = (16_u64 << 20) / 4096 - 1 - 4 - 1;
        assert_eq!(second[40..48], last_usable.to_le_bytes());
        assert_eq!(&second[56..60], &[b'z', 0, b'r', 0]);
    }

    #[test]
    fn a_table_with_512_byte_sectors_passes_sgdisk_verification() {
        if !crate::settings::program_exists("sgdisk") {
            return;
        }
        let path = std::env::temp_dir().join(format!("zbm-vmtest-gpt512-{}", std::process::id()));
        let parts = [
            Partition {
                name: "esp",
                type_guid: ESP,
                first: 2048,
                sectors: Some(2048),
            },
            Partition {
                name: "zfs",
                type_guid: LINUX,
                first: 4096,
                sectors: None,
            },
        ];

        write(&path, 8 << 20, 512, &parts).unwrap();

        let output = std::process::Command::new("sgdisk")
            .arg("-v")
            .arg(&path)
            .output()
            .unwrap();
        let printed = std::process::Command::new("sgdisk")
            .arg("-p")
            .arg(&path)
            .output()
            .unwrap();
        std::fs::remove_file(&path).ok();
        let verified = String::from_utf8_lossy(&output.stdout);
        assert!(verified.contains("No problems found"), "{verified}");
        let printed = String::from_utf8_lossy(&printed.stdout);
        assert!(
            printed.contains("EF00  esp") && printed.contains("8300  zfs"),
            "{printed}"
        );
    }
}
