use std::sync::Mutex;
use std::time::SystemTime;

use super::*;
use crate::clock::SystemClock;
use crate::event::{CancelToken, Mailbox};

/// A clock that stands still at the time it is set to.
struct Stopped(Mutex<Duration>);

impl Clock for Stopped {
    fn now(&self) -> Duration {
        *self.0.lock().unwrap()
    }
    fn wall(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + self.now()
    }
    fn sleep(&self, duration: Duration) {
        *self.0.lock().unwrap() += duration;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Key {
    Mail,
    Cancel,
    Tick,
    Countdown,
}

#[test]
fn ready_sources_and_passed_deadlines_wake_it() {
    let mailbox = Mailbox::<u8>::new().unwrap();
    let token = CancelToken::new().unwrap();
    let clock = Stopped(Mutex::new(Duration::from_secs(10)));
    let mut reactor = Reactor::new();
    reactor.watch(&mailbox, Key::Mail);
    reactor.watch(&token, Key::Cancel);
    reactor.at(Duration::from_secs(12), Key::Countdown);
    reactor.at(Duration::from_secs(9), Key::Tick);

    // A deadline that passed already: due at once, the earliest first.
    reactor.at(Duration::from_secs(8), Key::Countdown);
    assert_eq!(
        reactor.wait(&clock).unwrap(),
        [Wake::Due(Key::Countdown), Wake::Due(Key::Tick)]
    );

    mailbox.sender().send(1).unwrap();
    token.cancel();
    assert_eq!(
        reactor.wait(&clock).unwrap(),
        [Wake::Ready(Key::Mail), Wake::Ready(Key::Cancel)]
    );
    assert_eq!(mailbox.take(), [1]);

    reactor.unwatch(Key::Cancel);
    reactor.at(Duration::from_secs(10), Key::Tick);
    reactor.forget(Key::Tick);
    // Nothing to wait for: it returns at once.
    reactor.unwatch(Key::Mail);
    assert!(reactor.wait(&clock).unwrap().is_empty());
}

#[test]
fn it_sleeps_until_the_nearest_deadline() {
    let clock = SystemClock::new();
    let mailbox = Mailbox::<u8>::new().unwrap();
    let mut reactor = Reactor::new();
    reactor.watch(&mailbox, Key::Mail);
    let start = clock.now();
    reactor.at(start + Duration::from_millis(30), Key::Tick);
    reactor.at(start + Duration::from_secs(60), Key::Countdown);
    let mut woke = Vec::new();
    while woke.is_empty() {
        woke = reactor.wait(&clock).unwrap();
    }
    assert_eq!(woke, [Wake::Due(Key::Tick)]);
    assert!(clock.now() >= start + Duration::from_millis(30));
}

#[test]
fn a_closed_source_is_ready() {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(writer);
    assert!(readable(&reader, None).unwrap());
    assert_eq!(
        poll_readable(&[], Some(Duration::ZERO)).unwrap(),
        Vec::<bool>::new()
    );
}
