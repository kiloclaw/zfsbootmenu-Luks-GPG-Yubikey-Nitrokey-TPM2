use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// A subscriber that counts the events it gets, and hands every call on
/// to [`Sometimes`], which does nothing with any.
struct Counting(Arc<AtomicUsize>);

impl Subscriber for Counting {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        let _ = Sometimes.enabled(metadata);
        true
    }
    fn new_span(&self, attributes: &span::Attributes<'_>) -> span::Id {
        Sometimes.new_span(attributes)
    }
    fn record(&self, id: &span::Id, values: &span::Record<'_>) {
        Sometimes.record(id, values);
    }
    fn record_follows_from(&self, id: &span::Id, follows: &span::Id) {
        Sometimes.record_follows_from(id, follows);
    }
    fn event(&self, event: &Event<'_>) {
        Sometimes.event(event);
        self.0.fetch_add(1, Ordering::Relaxed);
    }
    fn enter(&self, id: &span::Id) {
        Sometimes.enter(id);
    }
    fn exit(&self, id: &span::Id) {
        Sometimes.exit(id);
    }
}

/// One call site, fired each time.
fn fire() {
    tracing::warn!("a warning");
}

#[test]
fn an_event_first_fired_without_a_log_reaches_a_log_installed_later() {
    keep_interest();
    keep_interest();
    // No log on this thread: the call site registers, and wants asking
    // each time, not never.
    fire();
    let count = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Counting(Arc::clone(&count)), fire);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert!(KEPT.get().is_some());
}

#[test]
fn the_kept_dispatcher_wants_nothing_itself() {
    let count = Arc::new(AtomicUsize::new(0));
    tracing::subscriber::with_default(Counting(Arc::clone(&count)), || {
        let span = tracing::info_span!("work", step = tracing::field::Empty);
        span.record("step", 1);
        span.follows_from(&span);
        span.in_scope(fire);
    });
    assert_eq!(count.load(Ordering::Relaxed), 1);
    // Alone, it lets no event through.
    let kept = Dispatch::new(Sometimes);
    tracing::dispatcher::with_default(&kept, fire);
}
