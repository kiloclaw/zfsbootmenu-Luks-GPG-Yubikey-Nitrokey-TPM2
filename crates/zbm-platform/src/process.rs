//! Processes: the runtime's own process ID, and signals to other processes.
//!
//! A second menu takes over from the first by signalling it and ending its
//! deepest child (see "Concurrent instances and takeover" in
//! `crates/DESIGN.md`); [`Processes`] sends the signals, and [`first_child`]
//! finds the children through `/proc`.

use std::io;
use std::path::PathBuf;

pub use zbm_sys::Signal;

use crate::files::Files;

/// The process and signals.
pub trait Processes {
    /// The ID of this process (`$$`).
    fn id(&self) -> u32;

    /// Sends `signal` to the process `pid` (`kill -SIG pid`).
    ///
    /// # Errors
    ///
    /// When the signal cannot be sent, as `kill` fails.
    fn signal(&self, pid: u32, signal: Signal) -> io::Result<()>;
}

/// The real processes.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcesses;

impl Processes for SystemProcesses {
    fn id(&self) -> u32 {
        std::process::id()
    }

    fn signal(&self, pid: u32, signal: Signal) -> io::Result<()> {
        zbm_sys::send_signal(pid, signal)
    }
}

/// The first child of process `pid`, as the legacy code reads it from
/// `/proc/<pid>/task/<pid>/children`.
pub fn first_child(files: &dyn Files, pid: u32) -> Option<u32> {
    let path = PathBuf::from(format!("/proc/{pid}/task/{pid}/children"));
    let children = files.read_to_string(&path).ok()?;
    children.split_whitespace().next()?.parse().ok()
}

/// The last process in the chain of first children below `pid`, or `pid`
/// itself if it has none: the process that blocks a running menu, which a
/// takeover ends.
pub fn deepest_child(files: &dyn Files, pid: u32) -> u32 {
    let mut current = pid;
    // A chain cannot be longer than the number of processes; the limit only
    // guards against a cycle in a broken /proc.
    for _ in 0..4096 {
        match first_child(files, current) {
            Some(child) => current = child,
            None => break,
        }
    }
    current
}

#[cfg(test)]
#[allow(
    clippy::disallowed_methods,
    reason = "the tests check the real files and processes"
)]
mod tests {
    use super::*;
    use crate::files::RealFiles;

    #[test]
    fn finds_children_of_real_processes() {
        let processes = SystemProcesses;
        assert_eq!(processes.id(), std::process::id());

        let mut parent = std::process::Command::new("sh")
            .args(["-c", "sleep 30 & wait"])
            .spawn()
            .unwrap();
        let pid = parent.id();
        // The shell starts `sleep` in the background right away; wait for it.
        let mut child = None;
        for _ in 0..500 {
            child = first_child(&RealFiles, pid);
            if child.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let child = child.expect("the shell started sleep");
        assert_ne!(child, pid);
        assert_eq!(deepest_child(&RealFiles, pid), child);
        assert_eq!(first_child(&RealFiles, child), None);
        assert_eq!(deepest_child(&RealFiles, child), child);

        processes.signal(child, Signal::Terminate).unwrap();
        parent.wait().unwrap();
        assert!(processes.signal(pid, Signal::Terminate).is_err());
    }
}
