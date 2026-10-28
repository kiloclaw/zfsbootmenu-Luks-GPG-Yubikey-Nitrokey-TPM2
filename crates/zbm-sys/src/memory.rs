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

use std::ffi::{c_long, c_void};
use std::io;
use std::os::fd::{FromRawFd as _, OwnedFd};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};

use rustix::mm::{Advice, MapFlags, MlockAllFlags, ProtFlags};
use rustix::process::DumpableBehavior;

unsafe extern "C" {
    // `rustix` has no `memfd_secret`.
    fn syscall(number: c_long, ...) -> c_long;
}

/// Makes the process undumpable: no core dump, and only processes with
/// `CAP_SYS_PTRACE` (root) may read its memory through `ptrace` or
/// `/proc/PID/mem`. Programs it starts are dumpable again once they run
/// another program, unless they are set-user-ID.
///
/// # Errors
///
/// When the kernel refuses.
pub fn set_undumpable() -> io::Result<()> {
    Ok(rustix::process::set_dumpable_behavior(
        DumpableBehavior::NotDumpable,
    )?)
}

/// Keeps all of the process's memory, now and later, out of swap, page by
/// page as it is first used: what the console on the display keeps of the
/// screen and its history.
///
/// # Errors
///
/// When the kernel refuses: without `CAP_IPC_LOCK`, beyond
/// `RLIMIT_MEMLOCK`, or before Linux 4.4.
pub fn lock_all_memory() -> io::Result<()> {
    let all = MlockAllFlags::CURRENT | MlockAllFlags::FUTURE | MlockAllFlags::ONFAULT;
    Ok(rustix::mm::mlockall(all)?)
}

/// Whether the process is dumpable.
pub fn is_dumpable() -> bool {
    rustix::process::dumpable_behavior()
        .is_ok_and(|behavior| behavior != DumpableBehavior::NotDumpable)
}

/// The number of `memfd_secret`, the same on every architecture that has it
/// (the common numbering of new system calls).
const SYS_MEMFD_SECRET: c_long = 447;

/// `O_CLOEXEC`, the only flag of `memfd_secret`, which SPARC numbers
/// differently.
#[cfg(any(target_arch = "sparc", target_arch = "sparc64"))]
const O_CLOEXEC: c_long = 0x40_0000;
#[cfg(not(any(target_arch = "sparc", target_arch = "sparc64")))]
const O_CLOEXEC: c_long = 0o2_000_000;

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
    rustix::param::page_size()
}

/// `bytes` rounded up to whole pages, at least one.
fn whole_pages(bytes: usize) -> Option<usize> {
    let page = page_size();
    bytes.max(1).checked_next_multiple_of(page)
}

/// `memfd_secret(O_CLOEXEC)`: a file whose pages are out of the kernel's
/// map of memory, or `None` when the kernel has none.
fn memfd_secret() -> Option<OwnedFd> {
    // SAFETY: `memfd_secret(unsigned int flags)` reads no memory of the
    // caller; the flags are passed as a whole register.
    let fd = unsafe { syscall(SYS_MEMFD_SECRET, O_CLOEXEC) };
    let fd = i32::try_from(fd).ok().filter(|fd| *fd >= 0)?;
    // SAFETY: the kernel just made `fd`, and nothing else owns it.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
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
    /// The file is closed once mapped, as the mapping keeps the memory; it
    /// is closed on exec too, so a thread that starts a program meanwhile
    /// does not hand it the secret.
    fn from_secretmem(len: usize) -> Option<Pages> {
        let fd = memfd_secret()?;
        rustix::fs::ftruncate(&fd, u64::try_from(len).ok()?).ok()?;
        let read_write = ProtFlags::READ | ProtFlags::WRITE;
        // SAFETY: a new shared mapping of the file, which aliases no Rust
        // memory; the advice applies to that mapping alone, and a failure of
        // it leaves the mapping as it was.
        let start = unsafe {
            let start = rustix::mm::mmap(
                std::ptr::null_mut(),
                len,
                read_write,
                MapFlags::SHARED,
                &fd,
                0,
            )
            .ok()?;
            let _ = rustix::mm::madvise(start, len, Advice::LinuxDontDump);
            let _ = rustix::mm::madvise(start, len, Advice::LinuxDontFork);
            start
        };
        Pages::mapped(start, len, Protection::Secret)
    }

    /// `len` bytes of anonymous memory, locked when `lock` says so and the
    /// process may.
    fn anonymous(len: usize, lock: bool) -> io::Result<Pages> {
        let read_write = ProtFlags::READ | ProtFlags::WRITE;
        // SAFETY: an anonymous mapping takes no file and aliases no Rust
        // memory; the other calls apply to it alone, and a failure of them
        // leaves it as it was.
        let (start, locked) = unsafe {
            let start = rustix::mm::mmap_anonymous(
                std::ptr::null_mut(),
                len,
                read_write,
                MapFlags::PRIVATE,
            )?;
            let locked = lock && rustix::mm::mlock(start, len).is_ok();
            let _ = rustix::mm::madvise(start, len, Advice::LinuxDontDump);
            let _ = rustix::mm::madvise(start, len, Advice::LinuxWipeOnFork);
            (start, locked)
        };
        let protection = if locked {
            Protection::Locked
        } else {
            Protection::Unlocked
        };
        Pages::mapped(start, len, protection).ok_or_else(|| io::ErrorKind::OutOfMemory.into())
    }

    /// The region `mmap` returned.
    fn mapped(start: *mut c_void, len: usize, protection: Protection) -> Option<Pages> {
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
        crate::wipe(self.bytes_mut());
        // SAFETY: the region is this value's mapping of `len` bytes, wiped
        // above; it is unmapped, and never used again.
        let _ = unsafe { rustix::mm::munmap(self.start.as_ptr().cast(), self.len) };
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real programs, files and time"
)]
mod tests {
    use super::*;

    /// Held by the tests that make regions: `cargo test` runs them side by
    /// side in one process, where one dropping its regions would change
    /// the count another reads ([`live_regions`]).
    static REGIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn a_process_can_make_itself_undumpable() {
        // Each test runs in a process of its own under nextest; under `cargo
        // test` the rest of the binary is undumpable too, which harms nothing.
        set_undumpable().unwrap();
        assert!(!is_dumpable());
    }

    #[test]
    fn regions_are_whole_pages_of_zeros_that_keep_what_is_written() {
        let _regions = REGIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _regions = REGIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _regions = REGIONS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
