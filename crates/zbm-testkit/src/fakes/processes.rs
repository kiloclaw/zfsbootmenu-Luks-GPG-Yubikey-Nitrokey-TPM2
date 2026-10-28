//! Recorded signals.

use std::sync::Mutex;

use crate::sync::lock;
use std::collections::BTreeSet;
use std::io;

use zbm_platform::process::{Processes, Signal};

/// A [`Processes`] that records the signals sent instead of sending them.
pub struct FakeProcesses {
    id: u32,
    signals: Mutex<Vec<(u32, Signal)>>,
    gone: Mutex<BTreeSet<u32>>,
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
            signals: Mutex::new(Vec::new()),
            gone: Mutex::new(BTreeSet::new()),
        }
    }

    /// Makes signals to `pid` fail as for a process that has ended.
    pub fn end(&self, pid: u32) {
        lock(&self.gone).insert(pid);
    }

    /// The signals sent so far, with their targets.
    pub fn signals(&self) -> Vec<(u32, Signal)> {
        lock(&self.signals).clone()
    }
}

impl Processes for FakeProcesses {
    fn id(&self) -> u32 {
        self.id
    }

    fn signal(&self, pid: u32, signal: Signal) -> io::Result<()> {
        if lock(&self.gone).contains(&pid) {
            // ESRCH, as kill(2) reports it.
            return Err(io::Error::from_raw_os_error(3));
        }
        lock(&self.signals).push((pid, signal));
        Ok(())
    }
}
