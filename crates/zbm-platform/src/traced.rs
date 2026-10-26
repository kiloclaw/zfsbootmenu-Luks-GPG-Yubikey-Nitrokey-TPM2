//! The command journal: a runner that reports every program it runs.

use std::cell::Cell;
use std::io;
use std::time::Duration;

use crate::Clock;
use crate::exec::{Command, Input, Output, Runner};
use tracing::{info, info_span, warn};

/// A [`Runner`] that runs commands through another runner and reports each
/// one to `tracing`: a `command` span with the program, its arguments and
/// what it reads on standard input, and an event with its exit status and
/// duration when it ends.
///
/// Standard input is reported by kind only (`secret`, `bytes`, `null`,
/// `inherit`), never by content, and arguments never hold secrets (see
/// "Secrets" in `crates/DESIGN.md`), so the journal holds none either.
#[derive(Clone, Copy)]
pub struct TracedRunner<'a> {
    /// The runner that runs the commands.
    inner: &'a dyn Runner,
    /// The clock that times them.
    clock: &'a dyn Clock,
}

thread_local! {
    /// Whether a traced runner of this thread runs a command now: one traced
    /// runner around another then reports each command once, as the outer.
    static TRACING: Cell<bool> = const { Cell::new(false) };
}

/// Marks this thread as running a traced command until dropped.
struct Tracing;

impl Tracing {
    /// `None` when this thread runs a traced command already.
    fn start() -> Option<Tracing> {
        (!TRACING.replace(true)).then_some(Tracing)
    }
}

impl Drop for Tracing {
    fn drop(&mut self) {
        TRACING.set(false);
    }
}

impl<'a> TracedRunner<'a> {
    /// A runner that reports the commands `inner` runs, timed by `clock`.
    pub fn new(inner: &'a dyn Runner, clock: &'a dyn Clock) -> TracedRunner<'a> {
        TracedRunner { inner, clock }
    }
}

impl Runner for TracedRunner<'_> {
    fn run(&self, command: &Command) -> io::Result<Output> {
        // A traced runner around this one reports the command already.
        let Some(_tracing) = Tracing::start() else {
            return self.inner.run(command);
        };
        let program = command.program().to_string_lossy();
        let span = info_span!(
            "command",
            program = %program,
            line = %command.display(),
            stdin = stdin_kind(command.get_stdin()),
        );
        let _entered = span.enter();
        let start = self.clock.now();
        let result = self.inner.run(command);
        let duration_ms = millis(self.clock.now().saturating_sub(start));
        match &result {
            Ok(output) => {
                info!(
                    status = output.status.shell_code(),
                    duration_ms, "command ended"
                );
            }
            Err(error) => warn!(%error, duration_ms, "command could not start"),
        }
        result
    }
}

/// What a command reads on standard input, without the content.
fn stdin_kind(input: &Input) -> &'static str {
    match input {
        Input::Inherit => "inherit",
        Input::Null => "null",
        Input::Bytes(_) => "bytes",
        Input::Secret(_) => "secret",
    }
}

/// `duration` in whole milliseconds, saturating.
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
