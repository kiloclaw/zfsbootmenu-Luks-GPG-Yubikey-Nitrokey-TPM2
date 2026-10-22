//! Why a text is not a valid name or path.

use std::fmt;

/// A text that could not be parsed as one of the types of this crate.
///
/// It keeps the input and the reason, so a message can say exactly what was
/// wrong:
///
/// ```
/// use zbm_types::{MappingName, Reason};
///
/// let error = "a/b".parse::<MappingName>().unwrap_err();
/// assert_eq!(error.reason(), &Reason::InvalidCharacter('/'));
/// assert_eq!(
///     error.to_string(),
///     "\"a/b\" is not a valid mapping name: it contains '/'"
/// );
/// ```
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{input:?} is not a valid {what}: {reason}")]
pub struct ParseError {
    /// What was being parsed, for example "mapping name".
    what: &'static str,
    /// The text that was parsed.
    input: String,
    /// Why it is not valid.
    reason: Reason,
}

impl ParseError {
    /// An error for `input`, which is not a valid `what` because of `reason`.
    pub(crate) fn new(what: &'static str, input: &str, reason: Reason) -> ParseError {
        ParseError {
            what,
            input: input.to_owned(),
            reason,
        }
    }

    /// What was being parsed, for example "mapping name".
    pub fn what(&self) -> &'static str {
        self.what
    }

    /// The text that was parsed.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// Why the text is not valid.
    pub fn reason(&self) -> &Reason {
        &self.reason
    }
}

/// Why a text is not valid.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Reason {
    /// The text is empty.
    Empty,
    /// The text is longer than `max` bytes.
    TooLong {
        /// The longest allowed, in bytes.
        max: usize,
    },
    /// The text contains a character that is not allowed.
    InvalidCharacter(char),
    /// A path does not start with `/`.
    NotAbsolute,
    /// A path has a `.` or `..` component.
    RelativeComponent,
    /// A name is reserved, such as `.` or the vdev type `mirror`.
    Reserved,
    /// A pool name does not start with a letter.
    FirstNotLetter,
    /// A dataset name has an empty component, as in `pool//data` or `pool/`.
    EmptyComponent,
}

impl fmt::Display for Reason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reason::Empty => write!(formatter, "it is empty"),
            Reason::TooLong { max } => write!(formatter, "it is longer than {max} bytes"),
            Reason::InvalidCharacter(c) => write!(formatter, "it contains {c:?}"),
            Reason::NotAbsolute => write!(formatter, "it does not start with '/'"),
            Reason::RelativeComponent => write!(formatter, "it has a '.' or '..' component"),
            Reason::Reserved => write!(formatter, "the name is reserved"),
            Reason::FirstNotLetter => write!(formatter, "it does not start with a letter"),
            Reason::EmptyComponent => write!(formatter, "it has an empty component"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_names_the_input_the_type_and_the_reason() {
        let error = ParseError::new("pool name", "1tank", Reason::FirstNotLetter);

        assert_eq!(
            error.to_string(),
            "\"1tank\" is not a valid pool name: it does not start with a letter"
        );
        assert_eq!(error.what(), "pool name");
        assert_eq!(error.input(), "1tank");
    }
}
