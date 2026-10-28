//! Requests to the console from other processes: `zbm console release`,
//! before `kexec`, sends `SIGUSR1` and waits until the console has given
//! the display, the keyboards and the virtual terminal back to the kernel;
//! `zbm console layout` sends `SIGUSR2` and waits until it has taken the
//! keyboard layout asked for.

#![allow(
    clippy::disallowed_methods,
    reason = "waiting on another process, the console, takes real time"
)]

use std::io;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use zbm_sys::{Process, Signal, SignalSet, Signals};

/// How often the request looks whether the console is done.
const POLL: Duration = Duration::from_millis(20);

/// The signals of the requests: `SIGUSR1` for the display, `SIGUSR2` for a
/// keyboard layout.
const REQUESTS: SignalSet = SignalSet::of(&[Signal::User1, Signal::User2]);

/// The requests other processes send the console.
#[derive(Clone, Debug, Default)]
pub struct Requests {
    /// Set when the display must be handed back.
    pub release: Arc<AtomicBool>,
    /// Set when another keyboard layout is asked for.
    pub layout: Arc<AtomicBool>,
    /// The signals that bring them, blocked and read here; readable when
    /// one came, so that the console waits on them instead of looking for
    /// requests on every tick. `None` takes no request but those set by
    /// hand.
    pub signals: Option<Arc<Signals>>,
}

impl Requests {
    /// The requests of this process, which blocks their signals. Call it
    /// before the process starts any thread.
    ///
    /// # Errors
    ///
    /// When the signals cannot be blocked.
    pub fn listen() -> io::Result<Requests> {
        Ok(Requests {
            signals: Some(Arc::new(Signals::block(REQUESTS)?)),
            ..Requests::default()
        })
    }

    /// Takes the requests whose signals came: sets [`Requests::release`]
    /// and [`Requests::layout`] for them.
    pub fn take(&self) {
        let Some(taken) = self
            .signals
            .as_ref()
            .and_then(|signals| signals.take().ok())
        else {
            return;
        };
        if taken.contains(Signal::User1) {
            self.release.store(true, Ordering::SeqCst);
        }
        if taken.contains(Signal::User2) {
            self.layout.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the display was asked for, which the request is taken
    /// with: the relay after the hand-over takes it again when asked.
    pub fn was_release_requested(&self) -> bool {
        self.take();
        self.release.swap(false, Ordering::SeqCst)
    }
}

/// What wakes the console when a request comes: readable then; taking it
/// records the requests and leaves it unreadable.
pub trait Wake {
    /// The descriptor to wait on.
    fn fd(&self) -> BorrowedFd<'_>;

    /// Takes what made it readable.
    fn take(&mut self);
}

/// The wake-up of [`Requests`] with signals: their descriptor.
#[derive(Debug)]
pub struct SignalWake {
    /// The requests it takes.
    requests: Requests,
    /// Their signals.
    signals: Arc<Signals>,
}

impl Requests {
    /// What wakes the console when a request comes; `None` without
    /// signals.
    pub fn wake(&self) -> Option<SignalWake> {
        let signals = Arc::clone(self.signals.as_ref()?);
        Some(SignalWake {
            requests: self.clone(),
            signals,
        })
    }
}

impl Wake for SignalWake {
    fn fd(&self) -> BorrowedFd<'_> {
        self.signals.as_fd()
    }

    fn take(&mut self) {
        self.requests.take();
    }
}

/// Asks the console with process ID `pid` to hand the display back, and
/// waits at most `timeout` until `is_released` says it did. Returns
/// whether it did; a console that is gone has nothing to hand back.
///
/// # Errors
///
/// When the signal cannot be sent.
pub fn request(
    pid: u32,
    timeout: Duration,
    is_released: &mut dyn FnMut() -> bool,
) -> io::Result<bool> {
    send(pid, Signal::User1, timeout, is_released)
}

/// Asks the console with process ID `pid` for the keyboard layout written
/// in the state directory, and waits at most `timeout` until `is_taken`
/// says it took the request. Returns whether it did, or is gone.
///
/// # Errors
///
/// When the signal cannot be sent.
pub fn request_layout(
    pid: u32,
    timeout: Duration,
    is_taken: &mut dyn FnMut() -> bool,
) -> io::Result<bool> {
    send(pid, Signal::User2, timeout, is_taken)
}

/// Sends `signal` to `pid`, and waits at most `timeout` until `is_done`, or
/// until the process ended. The process is held by a pidfd, so the signal
/// reaches it and not one that took its ID; on a kernel without pidfds,
/// before Linux 5.3, it is sent by its ID.
fn send(
    pid: u32,
    signal: Signal,
    timeout: Duration,
    is_done: &mut dyn FnMut() -> bool,
) -> io::Result<bool> {
    const ESRCH: i32 = 3;
    const ENOSYS: i32 = 38;
    if pid == 0 || i32::try_from(pid).is_err() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no such process ID",
        ));
    }
    let process = match Process::open(pid) {
        Ok(process) => Some(process),
        Err(error) if error.raw_os_error() == Some(ESRCH) => return Ok(true),
        Err(error) if error.raw_os_error() == Some(ENOSYS) => None,
        Err(error) => return Err(error),
    };
    let sent = match &process {
        Some(process) => process.signal(signal),
        None => zbm_sys::send_signal(pid, signal),
    };
    match sent {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == Some(ESRCH) => return Ok(true),
        Err(error) => return Err(error),
    }
    let start = Instant::now();
    loop {
        if is_done() {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        match &process {
            // Back at once when it ended: a console that is gone has
            // nothing to hand back.
            Some(process) => {
                if process.wait_exit(Some(POLL))? {
                    return Ok(true);
                }
            }
            None => std::thread::sleep(POLL),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command};

    use super::*;

    /// A process that a request's signal ends at once, as it would end a
    /// console that does not wait for it.
    fn child(script: &str) -> Child {
        Command::new("sh").args(["-c", script]).spawn().unwrap()
    }

    #[test]
    fn a_request_waits_until_the_console_is_done() {
        let mut console = child("trap '' USR1 USR2; sleep 30");
        // Let the shell set its traps.
        std::thread::sleep(Duration::from_millis(200));
        let mut asked = 0;
        let late = request(console.id(), Duration::from_millis(50), &mut || {
            asked += 1;
            false
        });
        assert!(!late.unwrap());
        assert!(asked >= 2);
        let done = request_layout(console.id(), Duration::from_secs(5), &mut || true);
        assert!(done.unwrap());
        console.kill().unwrap();
        console.wait().unwrap();
    }

    #[test]
    fn a_console_that_ends_has_nothing_to_hand_back() {
        let mut console = child("sleep 30");
        let start = Instant::now();
        // SIGUSR1 ends the shell, and the request sees it end.
        let done = request(console.id(), Duration::from_secs(20), &mut || false);
        assert!(done.unwrap());
        assert!(start.elapsed() < Duration::from_secs(10));
        console.wait().unwrap();
    }

    #[test]
    fn a_console_that_is_gone_has_nothing_to_hand_back() {
        // Process IDs end below 2^22 on Linux.
        let done = request(4_194_000, Duration::ZERO, &mut || false);
        assert!(done.unwrap());
        assert!(request(0, Duration::ZERO, &mut || false).is_err());
        assert!(request(u32::MAX, Duration::ZERO, &mut || false).is_err());
    }

    #[test]
    fn requests_set_by_hand_are_taken_without_signals() {
        let requests = Requests::default();
        requests.take();
        assert!(!requests.was_release_requested());
        requests.release.store(true, Ordering::SeqCst);
        assert!(requests.was_release_requested());
        assert!(!requests.release.load(Ordering::SeqCst));
    }
}
