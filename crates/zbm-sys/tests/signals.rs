//! Signals blocked and read from a `signalfd`, in a program of one thread:
//! the test harness runs tests in threads of their own, where a signal
//! sent to the process could reach a thread that does not block it. So
//! this test has no harness (`harness = false` in `Cargo.toml`), and its
//! checks run one after another in the main thread.

#![allow(clippy::unwrap_used, reason = "a failing check is meant to panic")]
#![allow(
    clippy::disallowed_methods,
    reason = "the checks run real programs and read the clock"
)]

use std::os::fd::AsFd as _;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use zbm_sys::{ChildSetup, Signal, SignalSet, Signals, prepare_child, send_signal};

fn main() {
    let signals = Signals::block(SignalSet::of(&[
        Signal::Terminate,
        Signal::User1,
        Signal::Hangup,
    ]))
    .unwrap();
    blocked_signals_wait_to_be_read(&signals);
    waiting_ends_with_a_signal_or_the_time(&signals);
    a_request_that_comes_again_is_forgotten(&signals);
    programs_start_with_no_signal_blocked();
}

fn me() -> u32 {
    std::process::id()
}

fn blocked_signals_wait_to_be_read(signals: &Signals) {
    assert!(signals.take().unwrap().is_empty());
    // SIGTERM would end the process, were it not blocked.
    send_signal(me(), Signal::Terminate).unwrap();
    // Readable, as a loop that polls it sees.
    let readable =
        zbm_sys::console::first_readable(&[signals.as_fd()], Some(Duration::from_secs(5)));
    assert_eq!(readable.unwrap(), Some(0));
    let taken = signals.wait(Some(Duration::from_secs(5))).unwrap();
    assert_eq!(taken, SignalSet::of(&[Signal::Terminate]));
    assert!(signals.take().unwrap().is_empty());
    assert!(signals.received().contains(Signal::Terminate));
    assert_eq!(signals.set().first(), Some(Signal::Hangup));
}

fn waiting_ends_with_a_signal_or_the_time(signals: &Signals) {
    let start = Instant::now();
    let taken = signals.wait(Some(Duration::from_millis(50))).unwrap();
    assert!(taken.is_empty());
    assert!(start.elapsed() >= Duration::from_millis(50));
    // From another process, and two at once.
    let status = Command::new("kill")
        .args(["-USR1", &me().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    send_signal(me(), Signal::Hangup).unwrap();
    let mut taken = SignalSet::EMPTY;
    let deadline = Instant::now() + Duration::from_secs(5);
    while taken != SignalSet::of(&[Signal::User1, Signal::Hangup]) {
        assert!(Instant::now() < deadline, "only {taken:?} came");
        taken = taken.union(signals.wait(Some(Duration::from_secs(1))).unwrap());
    }
}

fn a_request_that_comes_again_is_forgotten(signals: &Signals) {
    assert!(signals.received().contains(Signal::User1));
    signals.forget(Signal::User1);
    assert!(!signals.received().contains(Signal::User1));
    assert!(signals.received().contains(Signal::Terminate));
    send_signal(me(), Signal::User1).unwrap();
    signals.wait(Some(Duration::from_secs(5))).unwrap();
    assert!(signals.received().contains(Signal::User1));
}

/// The signals the program `command` starts with blocked.
fn blocked_in(mut command: Command) -> u64 {
    let output = command
        .args(["SigBlk", "/proc/self/status"])
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    u64::from_str_radix(text.trim().trim_start_matches("SigBlk:").trim(), 16).unwrap()
}

fn programs_start_with_no_signal_blocked() {
    // The standard library leaves the mask to the program it starts...
    let term = 1 << (Signal::Terminate.number() - 1);
    assert_ne!(blocked_in(Command::new("grep")) & term, 0);
    // ...which prepare_child empties.
    let mut grep = Command::new("grep");
    prepare_child(&mut grep, &ChildSetup::default());
    assert_eq!(blocked_in(grep), 0);
    // So SIGTERM ends a program this one runs.
    let mut sleep = Command::new("sleep");
    sleep.arg("30");
    prepare_child(&mut sleep, &ChildSetup::default());
    let mut child = sleep.spawn().unwrap();
    send_signal(child.id(), Signal::Terminate).unwrap();
    assert_eq!(child.wait().unwrap().signal(), Some(15));
}
