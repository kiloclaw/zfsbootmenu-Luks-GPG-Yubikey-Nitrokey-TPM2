//! Names of ZFS snapshots.

use std::fmt;
use std::str::FromStr;

use crate::error::{ParseError, Reason};
use crate::zfs::{MAX_LEN, is_name_char};
use crate::{DatasetName, PoolName};

/// The name of a ZFS snapshot: a dataset name, `@`, and the snapshot's own
/// name, such as `zroot/ROOT/void@2026-10-22`.
///
/// The dataset part is a valid [`DatasetName`]; the snapshot part is not
/// empty and has letters, digits and `-_.: `; the whole name has at most 255
/// bytes.
///
/// ```
/// use zbm_types::SnapshotName;
///
/// let snapshot: SnapshotName = "zroot/ROOT/void@before-upgrade".parse()?;
/// assert_eq!(snapshot.dataset().as_str(), "zroot/ROOT/void");
/// assert_eq!(snapshot.name(), "before-upgrade");
/// assert!("zroot/ROOT/void".parse::<SnapshotName>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SnapshotName {
    /// The dataset the snapshot is of.
    dataset: DatasetName,
    /// The part after `@`.
    name: String,
}

impl SnapshotName {
    /// The dataset the snapshot is of.
    pub fn dataset(&self) -> &DatasetName {
        &self.dataset
    }

    /// The pool the snapshot is in.
    pub fn pool(&self) -> PoolName {
        self.dataset.pool()
    }

    /// The snapshot's own name: the part after `@`.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl FromStr for SnapshotName {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<SnapshotName, ParseError> {
        let error = |reason| ParseError::new("snapshot name", text, reason);
        if text.len() > MAX_LEN {
            return Err(error(Reason::TooLong { max: MAX_LEN }));
        }
        let Some((dataset, name)) = text.split_once('@') else {
            return Err(error(Reason::NotOneAt));
        };
        if name.contains('@') {
            return Err(error(Reason::NotOneAt));
        }
        let dataset: DatasetName = dataset.parse().map_err(|cause: ParseError| {
            ParseError::new("snapshot name", text, cause.reason().clone())
        })?;
        if name.is_empty() {
            return Err(error(Reason::Empty));
        }
        if let Some(c) = name.chars().find(|c| !is_name_char(*c)) {
            return Err(error(Reason::InvalidCharacter(c)));
        }
        Ok(SnapshotName {
            dataset,
            name: name.to_owned(),
        })
    }
}

impl fmt::Display for SnapshotName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.dataset, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    fn reason(text: &str) -> Reason {
        text.parse::<SnapshotName>().unwrap_err().reason().clone()
    }

    #[test]
    fn invalid_snapshot_names_say_why() {
        let cases = [
            ("no at", "zroot/data", Reason::NotOneAt),
            ("two ats", "zroot@a@b", Reason::NotOneAt),
            ("empty name", "zroot@", Reason::Empty),
            ("bad dataset", "zroot//x@a", Reason::EmptyComponent),
            ("bad character", "zroot@a/b", Reason::InvalidCharacter('/')),
        ];

        for (name, text, expected) in cases {
            assert_eq!(reason(text), expected, "case {name}");
        }
    }

    #[test]
    fn a_pool_root_can_have_snapshots() {
        let snapshot: SnapshotName = "zroot@first".parse().unwrap();

        assert_eq!(snapshot.pool().as_str(), "zroot");
        assert_eq!(snapshot.to_string(), "zroot@first");
    }

    proptest! {
        #[test]
        fn valid_names_round_trip(
            dataset in "[abd-z][a-z0-9]{0,8}(/[a-z0-9_]{1,8}){0,3}",
            name in "[A-Za-z0-9_.:-]{1,16}",
        ) {
            let text = format!("{dataset}@{name}");

            let snapshot: SnapshotName = text.parse().unwrap();

            prop_assert_eq!(snapshot.to_string(), text);
        }

        #[test]
        fn parsing_any_text_never_panics(text in "\\PC*") {
            let _ = text.parse::<SnapshotName>();
        }
    }
}
