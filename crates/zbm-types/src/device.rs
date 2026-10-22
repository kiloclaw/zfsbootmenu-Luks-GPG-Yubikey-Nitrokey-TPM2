//! Paths of block devices.

use std::fmt;
use std::path::{Component, Path};
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// The longest path Linux accepts, in bytes (`PATH_MAX` without the NUL).
const MAX_LEN: usize = 4095;

/// What [`ParseError::what`] says for a device path.
const WHAT: &str = "device path";

/// A block device as an absolute path, such as `/dev/sda2` or
/// `/dev/disk/by-uuid/0b8e…`.
///
/// It is absolute, has no `.` or `..` component and no NUL byte, and is
/// valid UTF-8. Whether the device exists is a question for the system, not
/// for this type.
///
/// ```
/// use zbm_types::DevicePath;
///
/// let device: DevicePath = "/dev/disk/by-id/virtio-d0".parse()?;
/// assert_eq!(device.as_str(), "/dev/disk/by-id/virtio-d0");
/// assert!("dev/sda".parse::<DevicePath>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DevicePath(String);

impl DevicePath {
    /// The device as a [`Path`].
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    /// The device as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for DevicePath {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<DevicePath, ParseError> {
        let error = |reason| ParseError::new(WHAT, text, reason);
        if text.is_empty() {
            return Err(error(Reason::Empty));
        }
        if text.len() > MAX_LEN {
            return Err(error(Reason::TooLong { max: MAX_LEN }));
        }
        if text.contains('\0') {
            return Err(error(Reason::InvalidCharacter('\0')));
        }
        if !text.starts_with('/') {
            return Err(error(Reason::NotAbsolute));
        }
        let relative = Path::new(text)
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir));
        // `Path::components` drops interior `.` components, so look for them
        // in the text as well.
        if relative || text.split('/').any(|part| part == ".") {
            return Err(error(Reason::RelativeComponent));
        }
        Ok(DevicePath(text.to_owned()))
    }
}

impl fmt::Display for DevicePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<Path> for DevicePath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn reason(text: &str) -> Reason {
        text.parse::<DevicePath>().unwrap_err().reason().clone()
    }

    #[test]
    fn absolute_paths_are_devices() {
        let device: DevicePath = "/dev/mapper/luks-1".parse().unwrap();

        assert_eq!(device.as_str(), "/dev/mapper/luks-1");
        assert_eq!(device.to_string(), "/dev/mapper/luks-1");
    }

    #[test]
    fn invalid_paths_say_why() {
        let cases = [
            ("empty", "", Reason::Empty),
            ("relative", "dev/sda", Reason::NotAbsolute),
            ("parent", "/dev/../etc/shadow", Reason::RelativeComponent),
            ("current", "/dev/./sda", Reason::RelativeComponent),
            ("nul", "/dev/sd\0a", Reason::InvalidCharacter('\0')),
        ];

        for (name, text, expected) in cases {
            assert_eq!(reason(text), expected, "case {name}");
        }
    }

    #[test]
    fn a_path_of_4096_bytes_is_too_long() {
        let text = format!("/{}", "a".repeat(MAX_LEN));

        assert_eq!(reason(&text), Reason::TooLong { max: MAX_LEN });
    }

    proptest! {
        #[test]
        fn valid_paths_round_trip(parts in proptest::collection::vec("[a-z0-9_-]{1,12}", 1..6)) {
            let text = format!("/{}", parts.join("/"));

            let device: DevicePath = text.parse().unwrap();

            prop_assert_eq!(device.to_string(), text);
        }

        #[test]
        fn parsing_any_text_never_panics(text in "\\PC*") {
            let _ = text.parse::<DevicePath>();
        }
    }
}
