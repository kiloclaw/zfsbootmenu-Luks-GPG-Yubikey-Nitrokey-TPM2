//! The few C library calls that the Rust standard library does not offer.
//!
//! The legacy tools are Bash and Perl programs, and both react to signals and to the effective user in ways that the
//! Rust standard library cannot reproduce: Rust ignores `SIGPIPE` at start-up, has no signal handlers, and cannot
//! tell the effective user and groups of the process. This crate wraps the C functions that provide them. It is the
//! only crate in the workspace that may contain `unsafe` code, and each `unsafe` block states why it is sound.

#![allow(unsafe_code)]

use std::ffi::c_int;
use std::sync::atomic::{AtomicI32, Ordering};

unsafe extern "C" {
    // `sighandler_t signal(int signum, sighandler_t handler)`; the handler is passed as an address, so that the
    // default action (`SIG_DFL`, address 0) and real handlers can share one declaration.
    fn signal(signum: c_int, handler: usize) -> usize;
    fn geteuid() -> u32;
    fn getegid() -> u32;
    fn getgroups(size: c_int, list: *mut u32) -> c_int;
}

/// The value `signal` returns when it fails.
const SIG_ERR: usize = usize::MAX;

/// The default action of a signal.
const SIG_DFL: usize = 0;

/// The signals the host tools handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    /// `SIGINT`, sent by the terminal on Ctrl-C.
    Interrupt,
    /// `SIGPIPE`, sent when writing to a pipe that nobody reads.
    Pipe,
    /// `SIGTERM`, the default signal of `kill`.
    Terminate,
}

impl Signal {
    /// The signal number on Linux, which is the same on every supported architecture.
    pub fn number(self) -> i32 {
        match self {
            Signal::Interrupt => 2,
            Signal::Pipe => 13,
            Signal::Terminate => 15,
        }
    }

    /// The name Perl uses for the signal, as in `$SIG{INT}`.
    pub fn name(self) -> &'static str {
        match self {
            Signal::Interrupt => "INT",
            Signal::Pipe => "PIPE",
            Signal::Terminate => "TERM",
        }
    }

    fn from_number(number: i32) -> Option<Signal> {
        [Signal::Interrupt, Signal::Pipe, Signal::Terminate]
            .into_iter()
            .find(|signal| signal.number() == number)
    }
}

/// The last signal caught by [`record_signals`], or 0.
static RECEIVED: AtomicI32 = AtomicI32::new(0);

extern "C" fn record(signum: c_int) {
    // Storing to an atomic integer is async-signal-safe.
    RECEIVED.store(signum, Ordering::SeqCst);
}

fn set_handler(signal: Signal, handler: usize) -> std::io::Result<()> {
    // SAFETY: `signal` only changes the disposition of the given signal. The handler is either `SIG_DFL` or
    // `record`, which has the C ABI and only performs an async-signal-safe atomic store.
    let previous = unsafe { signal_ffi(signal.number(), handler) };
    if previous == SIG_ERR {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Calls the C `signal` function; kept separate so the name does not clash with [`Signal`].
///
/// # Safety
///
/// `handler` must be `SIG_DFL` or the address of an `extern "C" fn(c_int)` that is async-signal-safe.
unsafe fn signal_ffi(signum: c_int, handler: usize) -> usize {
    // SAFETY: the caller guarantees that `handler` is valid; `signal` has no other requirements.
    unsafe { signal(signum, handler) }
}

/// Restores the default action of `SIGPIPE`, which the Rust runtime ignores.
///
/// Bash and Perl programs end silently when they write to a closed pipe (for example in `zbm-kcl | head -n1`); with
/// the default action restored, the Rust tools do too.
pub fn restore_default_sigpipe() -> std::io::Result<()> {
    set_handler(Signal::Pipe, SIG_DFL)
}

/// Catches the given signals and records them instead of ending the process.
///
/// A caught signal does not interrupt the program: system calls are restarted, and child processes still receive the
/// default action, because handlers are reset when a program is executed. Call [`received_signal`] to find out
/// whether one arrived.
pub fn record_signals(signals: &[Signal]) -> std::io::Result<()> {
    signals
        .iter()
        .try_for_each(|&signal| set_handler(signal, record as extern "C" fn(c_int) as usize))
}

/// The last signal recorded by [`record_signals`], if any.
pub fn received_signal() -> Option<Signal> {
    Signal::from_number(RECEIVED.load(Ordering::SeqCst))
}

/// The effective user, group and supplementary groups of the process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Credentials {
    /// The effective user ID.
    pub uid: u32,
    /// The effective group ID.
    pub gid: u32,
    /// The supplementary group IDs.
    pub groups: Vec<u32>,
}

/// Returns the effective credentials of the process, which decide file access as Perl's `-r` and `-x` tests do.
pub fn credentials() -> Credentials {
    // SAFETY: `geteuid` and `getegid` take no arguments and cannot fail.
    let (uid, gid) = unsafe { (geteuid(), getegid()) };
    Credentials {
        uid,
        gid,
        groups: supplementary_groups(),
    }
}

fn supplementary_groups() -> Vec<u32> {
    // SAFETY: with a size of 0, `getgroups` only returns the number of groups and does not write to the list.
    let count = unsafe { getgroups(0, std::ptr::null_mut()) };
    let Ok(capacity) = usize::try_from(count) else {
        return Vec::new();
    };
    let mut groups = vec![0_u32; capacity];
    // SAFETY: `groups` holds `count` elements, so `getgroups` writes at most `count` IDs into valid memory.
    let written = unsafe { getgroups(count, groups.as_mut_ptr()) };
    match usize::try_from(written) {
        Ok(written) => {
            groups.truncate(written);
            groups
        }
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn signal_numbers_and_names_match_linux_and_perl() {
        assert_eq!(Signal::Interrupt.number(), 2);
        assert_eq!(Signal::Pipe.number(), 13);
        assert_eq!(Signal::Terminate.number(), 15);
        assert_eq!(Signal::Interrupt.name(), "INT");
        assert_eq!(Signal::Terminate.name(), "TERM");
        assert_eq!(Signal::from_number(15), Some(Signal::Terminate));
        assert_eq!(Signal::from_number(9), None);
    }

    #[test]
    fn recorded_signal_does_not_end_the_process() {
        record_signals(&[Signal::Terminate]).unwrap();
        let status = std::process::Command::new("kill")
            .args(["-TERM", &std::process::id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(5);
        while received_signal() != Some(Signal::Terminate) {
            assert!(Instant::now() < deadline, "SIGTERM was not recorded");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn credentials_match_proc_status() {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let field = |name: &str| -> Vec<u32> {
            let line = status.lines().find(|line| line.starts_with(name)).unwrap();
            line[name.len()..]
                .split_whitespace()
                .map(|id| id.parse().unwrap())
                .collect()
        };
        let credentials = credentials();
        // The second ID on the Uid: and Gid: lines is the effective one.
        assert_eq!(credentials.uid, field("Uid:")[1]);
        assert_eq!(credentials.gid, field("Gid:")[1]);
        let mut expected = field("Groups:");
        let mut groups = credentials.groups;
        expected.sort_unstable();
        groups.sort_unstable();
        assert_eq!(groups, expected);
    }
}
