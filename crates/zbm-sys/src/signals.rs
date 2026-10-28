//! Signals as data: the signals a program waits for are blocked and read
//! from a `signalfd` ([`Signals`]), and the processes it signals and waits
//! for are held by a pidfd ([`Process`]).
//!
//! A blocked signal stays pending until it is read, so nothing is lost and
//! no handler runs in the middle of the program: the program reads the
//! signals when it looks, or waits on the descriptor with the rest of its
//! input. This replaces the handlers that stored the last signal in a
//! global slot, and the self-pipes of `signal-hook`.
//!
//! The mask is a thread's: a signal sent to the process goes to a thread
//! that does not block it, and ends the process there when its action is
//! the default one. So a program blocks its signals first, before it
//! starts any thread; the threads it starts afterwards inherit the mask.
//! Programs inherit the mask, and the standard library's `Command` leaves
//! it as it is: [`prepare_child`](crate::prepare_child) empties it in the
//! child, so they get the signals as usual.

use std::ffi::{c_int, c_ulong};
use std::fs::File;
use std::io::{self, Read as _};
use std::os::fd::{AsFd, AsRawFd as _, BorrowedFd, FromRawFd as _, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::{Signal, console};

unsafe extern "C" {
    fn pthread_sigmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    fn signalfd(fd: c_int, mask: *const SigSet, flags: c_int) -> c_int;
}

/// `sigset_t`: 1024 bits in glibc and musl.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct SigSet([c_ulong; 1024 / (8 * std::mem::size_of::<c_ulong>())]);

impl SigSet {
    /// No signal, as `sigemptyset` leaves a set on Linux.
    pub(crate) const EMPTY: SigSet = SigSet([0; 1024 / (8 * std::mem::size_of::<c_ulong>())]);
}

/// `SIG_BLOCK`: adds to the mask.
const SIG_BLOCK: c_int = 0;

/// `SIG_SETMASK`: replaces the mask.
const SIG_SETMASK: c_int = 2;

/// Whether the calling thread blocks any signal, so that the programs it
/// starts would inherit the mask.
pub(crate) fn is_blocking_any() -> bool {
    let mut mask = SigSet::EMPTY;
    // SAFETY: no new mask is given; `mask` is a valid, writable `sigset_t`
    // for the old one.
    let error = unsafe { pthread_sigmask(SIG_BLOCK, std::ptr::null(), &mut mask) };
    error != 0 || mask.0.iter().any(|&word| word != 0)
}

/// Unblocks every signal in the calling thread, as a child does before
/// `exec`; `pthread_sigmask`'s result. Async-signal-safe, so it may run
/// between `fork` and `exec`.
pub(crate) fn unblock_all() -> c_int {
    // SAFETY: `SigSet::EMPTY` is a valid `sigset_t`; no old mask is asked
    // for. `pthread_sigmask` is async-signal-safe.
    unsafe { pthread_sigmask(SIG_SETMASK, &SigSet::EMPTY, std::ptr::null_mut()) }
}

/// `SFD_NONBLOCK` and `SFD_CLOEXEC`, as `O_NONBLOCK` and `O_CLOEXEC` on
/// x86 and Arm.
const SFD_FLAGS: c_int = 0o4000 | 0o2_000_000;

/// The size of `struct signalfd_siginfo`, whose first field is the signal's
/// number as a `u32`.
const SIGINFO_SIZE: usize = 128;

/// A set of [`Signal`]s.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SignalSet(u64);

impl SignalSet {
    /// No signal.
    pub const EMPTY: SignalSet = SignalSet(0);

    /// The set of `signals`.
    pub const fn of(signals: &[Signal]) -> SignalSet {
        let mut set = SignalSet::EMPTY;
        let mut index = 0;
        while index < signals.len() {
            set.0 |= bit(signals[index]);
            index += 1;
        }
        set
    }

    /// Whether `signal` is in the set.
    pub const fn contains(self, signal: Signal) -> bool {
        self.0 & bit(signal) != 0
    }

    /// Whether no signal is in the set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The signals of both sets.
    #[must_use]
    pub const fn union(self, other: SignalSet) -> SignalSet {
        SignalSet(self.0 | other.0)
    }

    /// The signals of the set, in the order of their numbers.
    pub fn iter(self) -> impl Iterator<Item = Signal> {
        super::SIGNALS
            .into_iter()
            .filter(move |&signal| self.contains(signal))
    }

    /// The first signal of the set, by number.
    pub fn first(self) -> Option<Signal> {
        self.iter().next()
    }

    /// The set as `sigset_t`, in its Linux layout, which glibc's and musl's
    /// `sigaddset` follow: signal `n` is bit `n - 1`, counted from the
    /// lowest bit of the first word.
    fn to_sigset(self) -> SigSet {
        let bits = 8 * std::mem::size_of::<c_ulong>();
        let mut set = SigSet::EMPTY;
        for signal in self.iter() {
            let index = usize::try_from(signal.number() - 1).unwrap_or(0);
            if let Some(word) = set.0.get_mut(index / bits) {
                *word |= 1 << (index % bits);
            }
        }
        set
    }
}

/// The bit of `signal` in a [`SignalSet`].
const fn bit(signal: Signal) -> u64 {
    // Linux numbers its signals from 1 to 64.
    1 << ((signal.number() - 1) as u32 & 63)
}

impl FromIterator<Signal> for SignalSet {
    fn from_iter<I: IntoIterator<Item = Signal>>(signals: I) -> SignalSet {
        signals.into_iter().fold(SignalSet::EMPTY, |set, signal| {
            set.union(SignalSet::of(&[signal]))
        })
    }
}

/// Signals blocked in this program and read from a `signalfd`.
///
/// Readable when one of them is pending, so that a loop waits on it with
/// its other descriptors; [`Signals::take`] reads them, and
/// [`Signals::received`] tells every signal read so far.
#[derive(Debug)]
pub struct Signals {
    /// The `signalfd`.
    file: File,
    /// The signals it reads.
    set: SignalSet,
    /// The signals read so far.
    received: AtomicU64,
}

impl Signals {
    /// Blocks the signals of `set` in the calling thread, and in the threads
    /// it starts from now on, and opens a descriptor that reads them. Call
    /// it before the program starts any thread (see the module's notes).
    ///
    /// # Errors
    ///
    /// When the signals cannot be blocked or the descriptor opened.
    pub fn block(set: SignalSet) -> io::Result<Signals> {
        let mask = set.to_sigset();
        // SAFETY: `mask` is a valid `sigset_t`, and no old mask is asked for;
        // -1 asks `signalfd` for a new descriptor, which nothing else owns
        // when it returns one.
        let fd = unsafe {
            let error = pthread_sigmask(SIG_BLOCK, &mask, std::ptr::null_mut());
            if error != 0 {
                return Err(io::Error::from_raw_os_error(error));
            }
            let fd = signalfd(-1, &mask, SFD_FLAGS);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            OwnedFd::from_raw_fd(fd)
        };
        Ok(Signals {
            file: File::from(fd),
            set,
            received: AtomicU64::new(0),
        })
    }

    /// The signals it reads.
    pub fn set(&self) -> SignalSet {
        self.set
    }

    /// Reads the signals pending now, without waiting; returns them, and
    /// keeps them for [`Signals::received`].
    ///
    /// # Errors
    ///
    /// When the descriptor cannot be read.
    pub fn take(&self) -> io::Result<SignalSet> {
        let mut taken = SignalSet::EMPTY;
        let mut info = [0_u8; SIGINFO_SIZE];
        loop {
            match (&self.file).read(&mut info) {
                Ok(SIGINFO_SIZE) => {
                    let number = info
                        .first_chunk::<4>()
                        .map_or(0, |bytes| u32::from_ne_bytes(*bytes));
                    let signal = i32::try_from(number).ok().and_then(Signal::from_number);
                    if let Some(signal) = signal {
                        taken = taken.union(SignalSet::of(&[signal]));
                    }
                }
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        self.received.fetch_or(taken.0, Ordering::SeqCst);
        Ok(taken)
    }

    /// Waits at most `timeout` (`None`: without limit) for a signal, then
    /// reads the signals pending, as [`Signals::take`] does.
    ///
    /// # Errors
    ///
    /// When the descriptor cannot be waited on or read.
    pub fn wait(&self, timeout: Option<Duration>) -> io::Result<SignalSet> {
        console::first_readable(&[self.file.as_raw_fd()], timeout)?;
        self.take()
    }

    /// Every signal read so far, with those pending now. A signal that
    /// cannot be read is left for the next call.
    pub fn received(&self) -> SignalSet {
        let _ = self.take();
        SignalSet(self.received.load(Ordering::SeqCst))
    }

    /// Forgets that `signal` was read, so that [`Signals::received`] tells
    /// only the next one: for a request that can come again.
    pub fn forget(&self, signal: Signal) {
        self.received.fetch_and(!bit(signal), Ordering::SeqCst);
    }
}

impl AsFd for Signals {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.file.as_fd()
    }
}

/// A process held by a pidfd: signals reach it, and not a process that
/// took its ID after it ended; it can be waited for without being this
/// process's child.
#[derive(Debug)]
pub struct Process {
    /// The pidfd, readable once the process ended.
    fd: OwnedFd,
}

impl Process {
    /// The process `pid`.
    ///
    /// # Errors
    ///
    /// When there is no such process (`ESRCH`), or the kernel has no pidfd
    /// (`ENOSYS`, before Linux 5.3).
    pub fn open(pid: u32) -> io::Result<Process> {
        let pid = i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let fd = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty())?;
        Ok(Process { fd })
    }

    /// Sends `signal` to it.
    ///
    /// # Errors
    ///
    /// When it ended (`ESRCH`), or the signal cannot be sent to it.
    pub fn signal(&self, signal: Signal) -> io::Result<()> {
        let signal = rustix::process::Signal::from_named_raw(signal.number())
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(rustix::process::pidfd_send_signal(&self.fd, signal)?)
    }

    /// Waits at most `timeout` (`None`: without limit) for it to end;
    /// whether it did.
    ///
    /// # Errors
    ///
    /// When the pidfd cannot be waited on.
    pub fn wait_exit(&self, timeout: Option<Duration>) -> io::Result<bool> {
        let ready = console::first_readable(&[self.fd.as_raw_fd()], timeout)?;
        Ok(ready.is_some())
    }

    /// Whether it ended, without waiting.
    ///
    /// # Errors
    ///
    /// When the pidfd cannot be polled.
    pub fn has_exited(&self) -> io::Result<bool> {
        self.wait_exit(Some(Duration::ZERO))
    }
}

impl AsFd for Process {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests may panic")]
mod tests {
    use super::*;

    #[test]
    fn sets_hold_the_signals_they_are_made_of() {
        let set = SignalSet::of(&[Signal::Terminate, Signal::Hangup]);

        assert!(set.contains(Signal::Hangup));
        assert!(set.contains(Signal::Terminate));
        assert!(!set.contains(Signal::User1));
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [Signal::Hangup, Signal::Terminate]
        );
        assert_eq!(set.first(), Some(Signal::Hangup));
        assert!(SignalSet::EMPTY.is_empty());
        assert_eq!(SignalSet::EMPTY.first(), None);
        let union = set.union(SignalSet::of(&[Signal::User2]));
        assert!(union.contains(Signal::User2) && union.contains(Signal::Hangup));
        let collected: SignalSet = [Signal::User1, Signal::User2].into_iter().collect();
        assert_eq!(collected, SignalSet::of(&[Signal::User2, Signal::User1]));
        // SIGHUP is bit 0, SIGTERM bit 14.
        assert_eq!(set.to_sigset().0.first(), Some(&0x4001));
    }

    #[test]
    fn a_process_is_signalled_and_waited_for_through_its_pidfd() {
        let _guard = crate::tests::PROCESSES.lock().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let process = Process::open(child.id()).unwrap();

        assert!(!process.has_exited().unwrap());
        assert!(!process.wait_exit(Some(Duration::from_millis(20))).unwrap());
        process.signal(Signal::Terminate).unwrap();
        assert!(process.wait_exit(Some(Duration::from_secs(10))).unwrap());
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(child.wait().unwrap().signal(), Some(15));
        // Collected: the process is gone, and the pidfd says so.
        assert_eq!(
            process
                .signal(Signal::Terminate)
                .unwrap_err()
                .raw_os_error(),
            Some(3)
        );
        assert!(Process::open(child.id()).is_err());
        assert!(Process::open(0).is_err());
        assert!(Process::open(u32::MAX).is_err());
    }
}
