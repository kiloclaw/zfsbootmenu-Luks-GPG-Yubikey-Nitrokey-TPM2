//! [`MemoryBudget`]: how much memory parallel work may hold at once.

use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::cancel::{CancelToken, Cancelled};

/// How often a wait for memory looks at its cancel token.
const CANCEL_CHECK: Duration = Duration::from_millis(50);

/// The memory that parallel work may hold at once, in bytes, handed out as
/// [`Permit`]s sized by the work: an Argon2 derivation takes a permit of its
/// memory cost, about 1 GiB, so two run together only where two fit.
#[derive(Debug)]
pub struct MemoryBudget {
    total: u64,
    used: Mutex<u64>,
    freed: Condvar,
}

/// Memory taken from a [`MemoryBudget`], given back when dropped.
#[must_use = "dropping the permit gives the memory back at once"]
#[derive(Debug)]
pub struct Permit<'a> {
    budget: &'a MemoryBudget,
    bytes: u64,
}

impl MemoryBudget {
    /// A budget of `total` bytes.
    pub fn new(total: u64) -> MemoryBudget {
        MemoryBudget {
            total,
            used: Mutex::new(0),
            freed: Condvar::new(),
        }
    }

    /// The bytes not taken.
    pub fn available(&self) -> u64 {
        self.total.saturating_sub(*self.used())
    }

    fn used(&self) -> MutexGuard<'_, u64> {
        self.used.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes `bytes`, waiting until they are free. A request larger than the
    /// whole budget takes all of it, so it runs alone rather than never.
    ///
    /// # Errors
    ///
    /// [`Cancelled`] when `cancel` is cancelled while it waits.
    pub fn acquire(
        &self,
        bytes: u64,
        cancel: Option<&CancelToken>,
    ) -> Result<Permit<'_>, Cancelled> {
        let bytes = bytes.min(self.total);
        let mut used = self.used();
        while self.total - *used < bytes {
            if let Some(token) = cancel {
                token.check()?;
            }
            used = self
                .freed
                .wait_timeout(used, CANCEL_CHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *used += bytes;
        Ok(Permit {
            budget: self,
            bytes,
        })
    }

    /// Takes `bytes` if they are free now.
    pub fn try_acquire(&self, bytes: u64) -> Option<Permit<'_>> {
        let bytes = bytes.min(self.total);
        let mut used = self.used();
        (self.total - *used >= bytes).then(|| {
            *used += bytes;
            Permit {
                budget: self,
                bytes,
            }
        })
    }
}

impl Permit<'_> {
    /// The bytes it holds.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut used = self.budget.used();
        *used = used.saturating_sub(self.bytes);
        drop(used);
        self.budget.freed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::event::WorkPool;

    #[test]
    fn permits_take_and_give_back_memory() {
        let budget = MemoryBudget::new(100);
        let first = budget.try_acquire(60).unwrap();
        assert_eq!((first.bytes(), budget.available()), (60, 40));
        assert!(budget.try_acquire(50).is_none());
        let huge = budget.try_acquire(500);
        assert!(huge.is_none());
        drop(first);
        let all = budget.acquire(500, None).unwrap();
        assert_eq!((all.bytes(), budget.available()), (100, 0));
    }

    #[test]
    fn parallel_work_holds_no_more_than_the_budget() {
        let budget = MemoryBudget::new(3);
        let held = AtomicU64::new(0);
        let most = AtomicU64::new(0);
        let pool = WorkPool::new(NonZeroUsize::new(4).unwrap());
        pool.map((0..12).collect(), |_: u8| {
            let permit = budget.acquire(2, None).unwrap();
            let now = held.fetch_add(permit.bytes(), Ordering::SeqCst) + permit.bytes();
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::yield_now();
            held.fetch_sub(permit.bytes(), Ordering::SeqCst);
        });
        assert!(most.load(Ordering::SeqCst) <= 3);
        assert_eq!(budget.available(), 3);
    }

    #[test]
    fn a_wait_for_memory_ends_when_cancelled() {
        let budget = MemoryBudget::new(1);
        let _held = budget.acquire(1, None).unwrap();
        let token = CancelToken::new().unwrap();
        token.cancel();
        assert_eq!(budget.acquire(1, Some(&token)).unwrap_err(), Cancelled);
    }
}
