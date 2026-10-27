//! Names of device-mapper mappings.

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// The longest name the device mapper accepts, in bytes (`DM_NAME_LEN`
/// without the NUL).
const MAX_LEN: usize = 127;

/// What [`ParseError::what`] says for a mapping name.
const WHAT: &str = "mapping name";

/// The name of a device-mapper mapping: the `NAME` of
/// `cryptsetup open DEVICE NAME`, which appears as `/dev/mapper/NAME`.
///
/// It has 1 to 127 printable ASCII characters other than `/` and the space,
/// does not start with `-`, which `cryptsetup` would read as an option, and
/// is not `.` or `..`. The device mapper itself accepts a few more, but it
/// rewrites them in the names udev creates, so ZFSBootMenu does not use
/// them.
///
/// ```
/// use zbm_types::MappingName;
///
/// let name: MappingName = "luks-0b8e1c4a".parse()?;
/// assert_eq!(name.mapper_path(), "/dev/mapper/luks-0b8e1c4a");
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MappingName(String);

impl MappingName {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The device node of the mapping: `/dev/mapper/NAME`.
    pub fn mapper_path(&self) -> String {
        format!("/dev/mapper/{}", self.0)
    }
}

impl FromStr for MappingName {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<MappingName, ParseError> {
        let error = |reason| ParseError::new(WHAT, text, reason);
        if text.is_empty() {
            return Err(error(Reason::Empty));
        }
        if text.len() > MAX_LEN {
            return Err(error(Reason::TooLong { max: MAX_LEN }));
        }
        if let Some(c) = text.chars().find(|c| !c.is_ascii_graphic() || *c == '/') {
            return Err(error(Reason::InvalidCharacter(c)));
        }
        if text == "." || text == ".." {
            return Err(error(Reason::Reserved));
        }
        if text.starts_with('-') {
            return Err(error(Reason::LeadingDash));
        }
        Ok(MappingName(text.to_owned()))
    }
}

impl fmt::Display for MappingName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use zbm_testkit::cases;

    fn reason(text: &str) -> Reason {
        text.parse::<MappingName>().unwrap_err().reason().clone()
    }

    #[test]
    fn the_mapper_path_is_below_dev_mapper() {
        let name: MappingName = "cryptroot".parse().unwrap();

        assert_eq!(name.mapper_path(), "/dev/mapper/cryptroot");
    }

    #[test]
    fn invalid_names_say_why() {
        cases! {
            for (text, expected) in {
                "empty" => ("", Reason::Empty),
                "slash" => ("a/b", Reason::InvalidCharacter('/')),
                "space" => ("a b", Reason::InvalidCharacter(' ')),
                "newline" => ("a\n", Reason::InvalidCharacter('\n')),
                "non-ascii" => ("é", Reason::InvalidCharacter('é')),
                "dot" => (".", Reason::Reserved),
                "dot dot" => ("..", Reason::Reserved),
                "an option" => ("--help", Reason::LeadingDash),
            } {
                assert_eq!(reason(text), expected);
            }
        }
    }

    #[test]
    fn names_may_have_127_bytes_but_not_128() {
        assert!("a".repeat(127).parse::<MappingName>().is_ok());
        assert_eq!(reason(&"a".repeat(128)), Reason::TooLong { max: 127 });
    }

    proptest! {
        #[test]
        fn valid_names_round_trip(text in "[A-Za-z0-9_+:@][A-Za-z0-9_.+:@-]{0,60}") {
            let name: MappingName = text.parse().unwrap();

            prop_assert_eq!(name.to_string(), text);
        }

        #[test]
        fn parsing_any_text_never_panics(text in "\\PC*") {
            let _ = text.parse::<MappingName>();
        }
    }
}
