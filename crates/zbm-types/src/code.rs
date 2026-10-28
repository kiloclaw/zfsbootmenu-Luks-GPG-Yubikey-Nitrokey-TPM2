//! Stable error codes: `ZBM-E0412` names one kind of error, whatever its
//! values, in the kernel log, on standard error and in `zbm explain`.
//!
//! Each crate gives its error enums codes with
//! [`error_codes!`](crate::error_codes), in a range of
//! its own: 02xx `zbm-zfs`, 03xx `zbm-pool`, 04xx `zbm-luks`, 05xx
//! `zbm-crypt`, 06xx `zbm-boot`, 07xx `zbm-startup`. The macro matches every
//! variant, so a new variant does not compile without a code. A code is never
//! given to another error: when an error goes, its number is not used again.
//!
//! ```
//! use zbm_types::code::{Coded, ErrorCode, report};
//!
//! #[derive(Debug, thiserror::Error)]
//! enum DemoError {
//!     #[error("unable to mount {0}")]
//!     Mount(String),
//! }
//!
//! zbm_types::error_codes! {
//!     DemoError, DEMO_CODES;
//!     Mount = 603 => "a boot environment could not be mounted":
//!         "Check that the dataset exists and that its key is loaded.",
//! }
//!
//! let error = DemoError::Mount("zroot/ROOT/void".to_owned());
//! assert_eq!(error.code(), ErrorCode::new(603));
//! assert_eq!(report(&error), "ZBM-E0603: unable to mount zroot/ROOT/void");
//! assert_eq!(DEMO_CODES[0].code.to_string(), "ZBM-E0603");
//! ```

use std::error::Error;
use std::fmt;

/// A stable error code, shown as `ZBM-E0412`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ErrorCode(u16);

impl ErrorCode {
    /// The code numbered `number`.
    pub const fn new(number: u16) -> ErrorCode {
        ErrorCode(number)
    }

    /// Its number: 412 for `ZBM-E0412`.
    pub const fn number(self) -> u16 {
        self.0
    }

    /// The code `text` names: `ZBM-E0412`, `E0412`, `0412` or `412`, in any
    /// case; `None` for anything else.
    pub fn parse(text: &str) -> Option<ErrorCode> {
        let text = text.trim().to_ascii_uppercase();
        let digits = text.strip_prefix("ZBM-").unwrap_or(&text);
        let digits = digits.strip_prefix('E').unwrap_or(digits);
        if digits.is_empty() || digits.len() > 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(ErrorCode)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ZBM-E{:04}", self.0)
    }
}

/// What a code means, for `zbm explain`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Explanation {
    /// The code.
    pub code: ErrorCode,
    /// What went wrong, in a few words.
    pub summary: &'static str,
    /// What it means and what to do about it.
    pub text: &'static str,
}

/// An error with a stable code.
pub trait Coded: Error {
    /// The code of this error.
    fn code(&self) -> ErrorCode;
}

/// `error` and its causes, each once, joined by `: `. A cause whose text
/// already ends its error's message, as `"{source}"` in a message puts it, is
/// not repeated.
pub fn chain(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut last = text.clone();
    let mut source = error.source();
    while let Some(cause) = source {
        let message = cause.to_string();
        if !message.is_empty() && !last.ends_with(&message) {
            text.push_str(": ");
            text.push_str(&message);
        }
        last = message;
        source = cause.source();
    }
    text
}

/// `error` with its code and causes, as one line:
/// `ZBM-E0412: no key method opened /dev/sda2: ...`.
pub fn report(error: &dyn Coded) -> String {
    format!("{}: {}", error.code(), chain(error))
}

/// Gives the variants of an error enum their codes and their explanations:
/// implements [`Coded`] for the enum, and defines a table of
/// [`Explanation`]s.
///
/// Each variant is `Name = number => "summary": "text"`. A variant that wraps
/// another coded error, as `#[error(transparent)]` does, is listed in
/// `delegate [...]` after the table's name, and takes that error's code.
#[macro_export]
macro_rules! error_codes {
    (
        $error:ty, $table:ident $(, delegate [$($inner:ident),+])?;
        $($variant:ident = $number:literal => $summary:literal : $text:expr),* $(,)?
    ) => {
        impl $crate::code::Coded for $error {
            fn code(&self) -> $crate::code::ErrorCode {
                match self {
                    $($(Self::$inner(inner) => $crate::code::Coded::code(inner),)+)?
                    $(Self::$variant { .. } => $crate::code::ErrorCode::new($number),)*
                }
            }
        }

        #[doc = concat!("The codes of [`", stringify!($error), "`], for `zbm explain`.")]
        pub const $table: &[$crate::code::Explanation] = &[
            $($crate::code::Explanation {
                code: $crate::code::ErrorCode::new($number),
                summary: $summary,
                text: $text,
            },)*
        ];
    };
}

#[cfg(test)]
mod tests;
