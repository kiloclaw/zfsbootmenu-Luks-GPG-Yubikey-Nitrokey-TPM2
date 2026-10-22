//! Names of ZFS pools and datasets, with the rules of OpenZFS
//! (`module/zcommon/zfs_namecheck.c`).

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};

/// The longest dataset name, in bytes (`ZFS_MAX_DATASET_NAME_LEN` without the
/// NUL); it limits pool names and snapshot names too.
pub(crate) const MAX_LEN: usize = 255;

/// Pool names OpenZFS reserves: the vdev types, as whole names or prefixes.
const RESERVED_PREFIXES: &[&str] = &["mirror", "raidz", "draid", "spare"];

/// Whether `c` may appear in a pool name or a dataset name component.
pub(crate) fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | ' ')
}

/// The first character of `text` that may not appear in a name, if any.
fn invalid_char(text: &str) -> Option<char> {
    text.chars().find(|c| !is_name_char(*c))
}

/// The name of a ZFS pool, such as `zroot`.
///
/// It starts with a letter, has at most 255 bytes of letters, digits and
/// `-_.: `, and is not a vdev type (`mirror`, `raidz…`, `draid…`, `spare`,
/// `log`) or a name like `c0` that old Solaris device names reserve.
///
/// ```
/// use zbm_types::PoolName;
///
/// assert!("zroot".parse::<PoolName>().is_ok());
/// assert!("mirror2".parse::<PoolName>().is_err());
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PoolName(String);

impl PoolName {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PoolName {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<PoolName, ParseError> {
        check_pool(text).map_err(|reason| ParseError::new("pool name", text, reason))?;
        Ok(PoolName(text.to_owned()))
    }
}

/// Why `text` is not a valid pool name, if it is not.
fn check_pool(text: &str) -> Result<(), Reason> {
    let first = text.chars().next().ok_or(Reason::Empty)?;
    if text.len() > MAX_LEN {
        return Err(Reason::TooLong { max: MAX_LEN });
    }
    if let Some(c) = invalid_char(text) {
        return Err(Reason::InvalidCharacter(c));
    }
    if !first.is_ascii_alphabetic() {
        return Err(Reason::FirstNotLetter);
    }
    let solaris_device =
        text.len() > 1 && first == 'c' && text.chars().nth(1).is_some_and(|c| c.is_ascii_digit());
    let reserved = text == "log"
        || RESERVED_PREFIXES
            .iter()
            .any(|prefix| text.starts_with(prefix));
    if solaris_device || reserved {
        return Err(Reason::Reserved);
    }
    Ok(())
}

impl fmt::Display for PoolName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The name of a ZFS filesystem or volume: a pool name followed by
/// `/`-separated components, such as `zroot/ROOT/void`.
///
/// Snapshots (`@`) and bookmarks (`#`) are not datasets. Every component has
/// letters, digits and `-_.: `, and is not `.` or `..`; the whole name has at
/// most 255 bytes.
///
/// ```
/// use zbm_types::DatasetName;
///
/// let dataset: DatasetName = "zroot/ROOT/void".parse()?;
/// assert_eq!(dataset.pool().as_str(), "zroot");
/// assert_eq!(dataset.last_component(), "void");
/// assert!("zroot/ROOT@snap".parse::<DatasetName>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DatasetName(String);

impl DatasetName {
    /// The name as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The pool the dataset is in.
    pub fn pool(&self) -> PoolName {
        let pool = self.0.split('/').next().unwrap_or(&self.0);
        PoolName(pool.to_owned())
    }

    /// The last component, which is the pool name for a pool's root dataset.
    pub fn last_component(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }
}

impl FromStr for DatasetName {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<DatasetName, ParseError> {
        check_dataset(text).map_err(|reason| ParseError::new("dataset name", text, reason))?;
        Ok(DatasetName(text.to_owned()))
    }
}

/// Why `text` is not a valid dataset name, if it is not.
fn check_dataset(text: &str) -> Result<(), Reason> {
    if text.len() > MAX_LEN {
        return Err(Reason::TooLong { max: MAX_LEN });
    }
    let mut components = text.split('/');
    check_pool(components.next().unwrap_or_default())?;
    for component in components {
        if component.is_empty() {
            return Err(Reason::EmptyComponent);
        }
        if let Some(c) = invalid_char(component) {
            return Err(Reason::InvalidCharacter(c));
        }
        if component == "." || component == ".." {
            return Err(Reason::RelativeComponent);
        }
    }
    Ok(())
}

impl fmt::Display for DatasetName {
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

    fn pool_reason(text: &str) -> Reason {
        text.parse::<PoolName>().unwrap_err().reason().clone()
    }

    fn dataset_reason(text: &str) -> Reason {
        text.parse::<DatasetName>().unwrap_err().reason().clone()
    }

    #[test]
    fn invalid_pool_names_say_why() {
        cases! {
            for (text, expected) in {
                "empty" => ("", Reason::Empty),
                "digit first" => ("1tank", Reason::FirstNotLetter),
                "slash" => ("tank/a", Reason::InvalidCharacter('/')),
                "vdev type" => ("raidz1", Reason::Reserved),
                "log" => ("log", Reason::Reserved),
                "solaris device" => ("c0t0d0", Reason::Reserved),
            } {
                assert_eq!(pool_reason(text), expected);
            }
        }
    }

    #[test]
    fn names_that_only_resemble_reserved_ones_are_pools() {
        for text in ["logs", "c", "cache", "tank mirror"] {
            assert!(text.parse::<PoolName>().is_ok(), "{text}");
        }
    }

    #[test]
    fn invalid_dataset_names_say_why() {
        cases! {
            for (text, expected) in {
                "trailing slash" => ("zroot/", Reason::EmptyComponent),
                "double slash" => ("zroot//a", Reason::EmptyComponent),
                "snapshot" => ("zroot/a@s", Reason::InvalidCharacter('@')),
                "bookmark" => ("zroot/a#b", Reason::InvalidCharacter('#')),
                "parent" => ("zroot/..", Reason::RelativeComponent),
                "bad pool" => ("9/a", Reason::FirstNotLetter),
            } {
                assert_eq!(dataset_reason(text), expected);
            }
        }
    }

    #[test]
    fn a_pool_root_is_a_dataset_whose_last_component_is_the_pool() {
        let dataset: DatasetName = "zroot".parse().unwrap();

        assert_eq!(dataset.pool().as_str(), "zroot");
        assert_eq!(dataset.last_component(), "zroot");
    }

    #[test]
    fn spaces_are_allowed_inside_names() {
        let dataset: DatasetName = "zroot/ROOT/my system".parse().unwrap();

        assert_eq!(dataset.last_component(), "my system");
    }

    proptest! {
        #[test]
        fn valid_dataset_names_round_trip(
            pool in "z[a-z0-9_.:-]{0,10}",
            parts in proptest::collection::vec("[A-Za-z0-9_:-][A-Za-z0-9_.: -]{0,10}", 0..4),
        ) {
            let text = std::iter::once(pool.clone()).chain(parts).collect::<Vec<_>>().join("/");

            let dataset: DatasetName = text.parse().unwrap();

            prop_assert_eq!(dataset.to_string(), text);
            prop_assert_eq!(dataset.pool().to_string(), pool);
        }

        #[test]
        fn parsing_any_text_never_panics(text in "\\PC*") {
            let _ = text.parse::<PoolName>();
            let _ = text.parse::<DatasetName>();
        }
    }
}
