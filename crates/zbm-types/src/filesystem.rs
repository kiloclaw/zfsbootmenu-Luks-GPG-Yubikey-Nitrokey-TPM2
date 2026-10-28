//! What ZFSBootMenu mounts to look for kernels: a dataset, or a snapshot of
//! one.

use std::fmt;
use std::str::FromStr;

use crate::error::ParseError;
use crate::{DatasetName, PoolName, SnapshotName};

/// A ZFS filesystem ZFSBootMenu can mount: a dataset such as
/// `zroot/ROOT/void`, or a snapshot of one such as
/// `zroot/ROOT/void@before-upgrade`, which mounts read-only.
///
/// A name with `@` parses as a [`SnapshotName`], any other as a
/// [`DatasetName`].
///
/// ```
/// use zbm_types::FilesystemName;
///
/// let snapshot: FilesystemName = "zroot/ROOT/void@before-upgrade".parse()?;
/// assert_eq!(snapshot.dataset().as_str(), "zroot/ROOT/void");
/// assert!(snapshot.is_snapshot());
/// assert!("zroot/ROOT/void@".parse::<FilesystemName>().is_err());
/// # Ok::<(), zbm_types::ParseError>(())
/// ```
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FilesystemName {
    /// A filesystem.
    Dataset(DatasetName),
    /// A snapshot of a filesystem.
    Snapshot(SnapshotName),
}

impl FilesystemName {
    /// The dataset: the filesystem itself, or the one the snapshot is of.
    pub fn dataset(&self) -> &DatasetName {
        match self {
            FilesystemName::Dataset(dataset) => dataset,
            FilesystemName::Snapshot(snapshot) => snapshot.dataset(),
        }
    }

    /// The pool the filesystem is in.
    pub fn pool(&self) -> PoolName {
        self.dataset().pool()
    }

    /// Whether it names a snapshot.
    pub fn is_snapshot(&self) -> bool {
        matches!(self, FilesystemName::Snapshot(_))
    }
}

impl From<DatasetName> for FilesystemName {
    fn from(dataset: DatasetName) -> FilesystemName {
        FilesystemName::Dataset(dataset)
    }
}

impl From<SnapshotName> for FilesystemName {
    fn from(snapshot: SnapshotName) -> FilesystemName {
        FilesystemName::Snapshot(snapshot)
    }
}

impl FromStr for FilesystemName {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<FilesystemName, ParseError> {
        if text.contains('@') {
            text.parse().map(FilesystemName::Snapshot)
        } else {
            text.parse().map(FilesystemName::Dataset)
        }
    }
}

impl fmt::Display for FilesystemName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesystemName::Dataset(dataset) => dataset.fmt(formatter),
            FilesystemName::Snapshot(snapshot) => snapshot.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;

    #[test]
    fn datasets_and_snapshots_parse_as_themselves() {
        let dataset: FilesystemName = "tank/ROOT/void".parse().unwrap();
        assert_eq!(
            dataset,
            FilesystemName::Dataset("tank/ROOT/void".parse().unwrap())
        );
        let snapshot: FilesystemName = "tank/ROOT/void@a".parse().unwrap();
        assert_eq!(
            snapshot,
            FilesystemName::Snapshot("tank/ROOT/void@a".parse().unwrap())
        );
        assert_eq!(snapshot.dataset(), dataset.dataset());
        assert_eq!(snapshot.pool().as_str(), "tank");
        assert!(!dataset.is_snapshot());
    }

    #[test]
    fn invalid_names_are_refused() {
        for text in ["", "@a", "tank@", "tank@a@b", "tank/../x"] {
            assert!(text.parse::<FilesystemName>().is_err(), "{text}");
        }
    }

    proptest! {
        #[test]
        fn names_round_trip(text in "t[a-z0-9]{0,6}(/[a-z0-9]{1,6}){0,3}(@[a-z0-9]{1,6})?") {
            let name: FilesystemName = text.parse().unwrap();
            prop_assert_eq!(name.to_string(), text);
        }
    }
}
