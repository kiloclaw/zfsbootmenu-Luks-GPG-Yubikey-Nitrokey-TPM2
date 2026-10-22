//! Tests of the command journal, with a fake runner and clock.
//!
//! They are integration tests because the fakes of `zbm-testkit` implement
//! the traits of this crate as other crates see it.

#![allow(
    clippy::disallowed_methods,
    clippy::unwrap_used,
    reason = "integration tests set up real files and fail loudly when they cannot"
)]

use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use tracing_subscriber::layer::SubscriberExt;
use zbm_platform::{Command, Input, Output, Runner, Secret, Status, TracedRunner};
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
