//! Writing an image: the headers of the image read, updated; the sections
//! that keep their place where they were, moved down as much as the headers
//! grew; the sections laid out after them; what followed the sections in
//! the file after those.

use object::pe;

use super::{Image, PeError, Placement, SECTION_HEADER, Section};

/// The file offsets, in the optional header, of the fields the writer sets.
const SIZE_OF_INITIALIZED_DATA: usize = 8;
const SIZE_OF_IMAGE: usize = 56;
const SIZE_OF_HEADERS: usize = 60;
const CHECK_SUM: usize = 64;

/// How to write an image.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WriteOptions {
    /// The time stamp of the image, `SOURCE_DATE_EPOCH`'s; the image's own
    /// when `None`.
    pub timestamp: Option<u32>,
}

/// An image written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Written {
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// What the writer had to drop, said to the person.
    pub warnings: Vec<String>,
}

/// A section as it is written.
struct Placed<'a> {
    section: &'a Section,
    virtual_address: u32,
    /// Its file offset, 0 without data.
    pointer: u32,
    /// The bytes it takes in the file: those read, or its data padded to
    /// the file alignment.
    file_size: u64,
}

/// `value` rounded up to a multiple of `alignment`, a power of two.
fn align(value: u64, alignment: u32) -> u64 {
    value.div_ceil(u64::from(alignment)) * u64::from(alignment)
}

/// `value` as a field of 32 bits.
fn field(value: u64) -> Result<u32, PeError> {
    u32::try_from(value).map_err(|_| PeError::TooLarge)
}

/// Writes `value` at `offset` of `bytes`, little-endian.
fn put(bytes: &mut [u8], offset: usize, value: &[u8]) {
    if let Some(slot) = bytes.get_mut(offset..offset + value.len()) {
        slot.copy_from_slice(value);
    }
}

/// The PE checksum of `bytes`, whose own checksum is at `skip`: the 16-bit
/// words added with their carries folded in, plus the length of the file.
pub fn checksum(bytes: &[u8], skip: usize) -> u32 {
    let mut sum: u64 = 0;
    for (index, pair) in bytes.chunks(2).enumerate() {
        let at = index * 2;
        if at == skip || at == skip + 2 {
            continue;
        }
        let word = match pair {
            [low, high] => u16::from_le_bytes([*low, *high]),
            [low] => u16::from(*low),
            _ => 0,
        };
        sum += u64::from(word);
        sum = (sum & 0xffff) + (sum >> 16);
    }
    let sum = (sum & 0xffff) + (sum >> 16);
    u32::try_from(sum + bytes.len() as u64).unwrap_or(u32::MAX)
}

impl Image {
    /// Writes the image with its sections.
    ///
    /// # Errors
    ///
    /// When the headers have no room for the section table and cannot grow,
    /// when bytes after the section table would be overwritten, or when the
    /// image would be larger than 4 GiB.
    pub fn write(&self, options: &WriteOptions) -> Result<Written, PeError> {
        let size_of_headers = self.size_of_headers()?;
        let grown = size_of_headers - self.headers.header_size;
        let placed = self.place(grown)?;
        let laid_end = placed
            .iter()
            .map(|placed| u64::from(placed.pointer) + placed.file_size)
            .max()
            .unwrap_or(0)
            .max(u64::from(size_of_headers));
        let (overlay, mut warnings) = self.overlay();
        let length = usize::try_from(laid_end).map_err(|_| PeError::TooLarge)? + overlay.len();
        field(length as u64)?;
        let mut bytes = vec![0_u8; length];
        let headers_end = (self.headers.header_size as usize).min(self.original.len());
        put(
            &mut bytes,
            0,
            self.original.get(..headers_end).unwrap_or_default(),
        );
        for placed in &placed {
            put(&mut bytes, placed.pointer as usize, &placed.section.raw);
        }
        let overlay_start = length - overlay.len();
        put(&mut bytes, overlay_start, overlay);
        self.write_table(&mut bytes, &placed);
        self.update_headers(&mut bytes, &placed, size_of_headers, options.timestamp)?;
        self.move_debug_pointers(&mut bytes, &placed, grown, overlay_start);
        if self.certificates.is_some() {
            warnings.push(
                "the stub was signed; its signature does not cover the new image, which is \
                 written without it"
                    .to_owned(),
            );
        }
        let skip = self.headers.optional + CHECK_SUM;
        let sum = checksum(&bytes, skip);
        put(&mut bytes, skip, &sum.to_le_bytes());
        Ok(Written { bytes, warnings })
    }

    /// The size of the headers with the section table: as they were, or
    /// grown by the file alignment until the table fits.
    fn size_of_headers(&self) -> Result<u32, PeError> {
        let old = self.headers.header_size;
        let table_end = self.headers.table + SECTION_HEADER * self.sections.len();
        let old_end = self.headers.table + SECTION_HEADER * self.headers.count;
        // The bytes the new section headers take must be free.
        let free = self
            .original
            .get(old_end..table_end.min(old as usize))
            .unwrap_or_default();
        if free.iter().any(|byte| *byte != 0) {
            return Err(PeError::HeaderData);
        }
        let needed = field(align(table_end as u64, self.headers.file_alignment))?.max(old);
        let first = self
            .sections
            .iter()
            .filter(|section| matches!(section.placement, Placement::Kept { .. }))
            .map(|section| section.virtual_address)
            .min();
        if needed > old && first.is_some_and(|first| first < needed) {
            return Err(PeError::NoRoom);
        }
        Ok(needed)
    }

    /// Every section with its address and file offset.
    fn place(&self, grown: u32) -> Result<Vec<Placed<'_>>, PeError> {
        let mut placed = Vec::with_capacity(self.sections.len());
        let mut memory_end = 0;
        let mut file_end = u64::from(self.headers.header_size + grown);
        for section in &self.sections {
            if let Placement::Kept { pointer } = section.placement {
                let pointer = match section.raw.len() {
                    0 => 0,
                    _ => field(u64::from(pointer) + u64::from(grown))?,
                };
                memory_end = memory_end.max(section.end());
                file_end = file_end.max(u64::from(pointer) + section.raw.len() as u64);
                placed.push(Placed {
                    section,
                    virtual_address: section.virtual_address,
                    pointer,
                    file_size: section.raw.len() as u64,
                });
            }
        }
        let alignment = self.headers.section_alignment;
        let mut next_address = align(memory_end, alignment);
        let mut next_pointer = align(file_end, self.headers.file_alignment);
        for section in &self.sections {
            if let Placement::LaidOut { at_least } = section.placement {
                let address = next_address.max(align(u64::from(at_least), alignment));
                let pointer = match section.raw.len() {
                    0 => 0,
                    _ => field(next_pointer)?,
                };
                let file_size = align(section.raw.len() as u64, self.headers.file_alignment);
                next_address = align(address + section.raw.len() as u64, alignment);
                next_pointer += file_size;
                placed.push(Placed {
                    section,
                    virtual_address: field(address)?,
                    pointer,
                    file_size,
                });
            }
        }
        field(next_address)?;
        placed.sort_by_key(|placed| placed.virtual_address);
        Ok(placed)
    }

    /// What followed the sections in the file, without a certificate table
    /// at its end; and a warning when a certificate table elsewhere stays.
    fn overlay(&self) -> (&[u8], Vec<String>) {
        let mut overlay = self.original.get(self.overlay..).unwrap_or_default();
        let mut warnings = Vec::new();
        if let Some((start, size)) = self.certificates {
            let (start, end) = (start as usize, start as usize + size as usize);
            if start >= self.overlay && end >= self.original.len() {
                overlay = self.original.get(self.overlay..start).unwrap_or_default();
            } else {
                warnings.push("the certificate table is not at the end of the stub".to_owned());
            }
        }
        (overlay, warnings)
    }

    /// Writes the section table: one header per section, in the order of
    /// their addresses.
    fn write_table(&self, bytes: &mut [u8], placed: &[Placed<'_>]) {
        for (index, placed) in placed.iter().enumerate() {
            let section = placed.section;
            let mut header = [0_u8; SECTION_HEADER];
            let name = section.name.as_bytes();
            put(
                &mut header,
                0,
                name.get(..name.len().min(8)).unwrap_or_default(),
            );
            let virtual_size = match section.placement {
                Placement::Kept { .. } => section.virtual_size,
                Placement::LaidOut { .. } => u32::try_from(section.raw.len()).unwrap_or(u32::MAX),
            };
            let raw_size = placed.file_size;
            put(&mut header, 8, &virtual_size.to_le_bytes());
            put(&mut header, 12, &placed.virtual_address.to_le_bytes());
            put(
                &mut header,
                16,
                &u32::try_from(raw_size).unwrap_or(u32::MAX).to_le_bytes(),
            );
            put(&mut header, 20, &placed.pointer.to_le_bytes());
            put(&mut header, 36, &section.characteristics.to_le_bytes());
            put(bytes, self.headers.table + index * SECTION_HEADER, &header);
        }
    }

    /// Sets the number of sections, the sizes, the time stamp and the
    /// certificate table's entry.
    fn update_headers(
        &self,
        bytes: &mut [u8],
        placed: &[Placed<'_>],
        size_of_headers: u32,
        timestamp: Option<u32>,
    ) -> Result<(), PeError> {
        let file_header = self.headers.nt + 4;
        let count = u16::try_from(placed.len()).map_err(|_| PeError::TooLarge)?;
        put(bytes, file_header + 2, &count.to_le_bytes());
        if let Some(timestamp) = timestamp {
            put(bytes, file_header + 4, &timestamp.to_le_bytes());
        }
        let image_end = placed
            .iter()
            .map(|placed| u64::from(placed.virtual_address) + placed_size(placed))
            .max()
            .unwrap_or(0);
        let optional = self.headers.optional;
        let size_of_image = field(align(image_end, self.headers.section_alignment))?;
        put(
            bytes,
            optional + SIZE_OF_IMAGE,
            &size_of_image.to_le_bytes(),
        );
        put(
            bytes,
            optional + SIZE_OF_HEADERS,
            &size_of_headers.to_le_bytes(),
        );
        let added: u64 = placed
            .iter()
            .filter(|placed| matches!(placed.section.placement, Placement::LaidOut { .. }))
            .filter(|placed| {
                placed.section.characteristics & pe::IMAGE_SCN_CNT_INITIALIZED_DATA != 0
            })
            .map(|placed| placed.file_size)
            .sum();
        let initialized = (u64::from(self.headers.size_of_initialized_data) + added)
            .saturating_sub(self.dropped_initialized);
        let initialized = u32::try_from(initialized).unwrap_or(u32::MAX);
        put(
            bytes,
            optional + SIZE_OF_INITIALIZED_DATA,
            &initialized.to_le_bytes(),
        );
        if self.certificates.is_some()
            && self.headers.directory_count > pe::IMAGE_DIRECTORY_ENTRY_SECURITY
        {
            let entry = self.headers.directories + 8 * pe::IMAGE_DIRECTORY_ENTRY_SECURITY;
            put(bytes, entry, &[0; 8]);
        }
        Ok(())
    }

    /// Points the debug directory's entries at their data where it moved.
    fn move_debug_pointers(
        &self,
        bytes: &mut [u8],
        placed: &[Placed<'_>],
        grown: u32,
        overlay_start: usize,
    ) {
        let moved = |offset: usize| -> Option<usize> {
            self.moved_offset(offset, placed, grown, overlay_start)
        };
        for &field_offset in &self.debug_pointers {
            let Some(old) = self
                .original
                .get(field_offset..field_offset + 4)
                .and_then(|slice| <[u8; 4]>::try_from(slice).ok())
                .map(u32::from_le_bytes)
            else {
                continue;
            };
            let (Some(new_field), Some(new_value)) = (moved(field_offset), moved(old as usize))
            else {
                continue;
            };
            if old != 0 {
                let value = u32::try_from(new_value).unwrap_or(u32::MAX);
                put(bytes, new_field, &value.to_le_bytes());
            }
        }
    }

    /// Where the byte at `offset` of the image read is in the image written.
    fn moved_offset(
        &self,
        offset: usize,
        placed: &[Placed<'_>],
        grown: u32,
        overlay_start: usize,
    ) -> Option<usize> {
        if offset < self.headers.header_size as usize {
            return Some(offset);
        }
        if offset >= self.overlay {
            return Some(overlay_start + (offset - self.overlay));
        }
        placed
            .iter()
            .find_map(|placed| match placed.section.placement {
                Placement::Kept { pointer } => {
                    let start = pointer as usize;
                    (start..start + placed.section.raw.len())
                        .contains(&offset)
                        .then(|| offset + grown as usize)
                }
                Placement::LaidOut { .. } => None,
            })
    }
}

/// The size of a section in memory, as written.
fn placed_size(placed: &Placed<'_>) -> u64 {
    match placed.section.placement {
        Placement::Kept { .. } => placed.section.end() - u64::from(placed.section.virtual_address),
        Placement::LaidOut { .. } => placed.section.raw.len() as u64,
    }
}
