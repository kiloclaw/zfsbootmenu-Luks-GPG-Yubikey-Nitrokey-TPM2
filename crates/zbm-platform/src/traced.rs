//! The command journal: a runner that reports every program it runs.

use std::io;
use std::time::Duration;

use tracing::{info, info_span, warn};
use zbm_core::exec::{Command, Input, Output, Runner};
use zbm_system::Clock;

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

impl<'a> TracedRunner<'a> {
    /// A runner that reports the commands `inner` runs, timed by `clock`.
    pub fn new(inner: &'a dyn Runner, clock: &'a dyn Clock) -> TracedRunner<'a> {
        TracedRunner { inner, clock }
    }
}

impl Runner for TracedRunner<'_> {
    fn run(&self, command: &Command) -> io::Result<Output> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use tracing_subscriber::layer::SubscriberExt;
    use zbm_core::exec::Status;
    use zbm_core::secret::Secret;
    use zbm_testkit::FakeRunner;
    use zbm_testkit::fakes::FakeClock;
    use zbm_trace::JournalLayer;

    /// Runs `command` through a traced fake runner whose commands take
    /// `takes`, and returns the journal.
    fn journal_of(command: &Command, status: u8, takes: Duration) -> Vec<Value> {
        let clock = FakeClock::new();
        let runner = FakeRunner::new();
        let advancing = clock.clone();
        runner.on(move |_| {
            advancing.advance(takes);
            Some(Ok(Output::new(Status::exited(status), "")))
        });
        let layer = JournalLayer::new(std::io::sink());
        let lines = layer.lines();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            let _ = TracedRunner::new(&runner, &clock).run(command);
        });
        lines.take()
    }

    #[test]
    fn every_command_is_reported_with_status_and_duration() {
        let command = Command::new("cryptsetup").args(["close", "cryptroot"]);

        let lines = journal_of(&command, 4, Duration::from_millis(250));

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["message"], "command ended");
        assert_eq!(lines[0]["fields"]["status"], 4);
        assert_eq!(lines[0]["fields"]["duration_ms"], 250);
        let span = &lines[0]["spans"][0];
        assert_eq!(span["span"], "command");
        assert_eq!(span["line"], "cryptsetup close cryptroot");
        assert_eq!(span["stdin"], "inherit");
    }

    #[test]
    fn a_secret_on_standard_input_is_reported_by_kind_only() {
        let command = Command::new("cryptsetup")
            .args(["open", "--key-file=-", "/dev/sda2", "v"])
            .stdin(Input::Secret(Secret::copy_from(b"hunter2")));

        let lines = journal_of(&command, 0, Duration::ZERO);

        assert_eq!(lines[0]["spans"][0]["stdin"], "secret");
        assert!(!lines[0].to_string().contains("hunter2"));
    }

    #[test]
    fn the_output_of_the_inner_runner_is_returned_unchanged() {
        let clock = FakeClock::new();
        let runner = FakeRunner::new();
        runner.respond("echo hi", 0, "hi\n");

        let output = TracedRunner::new(&runner, &clock)
            .run(&Command::new("echo").arg("hi"))
            .unwrap();

        assert_eq!(output.stdout_text(), "hi\n");
        assert_eq!(runner.lines(), ["echo hi"]);
    }
}
