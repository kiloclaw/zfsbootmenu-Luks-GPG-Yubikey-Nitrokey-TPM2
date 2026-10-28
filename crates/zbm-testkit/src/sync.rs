//! Locking the state the fakes share between threads.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// The value behind `mutex`, also after a test panicked while holding it.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A value a test shares with the responders of its fakes, which may run on
/// other threads: a thread-safe `Rc<RefCell<T>>`, with the methods of `Cell`
/// and `RefCell`. Clones share the value.
#[derive(Debug, Default)]
pub struct Shared<T>(std::sync::Arc<Mutex<T>>);

impl<T> Clone for Shared<T> {
    fn clone(&self) -> Shared<T> {
        Shared(std::sync::Arc::clone(&self.0))
    }
}

impl<T> Shared<T> {
    /// A shared `value`.
    pub fn new(value: T) -> Shared<T> {
        Shared(std::sync::Arc::new(Mutex::new(value)))
    }

    /// Replaces the value.
    pub fn set(&self, value: T) {
        *lock(&self.0) = value;
    }

    /// The value, to read.
    pub fn borrow(&self) -> MutexGuard<'_, T> {
        lock(&self.0)
    }

    /// The value, to change.
    pub fn borrow_mut(&self) -> MutexGuard<'_, T> {
        lock(&self.0)
    }
}

impl<T: Clone> Shared<T> {
    /// A copy of the value.
    pub fn get(&self) -> T {
        lock(&self.0).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_share_the_value() {
        let shared = Shared::new(1);
        let other = shared.clone();
        other.set(2);
        assert_eq!(shared.get(), 2);
        *shared.borrow_mut() += 1;
        assert_eq!(*other.borrow(), 3);
        assert_eq!(Shared::<u8>::default().get(), 0);
    }
}
