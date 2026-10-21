//! The few C library calls that the Rust standard library does not offer.
//!
//! The legacy tools are Bash and Perl programs, and both react to signals and
//! to the effective user in ways that the Rust standard library cannot
//! reproduce: Rust ignores `SIGPIPE` at start-up, has no signal handlers, and
//! cannot tell the effective user and groups of the process. The runtime also
//! sends signals to other processes and wipes secrets from memory. This crate
//! wraps the C functions that provide them. It is the only crate in the
//! workspace that may contain `unsafe` code, and each `unsafe` block states
//! why it is sound.

#![allow(unsafe_code)]

use std::ffi::{c_int, c_ulong, c_void};
use std::sync::atomic::{AtomicI32, Ordering};

unsafe extern "C" {
    // `sighandler_t signal(int signum, sighandler_t handler)`; the handler is
    // passed as an address, so that the default action (`SIG_DFL`, address 0) and
    // real handlers can share one declaration.
    fn signal(signum: c_int, handler: usize) -> usize;
    fn geteuid() -> u32;
    fn getegid() -> u32;
    fn getgroups(size: c_int, list: *mut u32) -> c_int;
    fn kill(pid: c_int, sig: c_int) -> c_int;
    // `nfds_t` is `unsigned long` in glibc and musl.
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    // glibc 2.25 and musl 1.1.20 provide it: a `memset` to zero that the
    // compiler may not remove, even though the memory is not read afterwards.
    fn explicit_bzero(s: *mut c_void, n: usize);
}

/// `struct pollfd`, which has this layout on every Linux architecture.
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// `POLLIN`: data to read.
const POLLIN: i16 = 1;

/// The value `signal` returns when it fails.
const SIG_ERR: usize = usize::MAX;

/// The default action of a signal.
const SIG_DFL: usize = 0;

/// The signals ZFSBootMenu handles or sends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    /// `SIGINT`, sent by the terminal on Ctrl-C.
    Interrupt,
    /// `SIGUSR1`, which tells a running menu to give way to another instance.
    User1,
    /// `SIGPIPE`, sent when writing to a pipe that nobody reads.
    Pipe,
    /// `SIGTERM`, the default signal of `kill`.
    Terminate,
}

/// Every [`Signal`].
const SIGNALS: [Signal; 4] = [
    Signal::Interrupt,
    Signal::User1,
    Signal::Pipe,
    Signal::Terminate,
];

impl Signal {
    /// The signal number on Linux, which is the same on every supported
    /// architecture.
    pub fn number(self) -> i32 {
        match self {
            Signal::Interrupt => 2,
            Signal::User1 => 10,
            Signal::Pipe => 13,
            Signal::Terminate => 15,
        }
    }

    /// The name Perl uses for the signal, as in `$SIG{INT}`.
    pub fn name(self) -> &'static str {
        match self {
            Signal::Interrupt => "INT",
            Signal::User1 => "USR1",
            Signal::Pipe => "PIPE",
            Signal::Terminate => "TERM",
        }
    }

    fn from_number(number: i32) -> Option<Signal> {
        SIGNALS.into_iter().find(|signal| signal.number() == number)
    }
}

/// The last signal caught by [`record_signals`], or 0.
static RECEIVED: AtomicI32 = AtomicI32::new(0);

extern "C" fn record(signum: c_int) {
    // Storing to an atomic integer is async-signal-safe.
    RECEIVED.store(signum, Ordering::SeqCst);
}

fn set_handler(signal: Signal, handler: usize) -> std::io::Result<()> {
    // SAFETY: `signal` only changes the disposition of the given signal. The
    // handler is either `SIG_DFL` or `record`, which has the C ABI and only
    // performs an async-signal-safe atomic store.
    let previous = unsafe { signal_ffi(signal.number(), handler) };
    if previous == SIG_ERR {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Calls the C `signal` function; kept separate so the name does not clash with
/// [`Signal`].
///
/// # Safety
///
/// `handler` must be `SIG_DFL` or the address of an `extern "C" fn(c_int)` that
/// is async-signal-safe.
unsafe fn signal_ffi(signum: c_int, handler: usize) -> usize {
    // SAFETY: the caller guarantees that `handler` is valid; `signal` has no other
    // requirements.
    unsafe { signal(signum, handler) }
}

/// Restores the default action of `SIGPIPE`, which the Rust runtime ignores.
///
/// Bash and Perl programs end silently when they write to a closed pipe (for
/// example in `zbm-kcl | head -n1`); with the default action restored, the Rust
/// tools do too.
pub fn restore_default_sigpipe() -> std::io::Result<()> {
    set_handler(Signal::Pipe, SIG_DFL)
}

/// Catches the given signals and records them instead of ending the process.
///
/// A caught signal does not interrupt the program: system calls are restarted,
/// and child processes still receive the default action, because handlers are
/// reset when a program is executed. Call [`received_signal`] to find out
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

/// Sends `signal` to the process `pid`, as `kill -SIG pid` does.
pub fn send_signal(pid: u32, signal: Signal) -> std::io::Result<()> {
    // Process IDs above `i32::MAX` do not exist; refusing them also keeps the
    // value from turning negative, which would signal a process group.
    let pid =
        c_int::try_from(pid).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    if pid == 0 {
        // 0 would signal the caller's own process group.
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    // SAFETY: `kill` takes two integers and touches no memory of this process.
    if unsafe { kill(pid, signal.number()) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Waits until `fd` has data to read, or end of file, for at most `timeout`
/// (`None`: without limit). Returns whether it is readable, as Bash's
/// `read -t` decides whether to read.
pub fn wait_readable(fd: i32, timeout: Option<std::time::Duration>) -> std::io::Result<bool> {
    // Milliseconds, rounded up so that a short wait still waits; -1 waits without
    // limit.
    let timeout = match timeout {
        None => -1,
        Some(timeout) => {
            let millis = timeout.as_nanos().div_ceil(1_000_000);
            c_int::try_from(millis).unwrap_or(c_int::MAX)
        }
    };
    let mut entry = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: `entry` is a valid `struct pollfd` that lives across the call,
        // and the count says there is exactly one.
        let ready = unsafe { poll(&mut entry, 1, timeout) };
        match ready {
            0 => return Ok(false),
            ready if ready > 0 => return Ok(true),
            _ => {
                let error = std::io::Error::last_os_error();
                if error.kind() != std::io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
}

/// Overwrites `bytes` with zeros in a way the compiler cannot optimise away,
/// so that secrets do not stay in memory after use.
pub fn wipe(bytes: &mut [u8]) {
    // SAFETY: the pointer and length come from a valid mutable slice, so
    // `explicit_bzero` writes only to memory this function may write.
    unsafe { explicit_bzero(bytes.as_mut_ptr().cast(), bytes.len()) }
}

/// Overwrites the whole allocation of `vec` with zeros, including the capacity
/// beyond its length, and empties it.
pub fn wipe_vec(vec: &mut Vec<u8>) {
    let capacity = vec.capacity();
    // SAFETY: a `Vec` owns an allocation of `capacity` bytes starting at its
    // pointer (none when the capacity is 0, where nothing is written). Zero is
    // a valid `u8`, and the length is set to 0 afterwards, so no byte becomes
    // readable that was not initialised before.
    unsafe { explicit_bzero(vec.as_mut_ptr().cast(), capacity) }
    vec.clear();
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

/// Returns the effective credentials of the process, which decide file access
/// as Perl's `-r` and `-x` tests do.
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
    // SAFETY: with a size of 0, `getgroups` only returns the number of groups and
    // does not write to the list.
    let count = unsafe { getgroups(0, std::ptr::null_mut()) };
    let Ok(capacity) = usize::try_from(count) else {
        return Vec::new();
    };
    let mut groups = vec![0_u32; capacity];
    // SAFETY: `groups` holds `count` elements, so `getgroups` writes at most
    // `count` IDs into valid memory.
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
        assert_eq!(Signal::User1.number(), 10);
        assert_eq!(Signal::User1.name(), "USR1");
        assert_eq!(Signal::from_number(10), Some(Signal::User1));
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
    fn sends_signals_to_other_processes() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        send_signal(child.id(), Signal::Terminate).unwrap();
        let status = child.wait().unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(15));
        // The process is gone now.
        let error = send_signal(child.id(), Signal::Terminate).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(3));
        assert_eq!(
            send_signal(0, Signal::Terminate).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            send_signal(u32::MAX, Signal::Terminate).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn waits_for_readable_input() {
        use std::io::Write;
        use std::os::fd::AsRawFd;
        use std::process::{Command, Stdio};

        // A child that keeps a pipe open but writes nothing until told to.
        let mut child = Command::new("sh")
            .args(["-c", "read line; echo \"$line\""])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let fd = output.as_raw_fd();
        let start = Instant::now();
        assert!(!wait_readable(fd, Some(Duration::from_millis(50))).unwrap());
        assert!(start.elapsed() >= Duration::from_millis(50));

        let mut input = child.stdin.take().unwrap();
        input.write_all(b"go\n").unwrap();
        assert!(wait_readable(fd, None).unwrap());
        drop(input);
        child.wait().unwrap();
        drop(output);

        assert!(wait_readable(-1, Some(Duration::ZERO)).is_ok());
    }

    #[test]
    fn wipes_slices_and_whole_allocations() {
        let mut bytes = *b"secret";
        wipe(&mut bytes);
        assert_eq!(bytes, [0; 6]);

        let mut vec = Vec::with_capacity(16);
        vec.extend_from_slice(b"secret");
        let pointer = vec.as_ptr();
        wipe_vec(&mut vec);
        assert!(vec.is_empty());
        assert_eq!(vec.capacity(), 16);
        // The allocation is kept, so its old contents can be inspected through
        // the spare capacity.
        assert_eq!(vec.as_ptr(), pointer);
        let spare: Vec<u8> = vec
            .spare_capacity_mut()
            .iter()
            // SAFETY: `wipe_vec` wrote zeros to the whole allocation, so every
            // byte is initialised.
            .map(|byte| unsafe { byte.assume_init() })
            .collect();
        assert_eq!(spare, [0; 16]);

        let mut empty = Vec::new();
        wipe_vec(&mut empty);
        assert!(empty.is_empty());
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
