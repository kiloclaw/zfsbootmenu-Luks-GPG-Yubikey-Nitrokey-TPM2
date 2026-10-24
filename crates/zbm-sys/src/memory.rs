//! Memory for secrets (ROADMAP.md #1): whole pages of their own, which are
//! never swapped out, left out of core dumps and of the memory of child
//! processes, and wiped before they are given back.
//!
//! A [`Pages`] region comes, where the kernel offers it, from
//! `memfd_secret` (Linux 5.14 and later, when `secretmem` is enabled): its
//! pages are then also gone from the kernel's own map of physical memory, so
//! that no other process, and no kernel code that reads memory by its
//! physical address, can see them. Otherwise it is anonymous memory, locked
//! with `mlock` when the process may lock that much (root may; others up to
//! `RLIMIT_MEMLOCK`). Either way the region is marked `MADV_DONTDUMP`, and
//! the anonymous one `MADV_WIPEONFORK`; the `memfd_secret` one is not mapped
//! in child processes at all (`MADV_DONTFORK`).

use std::ffi::{c_int, c_long, c_void};
use std::io;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        off: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn mlock(addr: *const c_void, len: usize) -> c_int;
    fn madvise(addr: *mut c_void, len: usize, advice: c_int) -> c_int;
    fn ftruncate(fd: c_int, length: i64) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn sysconf(name: c_int) -> c_long;
    fn syscall(number: c_long, ...) -> c_long;
    fn explicit_bzero(s: *mut c_void, n: usize);
    fn prctl(option: c_int, ...) -> c_int;
}

/// `PR_SET_DUMPABLE`.
const PR_SET_DUMPABLE: c_int = 4;

/// `PR_GET_DUMPABLE`.
const PR_GET_DUMPABLE: c_int = 3;

/// Makes the process undumpable: no core dump, and only processes with
/// `CAP_SYS_PTRACE` (root) may read its memory through `ptrace` or
/// `/proc/PID/mem`. Programs it starts are dumpable again once they run
/// another program, unless they are set-user-ID.
///
/// # Errors
///
/// When the kernel refuses.
pub fn set_undumpable() -> io::Result<()> {
    // SAFETY: `prctl(PR_SET_DUMPABLE, 0)` reads no memory of the caller; the
    // argument is passed as a whole register.
    if unsafe { prctl(PR_SET_DUMPABLE, 0 as c_long) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Whether the process is dumpable.
pub fn is_dumpable() -> bool {
    // SAFETY: `prctl(PR_GET_DUMPABLE)` reads no memory of the caller.
    unsafe { prctl(PR_GET_DUMPABLE) > 0 }
}

/// `PROT_READ | PROT_WRITE`.
const READ_WRITE: c_int = 0x1 | 0x2;

/// `MAP_SHARED`, for the `memfd_secret` file.
const MAP_SHARED: c_int = 0x1;

/// `MAP_PRIVATE`.
const MAP_PRIVATE: c_int = 0x2;

/// `MAP_ANONYMOUS`, which MIPS numbers differently.
#[cfg(any(target_arch = "mips", target_arch = "mips64"))]
const MAP_ANONYMOUS: c_int = 0x800;
#[cfg(not(any(target_arch = "mips", target_arch = "mips64")))]
const MAP_ANONYMOUS: c_int = 0x20;

/// What `mmap` returns when it fails.
const MAP_FAILED: *mut c_void = usize::MAX as *mut c_void;

/// `MADV_DONTFORK`: child processes do not get the mapping.
const MADV_DONTFORK: c_int = 10;

/// `MADV_DONTDUMP`: core dumps leave the mapping out.
const MADV_DONTDUMP: c_int = 16;

/// `MADV_WIPEONFORK`: child processes get the mapping filled with zeros.
const MADV_WIPEONFORK: c_int = 18;

/// `_SC_PAGESIZE` in glibc and musl.
const SC_PAGESIZE: c_int = 30;

/// The number of `memfd_secret`, the same on every architecture that has it
/// (the common numbering of new system calls).
const SYS_MEMFD_SECRET: c_long = 447;

/// How many regions the process holds now.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// How many [`Pages`] regions the process holds now: the secrets it has not
/// dropped yet.
pub fn live_regions() -> usize {
    LIVE.load(Ordering::SeqCst)
}

/// How a region keeps its secret, from the strongest.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protection {
    /// `memfd_secret`: locked, and out of the kernel's map of memory.
    Secret,
    /// Anonymous memory locked with `mlock`.
    Locked,
    /// Anonymous memory the process could not lock, which may be swapped out.
    Unlocked,
}

/// Pages of memory for a secret, filled with zeros when made, and wiped and
/// unmapped when dropped.
#[derive(Debug)]
pub struct Pages {
    start: NonNull<u8>,
    len: usize,
    protection: Protection,
}

// SAFETY: the region belongs to one `Pages` value alone, like the buffer of a
// `Vec<u8>`; moving it to another thread or sharing `&Pages` is as safe as for
// a `Vec<u8>`.
unsafe impl Send for Pages {}
// SAFETY: as for `Send`: `&Pages` only reads the bytes.
unsafe impl Sync for Pages {}

/// The size of a page of memory.
pub fn page_size() -> usize {
    // SAFETY: `sysconf` takes an integer and reads no memory of the caller.
    let size = unsafe { sysconf(SC_PAGESIZE) };
    usize::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .unwrap_or(4096)
}

/// `bytes` rounded up to whole pages, at least one.
fn whole_pages(bytes: usize) -> Option<usize> {
    let page = page_size();
    bytes.max(1).checked_next_multiple_of(page)
}

impl Pages {
    /// At least `bytes` bytes, in whole pages: from `memfd_secret` where the
    /// kernel allows it, else anonymous memory, locked where it can be.
    ///
    /// # Errors
    ///
    /// When no memory can be mapped.
    pub fn new(bytes: usize) -> io::Result<Pages> {
        let len = whole_pages(bytes).ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
        match Pages::from_secretmem(len) {
            Some(pages) => Ok(pages),
            None => Pages::anonymous(len, true),
        }
    }

    /// [`Pages::new`], or the end of the process when no memory can be
    /// mapped, as when the heap is full: mapping a page fails only when the
    /// machine has no memory left.
    pub fn new_or_abort(bytes: usize) -> Pages {
        Pages::new(bytes).unwrap_or_else(|error| {
            // Like `handle_alloc_error`, which needs a layout this has not.
            let message = format!("zbm: no memory for a secret: {error}\n");
            let _ = io::Write::write_all(&mut io::stderr(), message.as_bytes());
            std::process::abort()
        })
    }

    /// Anonymous memory alone, locked when `lock` says so and it can be: for
    /// tests of the fall-back.
    ///
    /// # Errors
    ///
    /// When no memory can be mapped.
    pub fn new_anonymous(bytes: usize, lock: bool) -> io::Result<Pages> {
        let len = whole_pages(bytes).ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
        Pages::anonymous(len, lock)
    }

    /// `len` bytes from `memfd_secret`, or `None` when the kernel refuses.
    fn from_secretmem(len: usize) -> Option<Pages> {
        let size = i64::try_from(len).ok()?;
        // SAFETY: `memfd_secret(unsigned int flags)` reads no memory of the
        // caller (0, no flags, is passed as a whole register); the file it
        // makes is sized, mapped as a new mapping that aliases no Rust
        // memory, and closed, as the mapping keeps the memory. The advice
        // applies to that mapping alone.
        let start = unsafe {
            let fd = syscall(SYS_MEMFD_SECRET, 0 as c_long);
            let fd = c_int::try_from(fd).ok().filter(|fd| *fd >= 0)?;
            let start = if ftruncate(fd, size) == 0 {
                mmap(std::ptr::null_mut(), len, READ_WRITE, MAP_SHARED, fd, 0)
            } else {
                MAP_FAILED
            };
            close(fd);
            if start != MAP_FAILED {
                madvise(start, len, MADV_DONTDUMP);
                madvise(start, len, MADV_DONTFORK);
            }
            start
        };
        Pages::mapped(start, len, Protection::Secret)
    }

    /// `len` bytes of anonymous memory, locked when `lock` says so and the
    /// process may.
    fn anonymous(len: usize, lock: bool) -> io::Result<Pages> {
        let flags = MAP_PRIVATE | MAP_ANONYMOUS;
        let mut locked = false;
        // SAFETY: an anonymous mapping takes no file and aliases no Rust
        // memory; the other calls apply to it alone, and a failure of them
        // leaves it as it was.
        let start = unsafe {
            let start = mmap(std::ptr::null_mut(), len, READ_WRITE, flags, -1, 0);
            if start != MAP_FAILED {
                locked = lock && mlock(start, len) == 0;
                madvise(start, len, MADV_DONTDUMP);
                madvise(start, len, MADV_WIPEONFORK);
            }
            start
        };
        let protection = if locked {
            Protection::Locked
        } else {
            Protection::Unlocked
        };
        Pages::mapped(start, len, protection).ok_or_else(io::Error::last_os_error)
    }

    /// The region `mmap` returned, or `None` when it failed.
    fn mapped(start: *mut c_void, len: usize, protection: Protection) -> Option<Pages> {
        if start == MAP_FAILED {
            return None;
        }
        let start = NonNull::new(start.cast::<u8>())?;
        LIVE.fetch_add(1, Ordering::SeqCst);
        Some(Pages {
            start,
            len,
            protection,
        })
    }

    /// The bytes of the region, all of them.
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the mapping holds `len` readable bytes, initialised (to zero
        // by the kernel), for as long as this value lives.
        unsafe { std::slice::from_raw_parts(self.start.as_ptr(), self.len) }
    }

    /// The bytes of the region, to write.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: as in `bytes`; `&mut self` makes the access exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.start.as_ptr(), self.len) }
    }

    /// How many bytes the region holds.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the region is empty, which it never is.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How the region keeps its secret.
    pub fn protection(&self) -> Protection {
        self.protection
    }
}

impl Drop for Pages {
    fn drop(&mut self) {
        let start = self.start.as_ptr().cast::<c_void>();
        // SAFETY: the region is this value's mapping of `len` bytes; it is
        // wiped, then unmapped, and never used again.
        unsafe {
            explicit_bzero(start, self.len);
            munmap(start, self.len);
        }
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_can_make_itself_undumpable() {
        // Each test runs in a process of its own under nextest; under `cargo
        // test` the rest of the binary is undumpable too, which harms nothing.
        set_undumpable().unwrap();
        assert!(!is_dumpable());
    }

    #[test]
    fn regions_are_whole_pages_of_zeros_that_keep_what_is_written() {
        let page = page_size();
        let mut pages = Pages::new(10).unwrap();
        assert_eq!(pages.len(), page);
        assert!(!pages.is_empty());
        assert!(pages.bytes().iter().all(|byte| *byte == 0));
        pages.bytes_mut()[..6].copy_from_slice(b"hunter");
        assert_eq!(&pages.bytes()[..6], b"hunter");
        assert_eq!(Pages::new(page + 1).unwrap().len(), 2 * page);
        assert_eq!(Pages::new(0).unwrap().len(), page);
        let before = live_regions();
        let held = Pages::new(1).unwrap();
        assert!(live_regions() > before);
        drop(held);
        assert!(Pages::new(usize::MAX).is_err());
    }

    #[test]
    fn anonymous_regions_are_locked_when_asked_and_allowed() {
        let unlocked = Pages::new_anonymous(1, false).unwrap();
        assert_eq!(unlocked.protection(), Protection::Unlocked);
        // A page is far below any limit on locked memory.
        let locked = Pages::new_anonymous(1, true).unwrap();
        assert_eq!(locked.protection(), Protection::Locked);
        // The strongest protection the kernel of the test machine allows.
        let best = Pages::new(1).unwrap().protection();
        assert!(
            matches!(best, Protection::Secret | Protection::Locked),
            "{best:?}"
        );
    }

    /// The `VmFlags` of the mapping that starts at `start`, from
    /// `/proc/self/smaps`.
    fn vm_flags(start: usize) -> String {
        let maps = std::fs::read_to_string("/proc/self/smaps").unwrap();
        let header = format!("{start:x}-");
        maps.lines()
            .skip_while(|line| !line.starts_with(&header))
            .find(|line| line.starts_with("VmFlags:"))
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn regions_stay_out_of_core_dumps_and_child_processes() {
        let anonymous = Pages::new_anonymous(1, true).unwrap();
        let flags = vm_flags(anonymous.bytes().as_ptr() as usize);
        for flag in [" dd", " wf", " lo"] {
            assert!(flags.contains(flag), "{flag} in {flags}");
        }
        let best = Pages::new(1).unwrap();
        let flags = vm_flags(best.bytes().as_ptr() as usize);
        assert!(flags.contains(" dd"), "{flags}");
        if best.protection() == Protection::Secret {
            assert!(flags.contains(" dc"), "MADV_DONTFORK in {flags}");
            assert!(flags.contains(" lo"), "{flags}");
        }
    }
}
