//! Handing the display back before `kexec`: `zbm console release` sends
//! the console `SIGUSR1` and waits until it has given the display, the
//! keyboards and the virtual terminal back to the kernel.

#![allow(
    clippy::disallowed_methods,
    reason = "waiting on another process, the console, takes real time"
)]

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process};

/// How often the request looks whether the console is done.
const POLL: Duration = Duration::from_millis(20);

/// Sets the flag it returns when this process is asked to hand the display
/// back.
///
/// # Errors
///
/// When the signal's handler cannot be installed.
pub fn on_request() -> io::Result<Arc<AtomicBool>> {
    let flag = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGUSR1, Arc::clone(&flag))?;
    Ok(flag)
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
    let pid = i32::try_from(pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no such process ID"))?;
    match kill_process(pid, Signal::USR1) {
        Ok(()) => {}
        Err(rustix::io::Errno::SRCH) => return Ok(true),
        Err(error) => return Err(error.into()),
    }
    let start = Instant::now();
    loop {
        if is_released() {
            return Ok(true);
        }
        if start.elapsed() >= timeout {
            return Ok(false);
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn a_request_reaches_the_console_and_waits_for_it() {
        let flag = on_request().unwrap();
        let me = std::process::id();

        let done = request(me, Duration::from_secs(5), &mut || {
            flag.load(Ordering::SeqCst)
        });
        assert!(done.unwrap());

        let mut asked = 0;
        let late = request(me, Duration::from_millis(50), &mut || {
            asked += 1;
            false
        });
        assert!(!late.unwrap());
        assert!(asked >= 2);
    }

    #[test]
    fn a_console_that_is_gone_has_nothing_to_hand_back() {
        // Process IDs end below 2^22 on Linux.
        let done = request(4_194_000, Duration::ZERO, &mut || false);
        assert!(done.unwrap());
        assert!(request(0, Duration::ZERO, &mut || false).is_err());
        assert!(request(u32::MAX, Duration::ZERO, &mut || false).is_err());
    }
}
