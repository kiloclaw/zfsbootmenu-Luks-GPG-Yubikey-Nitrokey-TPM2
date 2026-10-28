//! [`CancelToken`]: asking long work to stop.

use std::fmt;
use std::io;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rustix::event::{EventfdFlags, eventfd};

/// Asks work to stop: a command, which the [`Runner`](crate::Runner) kills
/// when its token is cancelled, or work in the process, such as a key
/// derivation, which checks it between steps. Clones share one token; it
/// stays cancelled once it is.
///
/// Its file descriptor ([`AsFd`]) becomes readable when it is cancelled, so
/// a `poll` notices it among the reactor's other sources.
#[derive(Clone)]
pub struct CancelToken(Arc<Inner>);

/// What the clones of a token share.
struct Inner {
    /// Whether it was cancelled.
    cancelled: AtomicBool,
    /// An `eventfd`, written once when it is cancelled.
    wake: OwnedFd,
}

/// Work that stopped because its token was cancelled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("cancelled")
    }
}

impl std::error::Error for Cancelled {}

impl CancelToken {
    /// A token that is not cancelled.
    ///
    /// # Errors
    ///
    /// When the `eventfd` cannot be created.
    pub fn new() -> io::Result<CancelToken> {
        let wake = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?;
        Ok(CancelToken(Arc::new(Inner {
            cancelled: AtomicBool::new(false),
            wake,
        })))
    }

    /// Cancels it, from any thread.
    pub fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::SeqCst) {
            // The counter only rises: a failed write leaves the flag set,
            // which every check sees.
            let _ = rustix::io::write(&self.0.wake, &1_u64.to_ne_bytes());
        }
    }

    /// Whether it was cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// For work in steps: `Err(Cancelled)` once it was cancelled.
    ///
    /// # Errors
    ///
    /// [`Cancelled`] when the token was cancelled.
    pub fn check(&self) -> Result<(), Cancelled> {
        if self.is_cancelled() {
            Err(Cancelled)
        } else {
            Ok(())
        }
    }
}

impl AsFd for CancelToken {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.wake.as_fd()
    }
}

impl fmt::Debug for CancelToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Tokens are equal when they are clones of one.
impl PartialEq for CancelToken {
    fn eq(&self, other: &CancelToken) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for CancelToken {}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_cancelled_token_stays_cancelled_and_wakes_a_poll() {
        let token = CancelToken::new().unwrap();
        let clone = token.clone();
        assert!(!token.is_cancelled() && token.check().is_ok());
        assert!(!crate::event::reactor::readable(&token, Some(Duration::ZERO)).unwrap());
        clone.cancel();
        clone.cancel();
        assert_eq!(token.check(), Err(Cancelled));
        assert!(crate::event::reactor::readable(&token, Some(Duration::ZERO)).unwrap());
        assert_eq!(token, clone);
        assert_ne!(token, CancelToken::new().unwrap());
        assert_eq!(format!("{token:?}"), "CancelToken { cancelled: true }");
        assert_eq!(Cancelled.to_string(), "cancelled");
    }
}
