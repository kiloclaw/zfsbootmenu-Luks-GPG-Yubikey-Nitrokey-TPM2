//! Ending a child that runs past its time.

use std::io;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::Signal;

/// Kills a child that runs past its time. The kill and the collection of
/// the child happen under one lock, so that the signal never reaches
/// another process that got the child's ID once it was collected.
pub(super) struct Watchdog {
    /// Whether the child was collected.
    collected: Arc<Mutex<bool>>,
    /// Ends the watchdog early, when the child ended in time.
    done: mpsc::Sender<()>,
}

impl Watchdog {
    /// How often a child that closed its output is looked at until it ends.
    const POLL: Duration = Duration::from_millis(5);

    /// Watches the child `pid`, which may run for `timeout`.
    pub(super) fn start(pid: u32, timeout: Duration) -> Watchdog {
        let collected = Arc::new(Mutex::new(false));
        let (done, ended) = mpsc::channel::<()>();
        let watched = Arc::clone(&collected);
        let watch = move || {
            if ended.recv_timeout(timeout) == Err(mpsc::RecvTimeoutError::Timeout) {
                let collected = watched
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if !*collected {
                    let _ = zbm_sys::send_signal(pid, Signal::Kill);
                }
            }
        };
        // Without a thread, the time-out is not watched; the command runs
        // as long as it runs.
        let _ = crate::event::spawn_traced("watchdog", watch);
        Watchdog { collected, done }
    }

    /// Waits for `child` to end, collecting it under the lock.
    pub(super) fn wait(
        &self,
        child: &mut std::process::Child,
    ) -> io::Result<std::process::ExitStatus> {
        loop {
            {
                let mut collected = self
                    .collected
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(status) = child.try_wait()? {
                    *collected = true;
                    let _ = self.done.send(());
                    return Ok(status);
                }
            }
            std::thread::sleep(Self::POLL);
        }
    }
}
