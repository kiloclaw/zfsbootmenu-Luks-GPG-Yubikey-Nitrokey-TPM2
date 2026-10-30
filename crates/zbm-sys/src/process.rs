//! Starting child processes in a session of their own, with the default
//! action of some signals; waiting for them, as process 1 does: any child,
//! so that orphans are reaped too, or one child without blocking; and
//! taking the terminal back from a child that left it to its own process
//! group.

use std::ffi::c_int;
use std::io;
use std::os::fd::{AsFd, AsRawFd as _, OwnedFd};

use rustix::io::Errno;
use rustix::ioctl::{IntegerSetter, Opcode, ioctl};
use rustix::process::{Pid, WaitOptions, getpgrp, setsid};
use rustix::termios::{tcgetpgrp, tcsetpgrp};

use super::{SIG_DFL, SIG_ERR, SIG_IGN, Signal, signal};

/// `TIOCSCTTY`: makes a terminal the controlling terminal of the session,
/// as `asm-generic/ioctls.h` numbers it for x86, Arm and RISC-V.
/// `rustix`'s `ioctl_tiocsctty` passes 0, which does not take the terminal
/// from another session; `setsid -c` passes 1.
const TIOCSCTTY: Opcode = 0x540E;

/// The session a child process runs in.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Session {
    /// This process's.
    #[default]
    Inherit,
    /// One of its own, without a controlling terminal (`setsid`).
    New,
    /// One of its own, whose controlling terminal is the child's standard
    /// input (`setsid -c`), taken from another session that has it.
    NewOnTerminal,
}

/// What a child process does between `fork` and `exec`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChildSetup {
    /// The signals the program starts with the default action of, as
    /// `trap - SIG` before `exec` does, whatever this process does with
    /// them.
    pub default_signals: Vec<Signal>,
    /// The session it runs in.
    pub session: Session,
}

/// Has the program that `process` starts set up as `setup` says, with no
/// signal blocked: the signals this process reads from a
/// [`Signals`](crate::Signals) reach it as usual. Starting a session fails
/// when the child leads a process group, which a child that `spawn` forks
/// never does; `exec` without a fork needs a process that leads none.
pub fn prepare_child(process: &mut std::process::Command, setup: &ChildSetup) {
    use std::os::unix::process::CommandExt;
    let unblock = super::signals::is_blocking_any();
    if *setup == ChildSetup::default() && !unblock {
        return;
    }
    let numbers: Vec<c_int> = setup
        .default_signals
        .iter()
        .map(|signal| signal.number())
        .collect();
    let session = setup.session;
    let prepare = move || {
        if unblock && super::signals::unblock_all() != 0 {
            return Err(io::Error::last_os_error());
        }
        for &number in &numbers {
            // SAFETY: `SIG_DFL` is a valid disposition, and `signal` is
            // async-signal-safe.
            if unsafe { signal(number, SIG_DFL) } == SIG_ERR {
                return Err(io::Error::last_os_error());
            }
        }
        if session != Session::Inherit {
            setsid()?;
        }
        if session == Session::NewOnTerminal {
            // 1: taken from another session that has it, as `setsid -c`
            // does.
            // SAFETY: `TIOCSCTTY` takes its argument by value; the kernel
            // reads nothing from this process's memory.
            unsafe {
                ioctl(
                    rustix::stdio::stdin(),
                    IntegerSetter::<TIOCSCTTY>::new_usize(1),
                )
            }?;
        }
        Ok(())
    };
    // SAFETY: the closure runs between `fork` and `exec`, where only
    // async-signal-safe functions may be called: it calls
    // `pthread_sigmask`, `signal`, and the `setsid` and `ioctl` system
    // calls, reads `errno`, and allocates nothing.
    unsafe {
        process.pre_exec(prepare);
    }
}

/// Has the program that `process` starts keep `fd`, a copy of a
/// descriptor this process holds, at the number it has here, which this
/// returns for its arguments or environment
/// ([`Connection::inherited`](crate::socket::Connection::inherited)).
/// Only that program gets it: `fd` stays closed on `exec` here, and the
/// child clears that for itself between `fork` and `exec`.
pub fn pass_fd(process: &mut std::process::Command, fd: OwnedFd) -> i32 {
    use std::os::unix::process::CommandExt;
    let number = fd.as_raw_fd();
    let keep = move || {
        rustix::io::fcntl_setfd(&fd, rustix::io::FdFlags::empty())?;
        Ok(())
    };
    // SAFETY: the closure runs between `fork` and `exec`, where only
    // async-signal-safe functions may be called: it makes one `fcntl`
    // system call, and allocates nothing.
    unsafe {
        process.pre_exec(keep);
    }
    number
}

/// `SIGTTOU`, which the kernel sends a process of a background group that
/// sets the terminal's foreground group (the same on x86 and Arm).
const SIGTTOU: c_int = 22;

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
    loop {
        match rustix::process::wait(WaitOptions::empty()) {
            Ok(ended) => return Ok(ended.map(ended_of)),
            Err(Errno::CHILD) => return Ok(None),
            Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// A child of this process that ended, which this collects, without
/// waiting: `None` when none ended, or there are no children.
///
/// # Errors
///
/// When `waitpid` fails other than by a signal or for lack of children.
pub fn reap() -> io::Result<Option<Ended>> {
    loop {
        match rustix::process::wait(WaitOptions::NOHANG) {
            Ok(ended) => return Ok(ended.map(ended_of)),
            Err(Errno::CHILD) => return Ok(None),
            Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
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
    let pid = i32::try_from(pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    loop {
        match rustix::process::waitpid(Some(pid), WaitOptions::NOHANG) {
            Ok(ended) => return Ok(ended.map(|ended| ended_of(ended).status)),
            Err(Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// What `waitpid` returned, as an [`Ended`].
fn ended_of((pid, status): (Pid, rustix::process::WaitStatus)) -> Ended {
    Ended {
        pid: pid.as_raw_nonzero().get().unsigned_abs(),
        status: status.as_raw(),
    }
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
pub fn take_terminal(fd: impl AsFd) -> io::Result<()> {
    let group = getpgrp();
    if tcgetpgrp(&fd)? == group {
        return Ok(());
    }
    // From a background group, setting the foreground group raises
    // SIGTTOU, which would stop this process.
    // SAFETY: `signal` gets `SIG_IGN`, then the disposition it returned,
    // which was installed before and is therefore valid.
    let previous = unsafe { signal(SIGTTOU, SIG_IGN) };
    let result = tcsetpgrp(&fd, group);
    if previous != SIG_ERR {
        // SAFETY: as above.
        unsafe { signal(SIGTTOU, previous) };
    }
    Ok(result?)
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "zbm-sys is tested against the kernel itself: real programs, files and time"
)]
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
    #[allow(
        clippy::zombie_processes,
        reason = "reap collects the child, which is what the test checks"
    )]
    fn any_child_that_ended_is_reaped_without_waiting() {
        let _guard = PROCESSES.lock().unwrap();
        assert_eq!(reap().unwrap(), None);
        let child = Command::new("sh").args(["-c", "exit 5"]).spawn().unwrap();
        let start = Instant::now();
        let ended = loop {
            if let Some(ended) = reap().unwrap() {
                break ended;
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(ended.pid, child.id());
        assert_eq!((ended.status >> 8) & 0xff, 5);
        assert_eq!(reap().unwrap(), None);
    }
    /// The process and session IDs in `/proc/self/stat` of the program that
    /// `setup` prepares: `cat`, which prints its own.
    fn ids(setup: &ChildSetup) -> std::io::Result<(u32, u32)> {
        let mut command = Command::new("cat");
        command.arg("/proc/self/stat");
        prepare_child(&mut command, setup);
        let output = command.output()?;
        let stat = String::from_utf8(output.stdout).unwrap();
        // The name in parentheses may hold spaces; the fields after it are
        // the state, the parent, the group and the session.
        let (pid, rest) = stat.split_once(" (").unwrap();
        let fields: Vec<&str> = rest.rsplit_once(") ").unwrap().1.split(' ').collect();
        Ok((pid.parse().unwrap(), fields[3].parse().unwrap()))
    }

    #[test]
    fn a_child_starts_a_session_of_its_own_when_asked() {
        let _guard = PROCESSES.lock().unwrap();
        let (pid, session) = ids(&ChildSetup::default()).unwrap();
        assert_ne!(pid, session);
        let new = ChildSetup {
            session: Session::New,
            ..ChildSetup::default()
        };
        let (pid, session) = ids(&new).unwrap();
        assert_eq!(pid, session);
        // Without a terminal as standard input, there is none to control.
        let on_terminal = ChildSetup {
            session: Session::NewOnTerminal,
            ..ChildSetup::default()
        };
        let mut command = Command::new("true");
        command.stdin(std::process::Stdio::null());
        prepare_child(&mut command, &on_terminal);
        let error = command.status().unwrap_err();
        assert_eq!(error.raw_os_error(), Some(25), "{error}"); // ENOTTY
    }

    #[test]
    fn a_descriptor_passed_reaches_that_child_alone() {
        // A test that reaps any child must not collect this one.
        let _guard = PROCESSES.lock().unwrap();
        let file = std::fs::File::open("/dev/null").unwrap();
        let is_open_in_child = |pass: bool| {
            let mut command = std::process::Command::new("/bin/sh");
            let number = if pass {
                pass_fd(&mut command, file.as_fd().try_clone_to_owned().unwrap())
            } else {
                file.as_raw_fd()
            };
            let test = format!("[ -e /proc/self/fd/{number} ]");
            command.args(["-c", &test]).status().unwrap().success()
        };
        assert!(is_open_in_child(true));
        assert!(!is_open_in_child(false));
    }

    #[test]
    fn only_a_terminal_can_be_taken_back() {
        let file = std::fs::File::open("/dev/null").unwrap();
        let error = take_terminal(&file).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(25), "{error}"); // ENOTTY
    }
}
