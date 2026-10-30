//! A dispatcher that lives as long as the process, so that no event's
//! interest is ever cached as "never".
//!
//! `tracing` asks each call site once whether any dispatcher wants it,
//! of the dispatchers alive at that moment, and caches the answer; a new
//! dispatcher asks every call site again. A call site that registers on a
//! thread while another thread creates the first dispatcher can miss both
//! questions, and keep "never" for good: its events then reach no log,
//! as a test that checked a warning found one run in a hundred. With
//! [`keep_interest`] there is always a dispatcher alive that answers
//! "sometimes", so every event asks the thread's own log whether it
//! wants it.

use std::sync::OnceLock;

use tracing::subscriber::{Interest, Subscriber};
use tracing::{Dispatch, Event, Metadata, span};

/// The dispatcher that keeps interest.
static KEPT: OnceLock<Dispatch> = OnceLock::new();

/// Keeps, from now on, one dispatcher alive that leaves every event to the
/// thread's own log. Logs call it as they are installed.
pub fn keep_interest() {
    KEPT.get_or_init(|| Dispatch::new(Sometimes));
}

/// A subscriber that wants nothing itself, and asks about every event.
struct Sometimes;

impl Subscriber for Sometimes {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, _: &Metadata<'_>) -> bool {
        false
    }

    fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
        span::Id::from_u64(1)
    }

    fn record(&self, _: &span::Id, _: &span::Record<'_>) {}

    fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}

    fn event(&self, _: &Event<'_>) {}

    fn enter(&self, _: &span::Id) {}

    fn exit(&self, _: &span::Id) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A subscriber that counts the events it gets.
    struct Counting(Arc<AtomicUsize>);

    impl Subscriber for Counting {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &span::Attributes<'_>) -> span::Id {
            span::Id::from_u64(1)
        }
        fn record(&self, _: &span::Id, _: &span::Record<'_>) {}
        fn record_follows_from(&self, _: &span::Id, _: &span::Id) {}
        fn event(&self, _: &Event<'_>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn enter(&self, _: &span::Id) {}
        fn exit(&self, _: &span::Id) {}
    }

    /// One call site, fired each time.
    fn fire() {
        tracing::warn!("a warning");
    }

    #[test]
    fn an_event_first_fired_without_a_log_reaches_a_log_installed_later() {
        keep_interest();
        keep_interest();
        // No log on this thread: the call site registers, and wants
        // asking each time, not never.
        fire();
        let count = Arc::new(AtomicUsize::new(0));
        tracing::subscriber::with_default(Counting(Arc::clone(&count)), fire);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(KEPT.get().is_some());
    }
}
