//! The EFI images `zbm_host::pe` writes, checked against those `objcopy`
//! writes from the same stub and files (ADR 0014): the same section table
//! and size in memory, and the header fields that differ only as listed.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "the test runs objcopy on files of its own and fails loudly when it cannot"
)]

use object::read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile64};
use zbm_host::pe::{Image, WriteOptions, checksum};
use zbm_platform::exec::{Command, Runner, Stream, SystemRunner};
use zbm_testkit::{TempDir, can_run, have_program};

const STUB: &str = "/usr/lib/systemd/boot/efi/linuxx64.efi.stub";

/// The header fields the firmware and the stub read.
#[derive(Debug, PartialEq)]
struct Fields {
    entry: u32,
    image_base: u64,
    section_alignment: u32,
    file_alignment: u32,
    size_of_image: u32,
    subsystem: u16,
    dll_characteristics: u16,
    machine: u16,
}

fn fields(bytes: &[u8]) -> Fields {
    let file = PeFile64::parse(bytes).unwrap();
    let optional = file.nt_headers().optional_header();
    Fields {
        entry: optional.address_of_entry_point(),
        image_base: optional.image_base(),
        section_alignment: optional.section_alignment(),
        file_alignment: optional.file_alignment(),
        size_of_image: optional.size_of_image(),
        subsystem: optional.subsystem(),
        dll_characteristics: optional.dll_characteristics(),
        machine: file
            .nt_headers()
            .file_header()
            .machine
            .get(object::LittleEndian),
    }
}

/// The name, address, size in memory and contents of each section.
fn table(bytes: &[u8]) -> Vec<(String, u32, u32, Vec<u8>)> {
    Image::read(bytes.to_vec())
        .unwrap()
        .sections()
        .iter()
        .map(|section| {
            (
                section.name().to_owned(),
                section.virtual_address(),
                section.virtual_size(),
                section.data().to_vec(),
            )
        })
        .collect()
}

#[test]
fn the_bundle_has_the_sections_objcopy_gives_it() {
    let tools = std::path::Path::new(STUB).is_file() && have_program("objcopy");
    if !can_run(tools, "the systemd EFI stub or objcopy") {
        return;
    }
    let dir = TempDir::new("zbm-pe-objcopy");
    let files: Vec<(&str, Vec<u8>)> = vec![
        (".osrel", b"NAME=\"Test\"\nID=test\n".to_vec()),
        (".cmdline", b"quiet loglevel=4\0".to_vec()),
        (
            ".initrd",
            (0..200_000_u32).map(|n| (n % 251) as u8).collect(),
        ),
        (
            ".linux",
            (0..300_001_u32).map(|n| (n % 241) as u8).collect(),
        ),
    ];
    let stub = std::fs::read(STUB).unwrap();
    let mut image = Image::read(stub.clone()).unwrap();
    for (name, data) in &files {
        image.add(name, data).unwrap();
    }
    let ours = image.write(&WriteOptions::default()).unwrap().bytes;

    let base = fields(&stub).image_base;
    let mut command = Command::new("objcopy");
    for (name, data) in &files {
        let path = dir.write(format!("{name}.bin"), data);
        let address = Image::read(ours.clone())
            .unwrap()
            .section(name)
            .unwrap()
            .virtual_address();
        command = command
            .arg("--add-section")
            .arg(format!("{name}={}", path.display()))
            .arg("--change-section-vma")
            .arg(format!("{name}=0x{:X}", base + u64::from(address)));
    }
    let theirs_path = dir.join("objcopy.efi");
    let output = SystemRunner
        .run(
            &command
                .arg(STUB)
                .arg(&theirs_path)
                .stderr(Stream::Capture)
                .env("SOURCE_DATE_EPOCH", "0"),
        )
        .unwrap();
    assert!(output.status.is_success(), "{}", output.stderr_text());
    let theirs = std::fs::read(&theirs_path).unwrap();

    assert_eq!(table(&ours), table(&theirs));
    assert_eq!(fields(&ours), fields(&theirs));
    // The checksum is the one objcopy computes.
    let checksum_at = |bytes: &[u8]| -> usize {
        let nt = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
        nt + 24 + 64
    };
    let stored = |bytes: &[u8]| -> u32 {
        let at = checksum_at(bytes);
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    };
    assert_eq!(checksum(&theirs, checksum_at(&theirs)), stored(&theirs));
    assert_eq!(checksum(&ours, checksum_at(&ours)), stored(&ours));
    // What differs: objcopy moves the PE header behind binutils' DOS
    // program; the stub's headers are kept as they are.
    assert_eq!(ours[..0x40], stub[..0x40]);
    assert_ne!(theirs[..0x40], stub[..0x40]);
}
