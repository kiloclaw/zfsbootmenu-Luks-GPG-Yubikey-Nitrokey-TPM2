//! The system calls that the Rust standard library does not offer.
//!
//! The legacy tools are Bash and Perl programs, and both react to signals and
//! to the effective user in ways that the Rust standard library cannot
//! reproduce: Rust ignores `SIGPIPE` at start-up, has no signal handlers, and
//! cannot tell the effective user and groups of the process. The runtime also
//! sends signals to other processes, wipes secrets from memory and reads the
//! stack for crash reports ([`return_addresses`]), and its processes talk
//! over sockets of packets ([`socket`]); the menu on the display takes over
//! the virtual terminal's modes ([`console`]).
//!
//! Most of them are `rustix`'s safe calls, on borrowed descriptors. What
//! `rustix` has no safe call for stays here, `unsafe`, each block stating why
//! it is sound: signal dispositions, the signal mask and `signalfd`, the
//! terminal and input device `ioctl`s (through `rustix`'s typed `ioctl`),
//! memory mappings, `memfd_secret`, `explicit_bzero`, a child's set-up
//! between `fork` and `exec`, and the unwinder. It is the only crate in the
//! workspace that may contain `unsafe` code.

#![allow(unsafe_code)]

pub mod console;
pub mod memory;
mod process;
pub mod signals;
pub mod socket;
mod stack;

pub use process::{
    ChildSetup, Ended, Session, prepare_child, reap, take_terminal, try_wait, wait_any,
};
pub use signals::{Process, SignalSet, Signals};
pub use stack::{MAX_FRAMES, return_addresses};

use std::ffi::{c_int, c_void};
use std::os::fd::AsFd;

unsafe extern "C" {
    // `sighandler_t signal(int signum, sighandler_t handler)`; the handler is
    // passed as an address, so that the default action (`SIG_DFL`, address 0) and
    // real handlers can share one declaration.
    fn signal(signum: c_int, handler: usize) -> usize;
    // glibc 2.25 and musl 1.1.20 provide it: a `memset` to zero that the
    // compiler may not remove, even though the memory is not read afterwards.
    fn explicit_bzero(s: *mut c_void, n: usize);
}

/// The value `signal` returns when it fails.
const SIG_ERR: usize = usize::MAX;

/// The default action of a signal.
const SIG_DFL: usize = 0;

/// The disposition that ignores a signal.
const SIG_IGN: usize = 1;

/// The signals ZFSBootMenu handles or sends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    /// `SIGHUP`, sent when the terminal goes away, such as a dropped SSH
    /// session.
    Hangup,
    /// `SIGINT`, sent by the terminal on Ctrl-C.
    Interrupt,
    /// `SIGQUIT`, sent by the terminal on Ctrl-\\.
    Quit,
    /// `SIGKILL`, which cannot be caught: for a program past its time.
    Kill,
    /// `SIGUSR1`, which tells a running menu to give way to another
    /// instance, when the session's socket cannot.
    User1,
    /// `SIGUSR2`, which the runtime no longer sends: the console takes its
    /// requests on its control socket.
    User2,
    /// `SIGPIPE`, sent when writing to a pipe that nobody reads.
    Pipe,
    /// `SIGTERM`, the default signal of `kill`.
    Terminate,
    /// `SIGCHLD`, sent when a child process ends: process 1 reads it to
    /// collect the orphans that come to it.
    Child,
}

/// Every [`Signal`], in the order of their numbers.
const SIGNALS: [Signal; 9] = [
    Signal::Hangup,
    Signal::Interrupt,
    Signal::Quit,
    Signal::Kill,
    Signal::User1,
    Signal::User2,
    Signal::Pipe,
    Signal::Terminate,
    Signal::Child,
];

impl Signal {
    /// The signal number on Linux, which is the same on every supported
    /// architecture.
    pub const fn number(self) -> i32 {
        match self {
            Signal::Hangup => 1,
            Signal::Interrupt => 2,
            Signal::Quit => 3,
            Signal::Kill => 9,
            Signal::User1 => 10,
            Signal::User2 => 12,
            Signal::Pipe => 13,
            Signal::Terminate => 15,
            Signal::Child => 17,
        }
    }

    /// The name Perl uses for the signal, as in `$SIG{INT}`.
    pub fn name(self) -> &'static str {
        match self {
            Signal::Hangup => "HUP",
            Signal::Interrupt => "INT",
            Signal::Quit => "QUIT",
            Signal::Kill => "KILL",
            Signal::User1 => "USR1",
            Signal::User2 => "USR2",
            Signal::Pipe => "PIPE",
            Signal::Terminate => "TERM",
            Signal::Child => "CHLD",
        }
    }

    fn from_number(number: i32) -> Option<Signal> {
        SIGNALS.into_iter().find(|signal| signal.number() == number)
    }
}

extern "C" fn discard(_: c_int) {}

fn set_handler(signal: Signal, handler: usize) -> std::io::Result<()> {
    // SAFETY: `signal` only changes the disposition of the given signal. The
    // handler is `SIG_DFL`, `SIG_IGN`, or `discard`, which has the C ABI and
    // does nothing.
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
/// `handler` must be `SIG_DFL`, `SIG_IGN` or the address of an
/// `extern "C" fn(c_int)` that is async-signal-safe.
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
///
/// # Errors
///
/// When the kernel refuses the signal number, which it does not for these.
pub fn restore_default_sigpipe() -> std::io::Result<()> {
    set_handler(Signal::Pipe, SIG_DFL)
}

/// Ignores `signal`, as `trap '' SIG` does in the shell. Programs this process
/// starts inherit the ignored disposition.
///
/// # Errors
///
/// When the kernel refuses the signal number, which it does not for these.
pub fn ignore_signal(signal: Signal) -> std::io::Result<()> {
    set_handler(signal, SIG_IGN)
}

/// Discards `signal` in this process, as Bash does with `SIGQUIT`: it is
/// caught and nothing happens. Unlike an ignored signal, programs this
/// process starts get the default action back.
///
/// # Errors
///
/// When the kernel refuses the signal number, which it does not for these.
pub fn discard_signal(signal: Signal) -> std::io::Result<()> {
    set_handler(signal, discard as extern "C" fn(c_int) as usize)
}

/// Restores the default action of `signal` in this process.
///
/// # Errors
///
/// When the kernel refuses the signal number, which it does not for these.
pub fn restore_default(signal: Signal) -> std::io::Result<()> {
    set_handler(signal, SIG_DFL)
}

/// Sends `signal` to the process `pid`, as `kill -SIG pid` does.
///
/// # Errors
///
/// `InvalidInput` for 0 or a number beyond process IDs, which would signal
/// process groups; `ESRCH` when no such process exists; `EPERM` when this
/// process may not signal it.
pub fn send_signal(pid: u32, signal: Signal) -> std::io::Result<()> {
    // Process IDs above `i32::MAX` do not exist; refusing them also keeps the
    // value from turning negative, which would signal a process group.
    let pid =
        c_int::try_from(pid).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    if pid == 0 {
        // 0 would signal the caller's own process group.
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let pid = rustix::process::Pid::from_raw(pid).ok_or(std::io::ErrorKind::InvalidInput)?;
    let signal = rustix::process::Signal::from_named_raw(signal.number())
        .ok_or(std::io::ErrorKind::InvalidInput)?;
    Ok(rustix::process::kill_process(pid, signal)?)
}

/// Waits until `fd` has data to read, or end of file, for at most `timeout`
/// (`None`: without limit). Returns whether it is readable, as Bash's
/// `read -t` decides whether to read.
///
/// # Errors
///
/// When `poll` fails other than by a signal.
pub fn wait_readable(fd: impl AsFd, timeout: Option<std::time::Duration>) -> std::io::Result<bool> {
    console::first_readable(&[fd.as_fd()], timeout).map(|ready| ready.is_some())
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

/// Overwrites the whole allocation of `vec` with `blank`, the capacity beyond
/// its length included, and empties it: for vectors of values that tell a
/// secret, such as the keys typed at a passphrase prompt.
pub fn wipe_values<T: Copy>(vec: &mut Vec<T>, blank: T) {
    vec.clear();
    for slot in vec.spare_capacity_mut() {
        // SAFETY: `slot` is a valid, aligned place for a `T` in the vector's
        // allocation; a volatile write is not optimised away, and the vector's
        // length stays 0, so no value it holds changes.
        unsafe { std::ptr::write_volatile(slot.as_mut_ptr(), blank) }
    }
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
    use rustix::process::{Gid, getegid, geteuid, getgroups};
    Credentials {
        uid: geteuid().as_raw(),
        gid: getegid().as_raw(),
        groups: getgroups()
            .map(|groups| groups.into_iter().map(Gid::as_raw).collect())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real programs, files and time"
)]
mod tests {
    use super::*;

    #[test]
    fn values_are_wiped_beyond_the_length_too() {
        let mut values = vec![7_u16, 8, 9];
        values.truncate(1);
        wipe_values(&mut values, 0);
        assert!(values.is_empty());
        // SAFETY (test): the capacity holds 3 values written above.
        let all = unsafe { std::slice::from_raw_parts(values.as_ptr(), 3) };
        assert_eq!(all, [0, 0, 0]);
    }
    use std::time::{Duration, Instant};

    /// Held by every test that starts processes or changes dispositions, so that
    /// `wait_any` cannot collect another test's child and dispositions do not
    /// change under another test.
    pub(crate) static PROCESSES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ignored_mask(process: &mut std::process::Command) -> u64 {
        let output = process
            .args(["SigIgn", "/proc/self/status"])
            .stdout(std::process::Stdio::piped())
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        u64::from_str_radix(text.trim().trim_start_matches("SigIgn:").trim(), 16).unwrap()
    }

    #[test]
    fn children_get_default_signal_actions_back() {
        let _guard = PROCESSES.lock().unwrap();
        let interrupt = 1 << (Signal::Interrupt.number() - 1);
        ignore_signal(Signal::Interrupt).unwrap();
        let inherited = ignored_mask(&mut std::process::Command::new("grep"));
        let mut restored = std::process::Command::new("grep");
        let setup = ChildSetup {
            default_signals: vec![Signal::Interrupt],
            ..ChildSetup::default()
        };
        prepare_child(&mut restored, &setup);
        let restored = ignored_mask(&mut restored);
        restore_default(Signal::Interrupt).unwrap();
        assert_ne!(inherited & interrupt, 0, "{inherited:x}");
        assert_eq!(restored & interrupt, 0, "{restored:x}");
    }

    #[test]
    fn a_discarded_signal_is_survived_and_not_inherited() {
        let _guard = PROCESSES.lock().unwrap();
        let quit = 1 << (Signal::Quit.number() - 1);
        discard_signal(Signal::Quit).unwrap();
        // Caught by `discard`, which does nothing: the tests go on.
        let raised = send_signal(std::process::id(), Signal::Quit);
        let inherited = ignored_mask(&mut std::process::Command::new("grep"));
        restore_default(Signal::Quit).unwrap();
        assert!(raised.is_ok());
        assert_eq!(inherited & quit, 0, "{inherited:x}");
        assert_eq!(Signal::Quit.name(), "QUIT");
        assert_eq!(Signal::from_number(3), Some(Signal::Quit));
    }

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
        assert_eq!(Signal::from_number(9), Some(Signal::Kill));
        assert_eq!(Signal::Kill.name(), "KILL");
        assert_eq!(Signal::from_number(31), None);
        assert_eq!(Signal::Hangup.number(), 1);
        assert_eq!(Signal::Hangup.name(), "HUP");
        assert_eq!(Signal::User2.number(), 12);
        assert_eq!(Signal::from_number(12), Some(Signal::User2));
        assert_eq!(Signal::from_number(1), Some(Signal::Hangup));
        assert_eq!(Signal::Child.number(), 17);
        assert_eq!(Signal::from_number(17), Some(Signal::Child));
        assert_eq!(Signal::Child.name(), "CHLD");
    }

    #[test]
    fn sends_signals_to_other_processes() {
        let _guard = PROCESSES.lock().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        send_signal(child.id(), Signal::Terminate).unwrap();
        let status = child.wait().unwrap();
        assert_eq!(
            std::os::unix::process::ExitStatusExt::signal(&status),
            Some(15)
        );
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
        use std::process::{Command, Stdio};
        let _guard = PROCESSES.lock().unwrap();

        // A child that keeps a pipe open but writes nothing until told to.
        let mut child = Command::new("sh")
            .args(["-c", "read line; echo \"$line\""])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let start = Instant::now();
        assert!(!wait_readable(&output, Some(Duration::from_millis(50))).unwrap());
        assert!(start.elapsed() >= Duration::from_millis(50));

        let mut input = child.stdin.take().unwrap();
        input.write_all(b"go\n").unwrap();
        assert!(wait_readable(&output, None).unwrap());
        drop(input);
        child.wait().unwrap();
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
