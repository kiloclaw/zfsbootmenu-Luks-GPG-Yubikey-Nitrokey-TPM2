//! One-shot replies (ADR 0001): a worker that asks the person something
//! sends the question with a [`Reply`], and waits on its [`Awaiting`] for
//! the answer, a second at a time if it has something to look at
//! meanwhile, instead of running a loop of its own on the screen.

use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::time::Duration;

/// Where the answer to one question goes.
#[derive(Debug)]
pub struct Reply<T>(SyncSender<T>);

impl<T> Reply<T> {
    /// Answers; an answer nobody waits for any more is dropped.
    pub fn send(self, answer: T) {
        let _ = self.0.send(answer);
    }
}

/// The answer to one question, to wait for.
#[derive(Debug)]
pub struct Awaiting<T>(Receiver<T>);

/// Why [`Awaiting::wait`] has no answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoAnswer {
    /// None came within the time given; one may come later.
    NotYet,
    /// None will: the [`Reply`] was dropped unanswered.
    Gone,
}

impl<T> Awaiting<T> {
    /// The answer, waiting at most `timeout` for it; without limit with
    /// `None`.
    ///
    /// # Errors
    ///
    /// [`NoAnswer::NotYet`] when none came in time, [`NoAnswer::Gone`] when
    /// none will.
    pub fn wait(&self, timeout: Option<Duration>) -> Result<T, NoAnswer> {
        match timeout {
            None => self.0.recv().map_err(|_| NoAnswer::Gone),
            Some(timeout) => self.0.recv_timeout(timeout).map_err(|error| match error {
                RecvTimeoutError::Timeout => NoAnswer::NotYet,
                RecvTimeoutError::Disconnected => NoAnswer::Gone,
            }),
        }
    }
}

/// A question's two ends: the [`Reply`] goes with the question, the
/// [`Awaiting`] stays with who asks.
pub fn reply<T>() -> (Reply<T>, Awaiting<T>) {
    let (sender, receiver) = sync_channel(1);
    (Reply(sender), Awaiting(receiver))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_comes_once_and_a_dropped_reply_says_none_will() {
        let (answer, awaiting) = reply();
        assert_eq!(awaiting.wait(Some(Duration::ZERO)), Err(NoAnswer::NotYet));
        answer.send(7);
        assert_eq!(awaiting.wait(None), Ok(7));
        assert_eq!(awaiting.wait(None), Err(NoAnswer::Gone));

        let (dropped, awaiting) = reply::<u8>();
        drop(dropped);
        assert_eq!(
            awaiting.wait(Some(Duration::from_secs(1))),
            Err(NoAnswer::Gone)
        );

        // Nobody waits any more: the answer is dropped.
        let (late, awaiting) = reply();
        drop(awaiting);
        late.send(1);
    }
}
