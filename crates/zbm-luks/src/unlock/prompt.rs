//! The passphrase prompt as a state machine.
//!
//! A volume gets `tries` passphrase attempts per round (0: without limit).
//! When a round fails, a timed prompt asks whether to try again (a new round)
//! or to skip the volume. [`PassphrasePrompt`] holds the round and the
//! attempt, and its transitions say what comes next; they do no I/O, so the
//! driver in [`crate::unlock`] only reads, opens and draws.
//!
//! ```
//! use zbm_luks::unlock::prompt::{Next, PassphrasePrompt};
//! use zbm_types::Answer;
//!
//! let mut prompt = PassphrasePrompt::new(2);
//! assert_eq!(prompt.start(), Next::Ask { attempt: 1 });
//! assert_eq!(prompt.failed(), Next::Ask { attempt: 2 });
//! assert_eq!(prompt.failed(), Next::AskRetry);
//! assert_eq!(prompt.retry(Answer::Continue), Next::Ask { attempt: 1 });
//! assert_eq!(prompt.failed(), Next::Ask { attempt: 2 });
//! assert_eq!(prompt.failed(), Next::AskRetry);
//! assert_eq!(prompt.retry(Answer::Escape), Next::GiveUp);
//! ```

use zbm_types::Answer;

/// What the prompt does next.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Next {
    /// Ask for a passphrase; an attempt after the first says the last one
    /// did not open the volume.
    Ask {
        /// The attempt in this round, from 1.
        attempt: u64,
    },
    /// The round failed: ask whether to try again or to skip the volume.
    AskRetry,
    /// The user chose to skip the volume.
    GiveUp,
}

/// The state of the passphrase prompt of one volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PassphrasePrompt {
    tries: u64,
    attempt: u64,
}

impl PassphrasePrompt {
    /// A prompt with `tries` attempts per round; 0 means without limit.
    pub fn new(tries: u64) -> PassphrasePrompt {
        PassphrasePrompt { tries, attempt: 0 }
    }

    /// The first attempt.
    pub fn start(&mut self) -> Next {
        self.attempt = 0;
        self.next_attempt()
    }

    /// The transition after a passphrase that did not open the volume.
    pub fn failed(&mut self) -> Next {
        self.next_attempt()
    }

    /// The transition after the user answered the retry prompt.
    pub fn retry(&mut self, answer: Answer) -> Next {
        match answer {
            Answer::Continue => self.start(),
            Answer::Escape => Next::GiveUp,
        }
    }

    fn next_attempt(&mut self) -> Next {
        if self.tries == 0 || self.attempt < self.tries {
            self.attempt += 1;
            Next::Ask {
                attempt: self.attempt,
            }
        } else {
            Next::AskRetry
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_limit_the_prompt_keeps_asking() {
        let mut prompt = PassphrasePrompt::new(0);
        assert_eq!(prompt.start(), Next::Ask { attempt: 1 });
        for attempt in 2..10 {
            assert_eq!(prompt.failed(), Next::Ask { attempt });
        }
    }

    #[test]
    fn one_try_asks_to_retry_after_each_failure() {
        let mut prompt = PassphrasePrompt::new(1);
        assert_eq!(prompt.start(), Next::Ask { attempt: 1 });
        assert_eq!(prompt.failed(), Next::AskRetry);
        assert_eq!(prompt.retry(Answer::Continue), Next::Ask { attempt: 1 });
        assert_eq!(prompt.failed(), Next::AskRetry);
        assert_eq!(prompt.retry(Answer::Escape), Next::GiveUp);
    }
}
