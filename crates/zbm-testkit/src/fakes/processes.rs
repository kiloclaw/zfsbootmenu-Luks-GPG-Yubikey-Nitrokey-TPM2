//! Recorded signals.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io;

use zbm_platform::process::{Processes, Signal};

/// A [`Processes`] that records the signals sent instead of sending them.
pub struct FakeProcesses {
    id: u32,
    signals: RefCell<Vec<(u32, Signal)>>,
    gone: RefCell<BTreeSet<u32>>,
}

impl Default for FakeProcesses {
    fn default() -> FakeProcesses {
        FakeProcesses::new(4242)
    }
}

impl FakeProcesses {
    /// Processes in which this one has the ID `id`.
    pub fn new(id: u32) -> FakeProcesses {
        FakeProcesses {
            id,
            signals: RefCell::new(Vec::new()),
            gone: RefCell::new(BTreeSet::new()),
        }
    }

    /// Makes signals to `pid` fail as for a process that has ended.
    pub fn end(&self, pid: u32) {
        self.gone.borrow_mut().insert(pid);
    }

    /// The signals sent so far, with their targets.
    pub fn signals(&self) -> Vec<(u32, Signal)> {
        self.signals.borrow().clone()
    }
}

impl Processes for FakeProcesses {
    fn id(&self) -> u32 {
        self.id
    }

    fn signal(&self, pid: u32, signal: Signal) -> io::Result<()> {
        if self.gone.borrow().contains(&pid) {
            // ESRCH, as kill(2) reports it.
            return Err(io::Error::from_raw_os_error(3));
        }
        self.signals.borrow_mut().push((pid, signal));
        Ok(())
    }
}
