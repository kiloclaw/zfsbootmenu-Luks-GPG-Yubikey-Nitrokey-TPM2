//! The memory of a DRM dumb buffer, mapped once for as long as the display
//! shows it (ROADMAP.md UI4, the console's C72): presenting a frame copies
//! its damage into this memory, instead of mapping the buffer and dropping
//! the mapping for each frame.

use std::fmt;
use std::io;
use std::os::fd::BorrowedFd;
use std::ptr::NonNull;

use rustix::ioctl::{Opcode, Updater, ioctl, opcode};
use rustix::mm::{MapFlags, ProtFlags};

/// `struct drm_mode_map_dumb` of `drm/drm_mode.h`: the buffer, and the
/// offset of its memory in the card's file, which the kernel fills.
#[repr(C)]
struct MapDumb {
    handle: u32,
    pad: u32,
    offset: u64,
}

/// `DRM_IOCTL_MODE_MAP_DUMB`: `DRM_IOWR(0xB3, struct drm_mode_map_dumb)`.
const MAP_DUMB: Opcode = opcode::read_write::<MapDumb>(b'd', 0xB3);

/// Shared memory of a file, mapped until this is dropped: what is written
/// reaches the file, here the memory of a dumb buffer the display scans
/// out.
pub struct Mapped {
    start: NonNull<u8>,
    len: usize,
}

impl Mapped {
    /// `len` bytes of `file` from `offset`, readable and writable, shared
    /// with it.
    ///
    /// # Errors
    ///
    /// When `len` is 0, or the kernel refuses the mapping.
    pub fn shared(file: BorrowedFd<'_>, offset: u64, len: usize) -> io::Result<Mapped> {
        if len == 0 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let read_write = ProtFlags::READ | ProtFlags::WRITE;
        // SAFETY: a new shared mapping of the file, which aliases no Rust
        // memory; it is unmapped once, when the value is dropped.
        let start = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                len,
                read_write,
                MapFlags::SHARED,
                file,
                offset,
            )?
        };
        let start = NonNull::new(start.cast::<u8>()).ok_or(io::ErrorKind::OutOfMemory)?;
        Ok(Mapped { start, len })
    }

    /// The `len` bytes of the dumb buffer `handle` of the DRM card `card`.
    ///
    /// # Errors
    ///
    /// When the card does not know the buffer, or refuses the mapping.
    pub fn dumb_buffer(card: BorrowedFd<'_>, handle: u32, len: usize) -> io::Result<Mapped> {
        let mut map = MapDumb {
            handle,
            pad: 0,
            offset: 0,
        };
        // SAFETY: `MAP_DUMB` takes a `struct drm_mode_map_dumb`, which
        // `MapDumb` is, and fills its offset.
        unsafe { ioctl(card, Updater::<MAP_DUMB, MapDumb>::new(&mut map))? };
        Mapped::shared(card, map.offset, len)
    }

    /// The bytes, to write.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: the mapping holds `len` readable and writable bytes for as
        // long as this value lives; `&mut self` makes the access exclusive
        // within the process.
        unsafe { std::slice::from_raw_parts_mut(self.start.as_ptr(), self.len) }
    }

    /// How many bytes are mapped.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing is mapped, which never is.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for Mapped {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Mapped")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl Drop for Mapped {
    fn drop(&mut self) {
        // SAFETY: the region `mmap` returned, unmapped once; no slice of it
        // outlives `self`.
        let _ = unsafe { rustix::mm::munmap(self.start.as_ptr().cast(), self.len) };
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real files"
)]
mod tests {
    use std::io::{Read as _, Seek as _, SeekFrom};
    use std::os::fd::AsFd as _;

    use super::*;

    #[test]
    fn what_is_written_reaches_the_file_shared() {
        let fd = rustix::fs::memfd_create("zbm-sys-drm", rustix::fs::MemfdFlags::CLOEXEC).unwrap();
        rustix::fs::ftruncate(&fd, 8192).unwrap();
        let mut file = std::fs::File::from(fd);

        let mut mapped = Mapped::shared(file.as_fd(), 4096, 4096).unwrap();
        assert_eq!((mapped.len(), mapped.is_empty()), (4096, false));
        mapped.bytes_mut()[..5].copy_from_slice(b"pixel");
        assert!(format!("{mapped:?}").contains("4096"));
        drop(mapped);

        let mut read = [0; 5];
        file.seek(SeekFrom::Start(4096)).unwrap();
        file.read_exact(&mut read).unwrap();
        assert_eq!(&read, b"pixel");
        assert!(Mapped::shared(file.as_fd(), 0, 0).is_err());
    }

    #[test]
    fn a_file_that_is_not_a_drm_card_has_no_dumb_buffer() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let error = Mapped::dumb_buffer(file.as_fd(), 1, 4096).unwrap_err();
        assert!(error.raw_os_error().is_some(), "{error}");
    }
}
