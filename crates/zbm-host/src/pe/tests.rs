//! Tests of PE images, on small images built here.

use pretty_assertions::assert_eq;
use proptest::prelude::*;

use super::write::checksum;
use super::*;

const FILE_ALIGNMENT: u32 = 0x200;
const SECTION_ALIGNMENT: u32 = 0x1000;

/// A stub to build.
#[derive(Clone)]
struct Stub {
    is_64: bool,
    subsystem: u16,
    size_of_headers: u32,
    section_alignment: u32,
    sections: Vec<(&'static str, Vec<u8>)>,
    signed: bool,
    debug: bool,
}

impl Stub {
    fn new() -> Stub {
        Stub {
            is_64: true,
            subsystem: 10,
            size_of_headers: 0x400,
            section_alignment: SECTION_ALIGNMENT,
            sections: vec![(".text", vec![0xc3; 0x300]), (".data", b"data".to_vec())],
            signed: false,
            debug: false,
        }
    }

    fn optional_size(&self) -> usize {
        if self.is_64 { 240 } else { 224 }
    }

    fn table(&self) -> usize {
        0x40 + 24 + self.optional_size()
    }

    /// The image: headers, then each section at the next address and file
    /// offset; a debug directory at the start of the last section when
    /// asked, pointing at its own data; a certificate table at the end.
    fn build(&self) -> Vec<u8> {
        let mut bytes = vec![0_u8; self.size_of_headers as usize];
        put(&mut bytes, 0, b"MZ");
        put(&mut bytes, 0x3c, &0x40_u32.to_le_bytes());
        put(&mut bytes, 0x40, b"PE\0\0");
        let machine: u16 = if self.is_64 { 0x8664 } else { 0x14c };
        put(&mut bytes, 0x44, &machine.to_le_bytes());
        let count = u16::try_from(self.sections.len()).unwrap();
        put(&mut bytes, 0x46, &count.to_le_bytes());
        put(&mut bytes, 0x48, &0x1234_5678_u32.to_le_bytes());
        let optional_size = u16::try_from(self.optional_size()).unwrap();
        put(&mut bytes, 0x54, &optional_size.to_le_bytes());
        put(&mut bytes, 0x56, &0x22e_u16.to_le_bytes());
        let optional = 0x58;
        let magic: u16 = if self.is_64 { 0x20b } else { 0x10b };
        put(&mut bytes, optional, &magic.to_le_bytes());
        put(&mut bytes, optional + 8, &0x100_u32.to_le_bytes());
        put(
            &mut bytes,
            optional + 32,
            &self.section_alignment.to_le_bytes(),
        );
        put(&mut bytes, optional + 36, &FILE_ALIGNMENT.to_le_bytes());
        put(
            &mut bytes,
            optional + 60,
            &self.size_of_headers.to_le_bytes(),
        );
        put(&mut bytes, optional + 68, &self.subsystem.to_le_bytes());
        let directories = optional + if self.is_64 { 112 } else { 96 };
        put(&mut bytes, directories - 4, &16_u32.to_le_bytes());
        let mut address = u64::from(self.section_alignment);
        for (index, (name, data)) in self.sections.iter().enumerate() {
            let pointer = bytes.len();
            let raw = align(data.len(), FILE_ALIGNMENT);
            bytes.extend_from_slice(data);
            bytes.resize(pointer + raw, 0);
            let header = self.table() + 40 * index;
            put(&mut bytes, header, name.as_bytes());
            put(
                &mut bytes,
                header + 8,
                &u32::try_from(data.len()).unwrap().to_le_bytes(),
            );
            put(
                &mut bytes,
                header + 12,
                &u32::try_from(address).unwrap().to_le_bytes(),
            );
            put(
                &mut bytes,
                header + 16,
                &u32::try_from(raw).unwrap().to_le_bytes(),
            );
            put(
                &mut bytes,
                header + 20,
                &u32::try_from(pointer).unwrap().to_le_bytes(),
            );
            put(&mut bytes, header + 36, &DATA.to_le_bytes());
            if self.debug && index == self.sections.len() - 1 {
                // The entry's data is the section's bytes after the entry.
                put(
                    &mut bytes,
                    pointer + 24,
                    &u32::try_from(pointer + 28).unwrap().to_le_bytes(),
                );
                put(
                    &mut bytes,
                    directories + 48,
                    &u32::try_from(address).unwrap().to_le_bytes(),
                );
                put(&mut bytes, directories + 52, &28_u32.to_le_bytes());
            }
            address = align_u64(address + data.len() as u64, self.section_alignment);
        }
        put(
            &mut bytes,
            optional + 56,
            &u32::try_from(address).unwrap().to_le_bytes(),
        );
        if self.signed {
            let start = u32::try_from(bytes.len()).unwrap();
            bytes.extend_from_slice(&[0xee; 64]);
            put(&mut bytes, directories + 32, &start.to_le_bytes());
            put(&mut bytes, directories + 36, &64_u32.to_le_bytes());
        }
        bytes
    }
}

fn put(bytes: &mut [u8], at: usize, value: &[u8]) {
    bytes[at..at + value.len()].copy_from_slice(value);
}

fn align(value: usize, alignment: u32) -> usize {
    value.div_ceil(alignment as usize) * alignment as usize
}

fn align_u64(value: u64, alignment: u32) -> u64 {
    value.div_ceil(u64::from(alignment)) * u64::from(alignment)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// The names, addresses and contents of the sections of `bytes`.
fn layout(bytes: &[u8]) -> Vec<(String, u32, Vec<u8>)> {
    let image = Image::read(bytes.to_vec()).unwrap();
    image
        .sections()
        .iter()
        .map(|section| {
            (
                section.name().to_owned(),
                section.virtual_address(),
                section.data().to_vec(),
            )
        })
        .collect()
}

fn write(image: &Image) -> Written {
    image.write(&WriteOptions::default()).unwrap()
}

#[test]
fn a_stub_reads_as_its_sections() {
    for is_64 in [true, false] {
        let bytes = Stub {
            is_64,
            ..Stub::new()
        }
        .build();
        assert_eq!(
            layout(&bytes),
            [
                (".text".to_owned(), 0x1000, vec![0xc3; 0x300]),
                (".data".to_owned(), 0x2000, b"data".to_vec()),
            ]
        );
    }
}

#[test]
fn sections_added_follow_the_stub_at_the_alignment() {
    for is_64 in [true, false] {
        let stub = Stub {
            is_64,
            ..Stub::new()
        }
        .build();
        let mut image = Image::read(stub.clone()).unwrap();
        image.add(".osrel", &[b'o'; 0x190]).unwrap();
        image.add(".cmdline", b"quiet\0").unwrap();
        image.add(".linux", &[b'L'; 0x1801]).unwrap();
        let written = image.write(&WriteOptions { timestamp: Some(7) }).unwrap();
        let bytes = written.bytes;

        let sections = layout(&bytes);
        let names: Vec<(&str, u32)> = sections
            .iter()
            .map(|(name, address, _)| (name.as_str(), *address))
            .collect();
        assert_eq!(
            names,
            [
                (".text", 0x1000),
                (".data", 0x2000),
                (".osrel", 0x3000),
                (".cmdline", 0x4000),
                (".linux", 0x5000)
            ]
        );
        assert_eq!(sections[4].2, vec![b'L'; 0x1801]);
        // The stub's own bytes are where they were.
        assert_eq!(bytes[0x400..0x700], stub[0x400..0x700]);
        assert_eq!(bytes.len() % FILE_ALIGNMENT as usize, 0);
        let optional = 0x58;
        assert_eq!(u32_at(&bytes, 0x48), 7, "the time stamp");
        assert_eq!(u32_at(&bytes, optional + 56), 0x7000, "SizeOfImage");
        assert_eq!(u32_at(&bytes, optional + 60), 0x400, "SizeOfHeaders");
        assert_eq!(
            u32_at(&bytes, optional + 8),
            0x100 + 0x200 + 0x200 + 0x1a00,
            "SizeOfInitializedData"
        );
        assert_eq!(
            u32_at(&bytes, optional + 64),
            checksum(&bytes, optional + 64)
        );
        assert!(written.warnings.is_empty());
    }
}

#[test]
fn the_time_stamp_is_the_stubs_unless_given() {
    let mut image = Image::read(Stub::new().build()).unwrap();
    image.add(".cmdline", b"x\0").unwrap();
    assert_eq!(u32_at(&write(&image).bytes, 0x48), 0x1234_5678);
}

#[test]
fn full_headers_grow_and_the_sections_move_down_in_the_file() {
    // Room for exactly the two sections the stub has.
    let size_of_headers = u32::try_from(align(Stub::new().table() + 80, FILE_ALIGNMENT)).unwrap();
    let mut stub = Stub {
        size_of_headers,
        debug: true,
        ..Stub::new()
    };
    let room = (size_of_headers as usize - (stub.table() + 80)) / 40;
    stub.sections
        .extend((0..room).map(|_| (".pad", vec![1; 64])));
    let bytes = stub.build();
    let mut image = Image::read(bytes.clone()).unwrap();
    image.add(".cmdline", b"quiet\0").unwrap();

    let written = write(&image).bytes;

    let optional = 0x58;
    assert_eq!(
        u32_at(&written, optional + 60),
        size_of_headers + FILE_ALIGNMENT
    );
    let before = layout(&bytes);
    let after = layout(&written);
    let addresses = |sections: &[(String, u32, Vec<u8>)]| -> Vec<(String, u32)> {
        sections
            .iter()
            .map(|(name, address, _)| (name.clone(), *address))
            .collect()
    };
    assert_eq!(
        addresses(&after[..before.len()]),
        addresses(&before),
        "addresses stay"
    );
    // The data stays, but for the debug entry's pointer in the last one.
    let last = before.len() - 1;
    assert_eq!(after[..last], before[..last]);
    assert_eq!(after.last().unwrap().0, ".cmdline");
    // The debug directory is in the last section of the stub; its pointer
    // moved with the data.
    let debug_section = Image::read(written.clone()).unwrap().sections()[before.len() - 1].clone();
    let table = stub.table() + 40 * (before.len() - 1);
    let pointer = u32_at(&written, table + 20) as usize;
    assert_eq!(u32_at(&written, pointer + 24) as usize, pointer + 28);
    assert_eq!(debug_section.name(), ".pad");
}

#[test]
fn headers_that_cannot_grow_are_refused() {
    let size_of_headers = u32::try_from(align(Stub::new().table() + 80, FILE_ALIGNMENT)).unwrap();
    let mut stub = Stub {
        size_of_headers,
        section_alignment: FILE_ALIGNMENT,
        ..Stub::new()
    };
    let room = (size_of_headers as usize - (stub.table() + 80)) / 40;
    stub.sections
        .extend((0..room).map(|_| (".pad", vec![1; 64])));
    // The first section is in memory where the headers end.
    let bytes = stub.build();
    let first = Image::read(bytes.clone()).unwrap().sections()[0].virtual_address();
    assert!(first < size_of_headers + FILE_ALIGNMENT, "{first:x}");
    let mut image = Image::read(bytes).unwrap();
    image.add(".cmdline", b"x").unwrap();
    assert_eq!(image.write(&WriteOptions::default()), Err(PeError::NoRoom));
}

#[test]
fn data_after_the_section_table_is_not_overwritten() {
    let stub = Stub::new();
    let mut bytes = stub.build();
    bytes[stub.table() + 80 + 4] = 0x55;
    let mut image = Image::read(bytes).unwrap();
    image.add(".cmdline", b"x").unwrap();
    assert_eq!(
        image.write(&WriteOptions::default()),
        Err(PeError::HeaderData)
    );
}

#[test]
fn a_signature_is_dropped_with_a_warning() {
    let bytes = Stub {
        signed: true,
        ..Stub::new()
    }
    .build();
    let mut image = Image::read(bytes).unwrap();
    image.add(".cmdline", b"x\0").unwrap();

    let written = write(&image);

    let directories = 0x58 + 112;
    assert_eq!(u32_at(&written.bytes, directories + 32), 0);
    assert_eq!(u32_at(&written.bytes, directories + 36), 0);
    assert!(!written.bytes.windows(64).any(|window| window == [0xee; 64]));
    assert_eq!(written.warnings.len(), 1, "{:?}", written.warnings);
}

#[test]
fn what_is_not_an_efi_image_is_refused() {
    assert!(matches!(
        Image::read(b"not an image".to_vec()),
        Err(PeError::Malformed(_))
    ));
    let bytes = Stub {
        subsystem: 3,
        ..Stub::new()
    }
    .build();
    assert_eq!(Image::read(bytes), Err(PeError::NotEfi(3)));

    let stub = Stub::new();
    let mut outside = stub.build();
    outside.truncate(0x900);
    assert_eq!(
        Image::read(outside),
        Err(PeError::OutsideFile(".data".to_owned()))
    );

    let mut overlapping = stub.build();
    // .data at .text's address.
    put(
        &mut overlapping,
        stub.table() + 40 + 12,
        &0x1000_u32.to_le_bytes(),
    );
    assert_eq!(
        Image::read(overlapping),
        Err(PeError::Overlap(".data".to_owned(), ".text".to_owned()))
    );

    let mut long = stub.build();
    put(&mut long, stub.table() + 40, b"/4\0\0\0\0\0\0");
    assert_eq!(Image::read(long), Err(PeError::LongName("/4".to_owned())));

    let mut aligned = stub.build();
    put(&mut aligned, 0x58 + 36, &0x300_u32.to_le_bytes());
    assert!(matches!(
        Image::read(aligned),
        Err(PeError::Alignment { .. })
    ));

    let mut image = Image::read(stub.build()).unwrap();
    assert_eq!(
        image.add(".toolongname", b"x"),
        Err(PeError::LongName(".toolongname".to_owned()))
    );
}

/// A bundle: the stub with `.cmdline`, `.initrd` and `.linux`.
fn bundle(cmdline: &[u8]) -> Image {
    let mut image = Image::read(Stub::new().build()).unwrap();
    image.add(".cmdline", cmdline).unwrap();
    image.add(".initrd", &[b'I'; 0x1200]).unwrap();
    image.add(".linux", &[b'L'; 0x800]).unwrap();
    Image::read(write(&image).bytes).unwrap()
}

#[test]
fn a_section_that_fits_is_replaced_in_place() {
    let mut image = bundle(b"quiet\0");
    let before = layout(&write(&image).bytes);

    assert!(image.replace(".cmdline", b"loglevel=7 rw\0").unwrap());

    let after = layout(&write(&image).bytes);
    assert_eq!(
        after[2],
        (".cmdline".to_owned(), 0x3000, b"loglevel=7 rw\0".to_vec())
    );
    assert_eq!(after[3..], before[3..], "the sections after it stay");
    assert!(!image.replace(".splash", b"x").unwrap());
}

#[test]
fn a_longer_section_moves_those_after_it() {
    let mut image = bundle(b"quiet\0");
    let long = vec![b'x'; 0x1800];

    assert!(image.replace(".cmdline", &long).unwrap());

    let after = layout(&write(&image).bytes);
    let addresses: Vec<(&str, u32)> = after
        .iter()
        .map(|(name, address, _)| (name.as_str(), *address))
        .collect();
    assert_eq!(
        addresses,
        [
            (".text", 0x1000),
            (".data", 0x2000),
            (".cmdline", 0x3000),
            (".initrd", 0x5000),
            (".linux", 0x7000)
        ]
    );
    assert_eq!(after[2].2, long);
    assert_eq!(after[4].2, vec![b'L'; 0x800]);
}

#[test]
fn a_section_is_inserted_before_others_or_removed() {
    let mut image = Image::read(Stub::new().build()).unwrap();
    image.add(".initrd", &[b'I'; 0x10]).unwrap();
    image.add(".linux", &[b'L'; 0x10]).unwrap();
    let mut image = Image::read(write(&image).bytes).unwrap();

    image
        .insert_before(".cmdline", b"quiet\0", &[".initrd", ".linux"])
        .unwrap();

    let names: Vec<(String, u32)> = layout(&write(&image).bytes)
        .into_iter()
        .map(|(name, address, _)| (name, address))
        .collect();
    assert_eq!(
        names[2..],
        [
            (".cmdline".to_owned(), 0x3000),
            (".initrd".to_owned(), 0x4000),
            (".linux".to_owned(), 0x5000),
        ]
    );

    let mut image = Image::read(write(&image).bytes).unwrap();
    assert_eq!(image.remove(".cmdline").unwrap().data(), b"quiet\0");
    assert_eq!(image.remove(".cmdline"), None);
    let bytes = write(&image).bytes;
    let names: Vec<String> = layout(&bytes).into_iter().map(|(name, ..)| name).collect();
    assert_eq!(names, [".text", ".data", ".initrd", ".linux"]);
    assert_eq!(
        u32_at(&bytes, 0x58 + 8),
        0x100 + 0x200 + 0x200,
        "SizeOfInitializedData"
    );

    let mut plain = Image::read(Stub::new().build()).unwrap();
    plain.insert_before(".cmdline", b"x", &[".linux"]).unwrap();
    assert_eq!(plain.sections().last().unwrap().name(), ".cmdline");
}

#[test]
fn errors_say_what_is_wrong() {
    assert_eq!(
        PeError::NotEfi(3).to_string(),
        "not an EFI image (subsystem 3)"
    );
    assert_eq!(
        PeError::Overlap(".a".to_owned(), ".b".to_owned()).to_string(),
        "the sections '.a' and '.b' overlap"
    );
    for error in [
        PeError::Malformed("x".to_owned()),
        PeError::Alignment {
            file: 3,
            section: 1,
        },
        PeError::LongName("n".to_owned()),
        PeError::OutsideFile("n".to_owned()),
        PeError::HeaderData,
        PeError::NoRoom,
        PeError::TooLarge,
    ] {
        assert!(!error.to_string().is_empty());
    }
}

proptest! {
    #[test]
    fn any_sections_are_laid_out_without_overlap(
        is_64 in any::<bool>(),
        sizes in proptest::collection::vec(0_usize..0x3000, 1..6),
    ) {
        let mut image = Image::read(Stub { is_64, ..Stub::new() }.build()).unwrap();
        let names = [".osrel", ".cmdline", ".splash", ".dtb", ".initrd", ".linux"];
        let datas: Vec<Vec<u8>> = sizes
            .iter()
            .enumerate()
            .map(|(index, size)| vec![u8::try_from(index).unwrap() + 1; *size])
            .collect();
        for (name, data) in names.iter().zip(&datas) {
            image.add(name, data).unwrap();
        }

        let bytes = write(&image).bytes;

        // Reading checks that nothing overlaps or lies outside the file.
        let read = Image::read(bytes.clone()).unwrap();
        let size_of_image = u64::from(u32_at(&bytes, 0x58 + 56));
        for (section, data) in read.sections()[2..].iter().zip(&datas) {
            prop_assert_eq!(section.data(), data.as_slice());
            prop_assert!(section.end() <= size_of_image);
            prop_assert_eq!(section.virtual_address() % SECTION_ALIGNMENT, 0);
        }
        prop_assert_eq!(u32_at(&bytes, 0x58 + 64), checksum(&bytes, 0x58 + 64));
    }
}
