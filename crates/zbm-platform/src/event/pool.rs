//! [`WorkPool`]: parallel work on a fixed number of threads.

use std::collections::VecDeque;
use std::num::NonZeroUsize;
use std::sync::{Mutex, PoisonError};

use super::spawn::spawn_scoped_traced;

/// Runs work in parallel on at most a fixed number of threads (ADR 0001),
/// each started with [`spawn_scoped_traced`], so it logs where the caller
/// does. Memory is bounded separately, by a
/// [`MemoryBudget`](super::MemoryBudget) the work takes permits from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkPool {
    threads: NonZeroUsize,
}

impl WorkPool {
    /// A pool of `threads` threads.
    pub fn new(threads: NonZeroUsize) -> WorkPool {
        WorkPool { threads }
    }

    /// A pool of as many threads as the machine runs at once, at most
    /// `limit`.
    pub fn available(limit: NonZeroUsize) -> WorkPool {
        let threads =
            std::thread::available_parallelism().map_or(NonZeroUsize::MIN, |n| n.min(limit));
        WorkPool { threads }
    }

    /// The number of threads.
    pub fn threads(self) -> NonZeroUsize {
        self.threads
    }

    /// `work` applied to each of `items`, at most [`WorkPool::threads`] at a
    /// time; the results in the order of the items. It returns when all are
    /// done; a panic in `work` comes back here.
    pub fn map<I: Send, R: Send>(self, items: Vec<I>, work: impl Fn(I) -> R + Sync) -> Vec<R> {
        let count = items.len();
        if count <= 1 || self.threads.get() == 1 {
            return items.into_iter().map(work).collect();
        }
        let queue: Mutex<VecDeque<(usize, I)>> =
            Mutex::new(items.into_iter().enumerate().collect());
        let results: Mutex<Vec<Option<R>>> = Mutex::new((0..count).map(|_| None).collect());
        let next = || {
            queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front()
        };
        std::thread::scope(|scope| {
            for _ in 0..self.threads.get().min(count) {
                spawn_scoped_traced(scope, || {
                    while let Some((at, item)) = next() {
                        let result = work(item);
                        if let Some(slot) = results
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .get_mut(at)
                        {
                            *slot = Some(result);
                        }
                    }
                });
            }
        });
        results
            .into_inner()
            .unwrap_or_else(PoisonError::into_inner)
            .into_iter()
            .flatten()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn pool(threads: usize) -> WorkPool {
        WorkPool::new(NonZeroUsize::new(threads).unwrap())
    }

    #[test]
    fn results_keep_the_order_of_the_items() {
        let squares = pool(3).map((1..=20).collect(), |n: u64| n * n);
        assert_eq!(squares, (1..=20).map(|n| n * n).collect::<Vec<_>>());
        assert_eq!(pool(4).map(vec![7], |n: u8| n), [7]);
        assert!(pool(4).map(Vec::<u8>::new(), |n| n).is_empty());
        assert_eq!(pool(1).map(vec![1, 2], |n: u8| n + 1), [2, 3]);
    }

    #[test]
    fn no_more_than_its_threads_run_at_once() {
        let running = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        pool(2).map((0..8).collect(), |_: u8| {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            std::thread::yield_now();
            running.fetch_sub(1, Ordering::SeqCst);
        });
        assert!(most.load(Ordering::SeqCst) <= 2);
        let limited = WorkPool::available(NonZeroUsize::MIN);
        assert_eq!(limited.threads(), NonZeroUsize::MIN);
    }
}
