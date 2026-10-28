//! Starting threads: the only place a thread is started, so that every one
//! logs where the thread that started it does.

#![allow(
    clippy::disallowed_methods,
    reason = "where threads start, carrying the tracing of the thread that starts them"
)]

use std::io;
use std::thread::{Builder, JoinHandle, Scope, ScopedJoinHandle};

use tracing::{Dispatch, Span};

/// What a worker takes from the thread that starts it: where its messages
/// go (the kernel log and the journal installed for that thread) and the
/// span they belong to.
struct Tracing {
    dispatch: Dispatch,
    span: Span,
}

impl Tracing {
    /// The tracing of the current thread.
    fn current() -> Tracing {
        Tracing {
            dispatch: tracing::dispatcher::get_default(Dispatch::clone),
            span: Span::current(),
        }
    }

    /// Runs `work` with this tracing.
    fn run<T>(self, work: impl FnOnce() -> T) -> T {
        tracing::dispatcher::with_default(&self.dispatch, || self.span.in_scope(work))
    }
}

/// Starts `work` on a thread named `name`, which logs to where this thread
/// logs, inside this thread's current span.
///
/// # Errors
///
/// When the thread cannot be created.
pub fn spawn_traced<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> io::Result<JoinHandle<T>> {
    let tracing = Tracing::current();
    Builder::new()
        .name(name.to_owned())
        .spawn(move || tracing.run(work))
}

/// Starts `work` in `scope`, as [`spawn_traced`] starts a thread: it may
/// borrow from outside the scope, which waits for it.
pub fn spawn_scoped_traced<'scope, T: Send + 'scope>(
    scope: &'scope Scope<'scope, '_>,
    work: impl FnOnce() -> T + Send + 'scope,
) -> ScopedJoinHandle<'scope, T> {
    let tracing = Tracing::current();
    scope.spawn(move || tracing.run(work))
}

#[cfg(test)]
mod tests;
