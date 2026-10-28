//! The numbers of ZFSBootMenu's own options: how long the menu waits
//! (`zbm.timeout`), the delay between retries (`zbm.retry_delay`) and the
//! log level of its messages (`loglevel`). Each reads its value as the
//! shell's `test` read it in the legacy code ([`shell_integer`]), and prints
//! as the number it holds.

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};
use crate::number::shell_integer;

/// The integer of `text`, or the error that says what `what` needs.
fn integer(what: &'static str, text: &str) -> Result<i64, ParseError> {
    shell_integer(text).ok_or_else(|| ParseError::new(what, text, Reason::NotAnInteger))
}

/// How long the menu waits before it boots the default boot environment
/// (`zbm.timeout`, `zbm.show`, `zbm.skip`).
///
/// ```
/// use zbm_types::Timeout;
///
/// assert_eq!("3".parse::<Timeout>()?, Timeout::Seconds(3));
/// assert_eq!("-1".parse::<Timeout>()?, Timeout::Wait);
/// assert_eq!(Timeout::Wait.to_string(), "-1");
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Timeout {
    /// This many seconds; 0 boots at once.
    Seconds(u64),
    /// Until a key is pressed: any negative number, written `-1`.
    Wait,
}

impl Timeout {
    /// Ten seconds, when nothing is given.
    pub const DEFAULT: Timeout = Timeout::Seconds(10);

    /// The seconds, unless it waits.
    pub fn seconds(self) -> Option<u64> {
        match self {
            Timeout::Seconds(seconds) => Some(seconds),
            Timeout::Wait => None,
        }
    }
}

impl Default for Timeout {
    fn default() -> Timeout {
        Timeout::DEFAULT
    }
}

impl FromStr for Timeout {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Timeout, ParseError> {
        let number = integer("menu timeout", text)?;
        Ok(u64::try_from(number).map_or(Timeout::Wait, Timeout::Seconds))
    }
}

impl fmt::Display for Timeout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Timeout::Seconds(seconds) => write!(formatter, "{seconds}"),
            Timeout::Wait => formatter.write_str("-1"),
        }
    }
}

/// The seconds between import attempts and device checks
/// (`zbm.retry_delay`): a positive number.
///
/// ```
/// use zbm_types::RetryDelay;
///
/// assert_eq!("7".parse::<RetryDelay>()?.seconds(), 7);
/// assert!("0".parse::<RetryDelay>().is_err());
/// assert_eq!(RetryDelay::or_default("soon").seconds(), 5);
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RetryDelay(u64);

impl RetryDelay {
    /// Five seconds, when nothing valid is given.
    pub const DEFAULT: RetryDelay = RetryDelay(5);

    /// A delay of `seconds`, if it is positive.
    pub fn new(seconds: u64) -> Option<RetryDelay> {
        (seconds > 0).then_some(RetryDelay(seconds))
    }

    /// The delay `text` gives, or five seconds when it is not a positive
    /// number, as `zbm_retry_delay` was read. The legacy prompt read a
    /// value that is not a number as 0, which answers at once, so a
    /// missing device was looked for again and again without a pause.
    pub fn or_default(text: &str) -> RetryDelay {
        text.parse().unwrap_or_default()
    }

    /// The delay in seconds.
    pub fn seconds(self) -> u64 {
        self.0
    }
}

impl Default for RetryDelay {
    fn default() -> RetryDelay {
        RetryDelay::DEFAULT
    }
}

impl FromStr for RetryDelay {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<RetryDelay, ParseError> {
        let number = integer("retry delay", text)?;
        u64::try_from(number)
            .ok()
            .and_then(RetryDelay::new)
            .ok_or_else(|| ParseError::new("retry delay", text, Reason::Below(1)))
    }
}

impl fmt::Display for RetryDelay {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// The kernel log level of ZFSBootMenu's messages (`loglevel`): messages
/// up to it reach the console. At least 4, so that errors and warnings
/// always do.
///
/// ```
/// use zbm_types::LogLevel;
///
/// assert_eq!("7".parse::<LogLevel>()?.get(), 7);
/// assert!("3".parse::<LogLevel>().is_err());
/// assert_eq!(LogLevel::default().get(), 4);
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LogLevel(u32);

impl LogLevel {
    /// The lowest level: errors and warnings.
    pub const MIN: LogLevel = LogLevel(4);

    /// The level.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl Default for LogLevel {
    fn default() -> LogLevel {
        LogLevel::MIN
    }
}

impl FromStr for LogLevel {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<LogLevel, ParseError> {
        let number = integer("log level", text)?;
        u32::try_from(number)
            .ok()
            .filter(|level| *level >= LogLevel::MIN.0)
            .map(LogLevel)
            .ok_or_else(|| ParseError::new("log level", text, Reason::Below(4)))
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn values_read_as_the_shell_reads_them() {
        assert_eq!(" 5 ".parse::<Timeout>().unwrap(), Timeout::Seconds(5));
        assert_eq!("-7".parse::<Timeout>().unwrap(), Timeout::Wait);
        assert_eq!("0".parse::<Timeout>().unwrap(), Timeout::Seconds(0));
        assert!("ten".parse::<Timeout>().is_err());
        assert_eq!(Timeout::default().seconds(), Some(10));
        assert_eq!(Timeout::Wait.seconds(), None);
        assert_eq!("+3".parse::<RetryDelay>().unwrap().seconds(), 3);
        assert!("-3".parse::<RetryDelay>().is_err());
        assert_eq!(RetryDelay::or_default("").seconds(), 5);
        assert_eq!(RetryDelay::new(0), None);
        assert_eq!(LogLevel::MIN.to_string(), "4");
        let error = "2".parse::<LogLevel>().unwrap_err();
        assert_eq!(
            error.to_string(),
            "\"2\" is not a valid log level: it is below 4"
        );
        let error = "x".parse::<RetryDelay>().unwrap_err();
        assert_eq!(
            error.to_string(),
            "\"x\" is not a valid retry delay: it is not an integer"
        );
    }

    proptest! {
        #[test]
        fn values_round_trip(seconds in 0_u64..1_000_000, level in 4_u32..100) {
            let timeout = Timeout::Seconds(seconds);
            prop_assert_eq!(timeout.to_string().parse::<Timeout>().unwrap(), timeout);
            if let Some(delay) = RetryDelay::new(seconds) {
                prop_assert_eq!(delay.to_string().parse::<RetryDelay>().unwrap(), delay);
            }
            let level = LogLevel(level);
            prop_assert_eq!(level.to_string().parse::<LogLevel>().unwrap(), level);
        }
    }
}
