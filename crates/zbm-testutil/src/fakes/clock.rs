//! A clock that only moves when the code sleeps.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use zbm_system::clock::Clock;

/// The wall-clock time at which a fake clock starts: 2026-01-01 00:00:00 UTC.
pub const START: Duration = Duration::from_secs(1_767_225_600);

#[derive(Default)]
struct State {
    now: Cell<Duration>,
    sleeps: RefCell<Vec<Duration>>,
}

/// A [`Clock`] whose time advances only through [`Clock::sleep`] and
/// [`FakeClock::advance`], so countdowns and retry loops finish instantly and
/// tests can check how long the code waited. Clones share the same time, so
/// the fake console can let time pass when a key press times out.
#[derive(Clone, Default)]
pub struct FakeClock {
    state: Rc<State>,
}

impl FakeClock {
    /// A clock at time zero, at wall-clock time [`START`].
    pub fn new() -> FakeClock {
        FakeClock::default()
    }

    /// Lets `duration` pass without recording a sleep.
    pub fn advance(&self, duration: Duration) {
        self.state.now.set(self.state.now.get() + duration);
    }

    /// Every sleep so far, in order.
    pub fn sleeps(&self) -> Vec<Duration> {
        self.state.sleeps.borrow().clone()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Duration {
        self.state.now.get()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + START + self.state.now.get()
    }

    fn sleep(&self, duration: Duration) {
        self.state.sleeps.borrow_mut().push(duration);
        self.advance(duration);
    }
}
