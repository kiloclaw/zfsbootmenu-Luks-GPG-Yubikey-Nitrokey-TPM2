//! Waiting for child processes, as process 1 does: any child, so that
//! orphans are reaped too, or one child without blocking; and taking the
//! terminal back from a child that left it to its own process group.

use std::ffi::c_int;
use std::io;

use super::{ECHILD, SIG_ERR, SIG_IGN, signal, waitpid};

unsafe extern "C" {
    fn getpgrp() -> c_int;
    fn tcgetpgrp(fd: c_int) -> c_int;
    fn tcsetpgrp(fd: c_int, pgrp: c_int) -> c_int;
}

/// `SIGTTOU`, which the kernel sends a process of a background group that
/// sets the terminal's foreground group (the same on x86 and Arm).
const SIGTTOU: c_int = 22;

/// `WNOHANG`: `waitpid` returns at once when the child still runs.
const WNOHANG: c_int = 1;

/// How a child process ended, as `waitpid` reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Ended {
    /// The process ID.
    pub pid: u32,
    /// The raw wait status.
    pub status: i32,
}

/// Waits for any child of this process to end, as process 1 must, so that
/// orphans are reaped too. Returns `None` when the process has no children.
///
/// # Errors
///
/// When `waitpid` fails other than by a signal or for lack of children.
pub fn wait_any() -> io::Result<Option<Ended>> {
    match wait(-1, 0) {
        Ok(ended) => Ok(ended),
        Err(error) if error.raw_os_error() == Some(ECHILD) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The raw wait status of the child `pid` if it ended, which collects it;
/// `None` while it runs. Does not block.
///
/// # Errors
///
/// When `pid` is not a child of this process (`ECHILD`), or was collected
/// already.
pub fn try_wait(pid: u32) -> io::Result<Option<i32>> {
    // 0 and negative values would wait for process groups.
    let pid = c_int::try_from(pid)
        .ok()
        .filter(|&pid| pid > 0)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    wait(pid, WNOHANG).map(|ended| ended.map(|ended| ended.status))
}

/// Makes this process's group the foreground group of the terminal open
/// as `fd` again, as a shell does when a job ends. A shell that leads a
/// process group of its own, such as brush, and ends without giving the
/// terminal back leaves its dead group in the foreground: this process
/// then cannot read the terminal, nor set its modes. Nothing changes when
/// the group is in the foreground already.
///
/// # Errors
///
/// When `fd` is not this process's controlling terminal.
pub fn take_terminal(fd: i32) -> io::Result<()> {
    // SAFETY: `getpgrp`, `tcgetpgrp` and `tcsetpgrp` take and return plain
    // integers. `signal` gets `SIG_IGN`, then the disposition it returned,
    // which was installed before and is therefore valid.
    unsafe {
        let group = getpgrp();
        let foreground = tcgetpgrp(fd);
        if foreground < 0 {
            return Err(io::Error::last_os_error());
        }
        if foreground == group {
            return Ok(());
        }
        // From a background group, setting the foreground group raises
        // SIGTTOU, which would stop this process.
        let previous = signal(SIGTTOU, SIG_IGN);
        let result = tcsetpgrp(fd, group);
        let error = io::Error::last_os_error();
        if previous != SIG_ERR {
            signal(SIGTTOU, previous);
        }
        if result < 0 { Err(error) } else { Ok(()) }
    }
}

/// `waitpid(pid, options)`, again when a signal interrupts it: the child
/// that ended, `None` when `WNOHANG` found none.
fn wait(pid: c_int, options: c_int) -> io::Result<Option<Ended>> {
    let mut status: c_int = 0;
    loop {
        // SAFETY: `status` is a valid, writable `int` for the duration of the call.
        let ended = unsafe { waitpid(pid, &mut status, options) };
        if let Ok(pid) = u32::try_from(ended) {
            return Ok((pid > 0).then_some(Ended { pid, status }));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests may panic")]
mod tests {
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::tests::PROCESSES;

    #[test]
    #[allow(
        clippy::zombie_processes,
        reason = "wait_any collects the child, which is what the test checks"
    )]
    fn waiting_for_any_child() {
        let _guard = PROCESSES.lock().unwrap();
        let child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        let ended = wait_any().unwrap().unwrap();
        assert_eq!(ended.pid, child.id());
        assert_eq!((ended.status >> 8) & 0xff, 3);
        assert_eq!(wait_any().unwrap(), None);
    }

    #[test]
    #[allow(
        clippy::zombie_processes,
        reason = "try_wait collects the child, which is what the test checks"
    )]
    fn waiting_for_one_child_without_blocking() {
        let _guard = PROCESSES.lock().unwrap();
        let child = Command::new("sh").args(["-c", "exit 4"]).spawn().unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = try_wait(child.id()).unwrap() {
                break status;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!((status >> 8) & 0xff, 4);
        // Collected: no longer a child.
        assert!(try_wait(child.id()).is_err());
        assert!(try_wait(0).is_err());

        let mut running = Command::new("sleep").arg("5").spawn().unwrap();
        assert_eq!(try_wait(running.id()).unwrap(), None);
        running.kill().unwrap();
        running.wait().unwrap();
    }
    #[test]
    fn only_a_terminal_can_be_taken_back() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let error = take_terminal(std::os::fd::AsRawFd::as_raw_fd(&file)).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(25), "{error}"); // ENOTTY
    }
}
