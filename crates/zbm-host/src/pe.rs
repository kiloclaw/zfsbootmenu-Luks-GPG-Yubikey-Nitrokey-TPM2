//! PE images, as the EFI stub and the bundles built from it are (ADR 0014):
//! read with `object`'s PE reader, changed as a list of sections, and
//! written back as the stub with its sections, its own headers kept.
//!
//! A section read from the image keeps its place, in memory and in the file
//! (moved down in the file only when the headers grow). A section added, or
//! one that comes after a section that changed, is laid out: placed after
//! the sections that keep their place, each at the section alignment, its
//! data at the file alignment.

use std::fmt;

use object::LittleEndian as LE;
use object::pe;
use object::read::pe::{ImageNtHeaders, ImageOptionalHeader};

mod write;

pub use write::{WriteOptions, Written, checksum};

/// What sections the host tools add are: initialized data, readable.
pub const DATA: u32 = pe::IMAGE_SCN_CNT_INITIALIZED_DATA | pe::IMAGE_SCN_MEM_READ;

/// The size of a section header.
const SECTION_HEADER: usize = 40;

/// Why an image cannot be read or written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeError {
    /// Not a PE image: what the reader found wrong.
    Malformed(String),
    /// Not an EFI image: its subsystem.
    NotEfi(u16),
    /// Alignments that are not powers of two, or a section alignment below
    /// the file alignment.
    Alignment {
        /// The file alignment.
        file: u32,
        /// The section alignment.
        section: u32,
    },
    /// A section name longer than eight bytes.
    LongName(String),
    /// A section whose data lies outside the file.
    OutsideFile(String),
    /// Two sections that overlap, in the file or in memory.
    Overlap(String, String),
    /// Data after the section table, where new section headers would go.
    HeaderData,
    /// Headers that cannot grow: the first section starts in memory where
    /// they would end.
    NoRoom,
    /// An image larger than 4 GiB.
    TooLarge,
}

impl fmt::Display for PeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeError::Malformed(why) => write!(formatter, "not a PE image: {why}"),
            PeError::NotEfi(subsystem) => {
                write!(formatter, "not an EFI image (subsystem {subsystem})")
            }
            PeError::Alignment { file, section } => write!(
                formatter,
                "unusable alignments: 0x{file:x} in the file, 0x{section:x} in memory"
            ),
            PeError::LongName(name) => {
                write!(
                    formatter,
                    "the section name '{name}' is longer than 8 bytes"
                )
            }
            PeError::OutsideFile(name) => {
                write!(formatter, "the section '{name}' lies outside the file")
            }
            PeError::Overlap(first, second) => {
                write!(formatter, "the sections '{first}' and '{second}' overlap")
            }
            PeError::HeaderData => {
                formatter.write_str("the headers hold data where new section headers would go")
            }
            PeError::NoRoom => formatter.write_str(
                "the headers have no room for more sections, and cannot grow \
                 before the first section",
            ),
            PeError::TooLarge => formatter.write_str("the image would be larger than 4 GiB"),
        }
    }
}

/// Where a section is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Placement {
    /// Where the image had it: its data at this file offset.
    Kept {
        /// The file offset of its data.
        pointer: u32,
    },
    /// Laid out by the writer, in memory at this address or after.
    LaidOut {
        /// The lowest address it may have.
        at_least: u32,
    },
}

/// One section.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    name: String,
    characteristics: u32,
    virtual_address: u32,
    virtual_size: u32,
    /// Its bytes in the file, the padding to the file alignment included.
    raw: Vec<u8>,
    placement: Placement,
}

impl Section {
    /// Its name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its address in memory, relative to the image base.
    pub fn virtual_address(&self) -> u32 {
        self.virtual_address
    }

    /// Its size in memory.
    pub fn virtual_size(&self) -> u32 {
        self.virtual_size
    }

    /// Its flags.
    pub fn characteristics(&self) -> u32 {
        self.characteristics
    }

    /// Its contents: its bytes in the file, up to its size in memory.
    pub fn data(&self) -> &[u8] {
        let size = usize::try_from(self.virtual_size).unwrap_or(usize::MAX);
        match self.virtual_size {
            0 => &self.raw,
            _ => self.raw.get(..size).unwrap_or(&self.raw),
        }
    }

    /// The end of the section in memory.
    fn end(&self) -> u64 {
        let size = match self.virtual_size {
            0 => self.raw.len() as u64,
            size => u64::from(size),
        };
        u64::from(self.virtual_address) + size
    }

    /// A new section of initialized, readable data.
    fn new(name: &str, data: &[u8], at_least: u32) -> Result<Section, PeError> {
        if name.len() > 8 {
            return Err(PeError::LongName(name.to_owned()));
        }
        Ok(Section {
            name: name.to_owned(),
            characteristics: DATA,
            virtual_address: 0,
            virtual_size: u32::try_from(data.len()).map_err(|_| PeError::TooLarge)?,
            raw: data.to_vec(),
            placement: Placement::LaidOut { at_least },
        })
    }
}

/// Where the headers are, and what the writer keeps of them.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Headers {
    /// The file offset of the PE signature.
    nt: usize,
    /// The file offset of the optional header.
    optional: usize,
    /// The file offset of the data directories.
    directories: usize,
    /// The number of data directories.
    directory_count: usize,
    /// The file offset of the section table.
    table: usize,
    /// The number of sections the image had.
    count: usize,
    header_size: u32,
    file_alignment: u32,
    section_alignment: u32,
    size_of_initialized_data: u32,
}

/// A PE image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image {
    original: Vec<u8>,
    headers: Headers,
    sections: Vec<Section>,
    /// The file range of the certificate table, when the image is signed.
    certificates: Option<(u32, u32)>,
    /// The file offsets of the debug directory's pointers to its data.
    debug_pointers: Vec<usize>,
    /// Where the data after the sections starts in the file.
    overlay: usize,
    /// The initialized data of the sections the writer no longer keeps.
    dropped_initialized: u64,
}

impl Image {
    /// Reads the PE32 or PE32+ image `bytes`.
    ///
    /// # Errors
    ///
    /// When it is not a PE image, not an EFI image, or its sections lie
    /// outside the file, overlap or have long names.
    pub fn read(bytes: Vec<u8>) -> Result<Image, PeError> {
        let magic = object::read::pe::optional_header_magic(bytes.as_slice()).map_err(malformed)?;
        if magic == pe::IMAGE_NT_OPTIONAL_HDR64_MAGIC {
            parse::<pe::ImageNtHeaders64>(bytes)
        } else {
            parse::<pe::ImageNtHeaders32>(bytes)
        }
    }

    /// The sections, in the order the writer lays them out.
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The section named `name`.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.name == name)
    }

    /// The image's alignment in memory.
    pub fn section_alignment(&self) -> u32 {
        self.headers.section_alignment
    }

    /// Adds the section `name` with `data` after the others.
    ///
    /// # Errors
    ///
    /// When the name is longer than 8 bytes or the data larger than 4 GiB.
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), PeError> {
        self.sections.push(Section::new(name, data, 0)?);
        Ok(())
    }

    /// Gives the section `name` the contents `data`, at its address when
    /// they fit before the next section; the sections after it are laid
    /// out again when they do not. `false` when there is no such section.
    ///
    /// # Errors
    ///
    /// When the data is larger than 4 GiB.
    pub fn replace(&mut self, name: &str, data: &[u8]) -> Result<bool, PeError> {
        let Some(at) = self
            .sections
            .iter()
            .position(|section| section.name == name)
        else {
            return Ok(false);
        };
        let address = self
            .sections
            .get(at)
            .map_or(0, |section| section.virtual_address);
        self.lay_out_from(address);
        let replacement = Section::new(name, data, address)?;
        if let Some(section) = self.sections.get_mut(at) {
            *section = Section {
                characteristics: section.characteristics,
                ..replacement
            };
        }
        Ok(true)
    }

    /// Adds the section `name` with `data` before the first of the sections
    /// named in `before`, which, with those after it, are laid out again;
    /// after the others when there is none.
    ///
    /// # Errors
    ///
    /// When the name is longer than 8 bytes or the data larger than 4 GiB.
    pub fn insert_before(
        &mut self,
        name: &str,
        data: &[u8],
        before: &[&str],
    ) -> Result<(), PeError> {
        let next = self
            .sections
            .iter()
            .filter(|section| before.contains(&section.name.as_str()))
            .map(|section| section.virtual_address)
            .min();
        let Some(address) = next else {
            return self.add(name, data);
        };
        self.lay_out_from(address);
        let at = self
            .sections
            .iter()
            .position(|section| section.virtual_address >= address)
            .unwrap_or(self.sections.len());
        self.sections.insert(at, Section::new(name, data, address)?);
        Ok(())
    }

    /// Removes the section `name`, and gives it back; `None` when there is
    /// none.
    pub fn remove(&mut self, name: &str) -> Option<Section> {
        let at = self
            .sections
            .iter()
            .position(|section| section.name == name)?;
        let section = self.sections.remove(at);
        self.drop_kept(&section);
        Some(section)
    }

    /// Lets the writer lay out every section from `address` on, keeping
    /// them in memory at their addresses or after.
    fn lay_out_from(&mut self, address: u32) {
        let mut dropped = 0;
        for section in &mut self.sections {
            if section.virtual_address >= address
                && let Placement::Kept { .. } = section.placement
            {
                if section.characteristics & pe::IMAGE_SCN_CNT_INITIALIZED_DATA != 0 {
                    dropped += section.raw.len() as u64;
                }
                section.placement = Placement::LaidOut {
                    at_least: section.virtual_address,
                };
            }
        }
        self.dropped_initialized += dropped;
    }

    /// Counts the data of `section`, which the writer no longer keeps where
    /// it was.
    fn drop_kept(&mut self, section: &Section) {
        let is_initialized = section.characteristics & pe::IMAGE_SCN_CNT_INITIALIZED_DATA != 0;
        if let Placement::Kept { .. } = section.placement
            && is_initialized
        {
            self.dropped_initialized += section.raw.len() as u64;
        }
    }
}

/// The reader's error, as a [`PeError`].
fn malformed(error: object::read::Error) -> PeError {
    PeError::Malformed(error.to_string())
}

/// Reads the image `bytes`, whose NT headers are `Pe`'s.
fn parse<Pe: ImageNtHeaders>(bytes: Vec<u8>) -> Result<Image, PeError> {
    let slice = bytes.as_slice();
    let dos = pe::ImageDosHeader::parse(slice).map_err(malformed)?;
    let nt = dos.nt_headers_offset();
    let mut offset = u64::from(nt);
    let (nt_headers, directories) = Pe::parse(slice, &mut offset).map_err(malformed)?;
    let table = nt_headers.sections(slice, offset).map_err(malformed)?;
    let optional = nt_headers.optional_header();
    check_efi(optional)?;
    let (file, section) = (optional.file_alignment(), optional.section_alignment());
    let to_usize = |value: u64| usize::try_from(value).map_err(|_| PeError::TooLarge);
    let nt = to_usize(u64::from(nt))?;
    let optional_offset = nt + 24;
    let table_offset = to_usize(offset)?;
    let headers = Headers {
        nt,
        optional: optional_offset,
        directories: optional_offset + std::mem::size_of::<Pe::ImageOptionalHeader>(),
        directory_count: directories.len(),
        table: table_offset,
        count: table.len(),
        header_size: optional.size_of_headers(),
        file_alignment: file,
        section_alignment: section,
        size_of_initialized_data: optional.size_of_initialized_data(),
    };
    if (headers.header_size as usize) < table_offset + SECTION_HEADER * table.len() {
        return Err(PeError::Malformed(
            "the headers end inside the section table".to_owned(),
        ));
    }
    let sections = table
        .iter()
        .map(|header| read_section(slice, header))
        .collect::<Result<Vec<_>, _>>()?;
    check_overlaps(&sections, headers.header_size)?;
    let certificates = directories
        .get(pe::IMAGE_DIRECTORY_ENTRY_SECURITY)
        .map(pe::ImageDataDirectory::address_range)
        .filter(|(_, size)| *size > 0);
    let debug_pointers = debug_pointers(directories.get(pe::IMAGE_DIRECTORY_ENTRY_DEBUG), &table)?;
    let overlay = overlay_start(&sections, headers.header_size);
    Ok(Image {
        original: bytes,
        headers,
        sections,
        certificates,
        debug_pointers,
        overlay,
        dropped_initialized: 0,
    })
}

/// Checks that `optional` is the header of an EFI image with alignments it
/// can use.
fn check_efi(optional: &impl ImageOptionalHeader) -> Result<(), PeError> {
    let subsystem = optional.subsystem();
    if !(pe::IMAGE_SUBSYSTEM_EFI_APPLICATION..=pe::IMAGE_SUBSYSTEM_EFI_ROM).contains(&subsystem) {
        return Err(PeError::NotEfi(subsystem));
    }
    let (file, section) = (optional.file_alignment(), optional.section_alignment());
    if !file.is_power_of_two() || !section.is_power_of_two() || section < file {
        return Err(PeError::Alignment { file, section });
    }
    Ok(())
}

/// The file offsets of the pointers to their data of the entries of the
/// debug directory `entry`.
fn debug_pointers(
    entry: Option<&pe::ImageDataDirectory>,
    table: &object::read::coff::SectionTable<'_>,
) -> Result<Vec<usize>, PeError> {
    let Some(entry) = entry.filter(|entry| entry.size.get(LE) > 0) else {
        return Ok(Vec::new());
    };
    let (start, size) = entry.file_range(table).map_err(malformed)?;
    let entry_size = std::mem::size_of::<pe::ImageDebugDirectory>();
    let start = start as usize;
    Ok((0..size as usize / entry_size)
        .map(|index| start + index * entry_size + 24)
        .collect())
}

/// Where the data after the sections starts in the file: after the last
/// section's data, or the headers.
fn overlay_start(sections: &[Section], header_size: u32) -> usize {
    sections
        .iter()
        .filter_map(|section| match section.placement {
            Placement::Kept { pointer } if !section.raw.is_empty() => {
                Some(pointer as usize + section.raw.len())
            }
            _ => None,
        })
        .max()
        .unwrap_or(0)
        .max(header_size as usize)
}

/// The section `header` describes, with its data from `bytes`.
fn read_section(bytes: &[u8], header: &pe::ImageSectionHeader) -> Result<Section, PeError> {
    let raw_name = header.raw_name();
    let name = String::from_utf8_lossy(raw_name).into_owned();
    if raw_name.starts_with(b"/") {
        return Err(PeError::LongName(name));
    }
    let pointer = header.pointer_to_raw_data.get(LE);
    let size = header.size_of_raw_data.get(LE);
    let raw = match size {
        0 => Vec::new(),
        _ => bytes
            .get(pointer as usize..pointer as usize + size as usize)
            .ok_or_else(|| PeError::OutsideFile(name.clone()))?
            .to_vec(),
    };
    Ok(Section {
        name,
        characteristics: header.characteristics.get(LE),
        virtual_address: header.virtual_address.get(LE),
        virtual_size: header.virtual_size.get(LE),
        raw,
        placement: Placement::Kept { pointer },
    })
}

/// Checks that no two sections share bytes of the file or of memory, and
/// that no data lies in the headers.
fn check_overlaps(sections: &[Section], size_of_headers: u32) -> Result<(), PeError> {
    let mut in_file: Vec<(u64, u64, &str)> = sections
        .iter()
        .filter_map(|section| match section.placement {
            Placement::Kept { pointer } if !section.raw.is_empty() => Some((
                u64::from(pointer),
                u64::from(pointer) + section.raw.len() as u64,
                section.name.as_str(),
            )),
            _ => None,
        })
        .collect();
    in_file.push((0, u64::from(size_of_headers), "the headers"));
    let mut in_memory: Vec<(u64, u64, &str)> = sections
        .iter()
        .filter(|section| section.end() > u64::from(section.virtual_address))
        .map(|section| {
            (
                u64::from(section.virtual_address),
                section.end(),
                section.name.as_str(),
            )
        })
        .collect();
    for ranges in [&mut in_file, &mut in_memory] {
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            if let [(_, end, first), (start, _, second)] = pair
                && start < end
            {
                return Err(PeError::Overlap((*first).to_owned(), (*second).to_owned()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
