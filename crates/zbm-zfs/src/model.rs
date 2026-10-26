//! What `zfs` and `zpool` report, as types: pools, datasets, snapshots and
//! their properties, whichever output format they came from.

use std::collections::BTreeMap;
use std::fmt;

use zbm_types::{DatasetName, PoolName, SnapshotName};

/// Where a property's value comes from (the `SOURCE` column of `zfs get`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Source {
    /// Set on this dataset or pool.
    Local,
    /// The built-in default.
    Default,
    /// Inherited from the named ancestor.
    Inherited(String),
    /// Received with `zfs receive`.
    Received,
    /// Set for this mount only.
    Temporary,
    /// A property that has no source, such as a read-only statistic.
    #[default]
    None,
}

impl Source {
    /// The source as `zfs get -H -o source` prints it: `local`, `default`,
    /// `inherited from <name>`, `received`, `temporary` or `-`.
    pub fn parse(text: &str) -> Source {
        match text {
            "local" => Source::Local,
            "default" => Source::Default,
            "received" => Source::Received,
            "temporary" => Source::Temporary,
            other => other
                .strip_prefix("inherited from ")
                .map_or(Source::None, |from| Source::Inherited(from.to_owned())),
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Local => formatter.write_str("local"),
            Source::Default => formatter.write_str("default"),
            Source::Inherited(from) => write!(formatter, "inherited from {from}"),
            Source::Received => formatter.write_str("received"),
            Source::Temporary => formatter.write_str("temporary"),
            Source::None => formatter.write_str("-"),
        }
    }
}

/// A property's value and source.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Property {
    /// The value as `zfs get -p` prints it; `-` for an unset user property.
    pub value: String,
    /// Where the value comes from.
    pub source: Source,
}

impl Property {
    /// A property with `value` from `source`.
    pub fn new(value: impl Into<String>, source: Source) -> Property {
        Property {
            value: value.into(),
            source,
        }
    }

    /// The value, or `None` for `-`, which ZFS prints for an unset user
    /// property or a property that does not apply.
    pub fn value(&self) -> Option<&str> {
        (self.value != "-").then_some(self.value.as_str())
    }
}

/// The health of a pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Health {
    /// `ONLINE`.
    Online,
    /// `DEGRADED`: working, with redundancy lost.
    Degraded,
    /// `FAULTED`.
    Faulted,
    /// `OFFLINE`.
    Offline,
    /// `UNAVAIL`.
    Unavailable,
    /// `REMOVED`.
    Removed,
    /// `SUSPENDED`: I/O is suspended.
    Suspended,
    /// Anything else, as printed.
    Other(String),
}

impl Health {
    /// The health as `zpool list -o health` prints it.
    pub fn parse(text: &str) -> Health {
        match text {
            "ONLINE" => Health::Online,
            "DEGRADED" => Health::Degraded,
            "FAULTED" => Health::Faulted,
            "OFFLINE" => Health::Offline,
            "UNAVAIL" => Health::Unavailable,
            "REMOVED" => Health::Removed,
            "SUSPENDED" => Health::Suspended,
            other => Health::Other(other.to_owned()),
        }
    }
}

impl fmt::Display for Health {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Health::Online => "ONLINE",
            Health::Degraded => "DEGRADED",
            Health::Faulted => "FAULTED",
            Health::Offline => "OFFLINE",
            Health::Unavailable => "UNAVAIL",
            Health::Removed => "REMOVED",
            Health::Suspended => "SUSPENDED",
            Health::Other(text) => text,
        })
    }
}

/// An imported pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    /// Its name.
    pub name: PoolName,
    /// Its health.
    pub health: Health,
    /// Whether it is imported read-only.
    pub read_only: bool,
    /// The default boot environment (`bootfs`), if set.
    pub bootfs: Option<DatasetName>,
    /// Its unique identifier.
    pub guid: u64,
}

/// A dataset with the properties that were asked for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dataset {
    /// Its name.
    pub name: DatasetName,
    /// The properties asked for, by name; `name` itself is not among them.
    pub properties: BTreeMap<String, Property>,
}

impl Dataset {
    /// The value of `property`, or `None` when it was not asked for or is `-`.
    pub fn value(&self, property: &str) -> Option<&str> {
        self.properties.get(property).and_then(Property::value)
    }
}

/// A snapshot and when it was taken.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    /// Its name.
    pub name: SnapshotName,
    /// When it was taken, in seconds since 1970.
    pub creation: u64,
}

/// What happened to a path between a snapshot and what `zfs diff` compares
/// it with: the first column of `zfs diff -H`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffChange {
    /// `-`: removed since the snapshot.
    Removed,
    /// `+`: created since.
    Created,
    /// `M`: modified since.
    Modified,
    /// `R`: renamed since.
    Renamed,
}

impl DiffChange {
    /// The change of the mark `zfs diff` prints.
    pub fn parse(mark: &str) -> Option<DiffChange> {
        match mark {
            "-" => Some(DiffChange::Removed),
            "+" => Some(DiffChange::Created),
            "M" => Some(DiffChange::Modified),
            "R" => Some(DiffChange::Renamed),
            _ => None,
        }
    }
}

/// A path that `zfs diff -F -H` reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    /// What happened to it.
    pub change: DiffChange,
    /// The kind of file, as `zfs diff -F` marks it: `F` a file, `/` a
    /// directory, `@` a symbolic link, `B` and `C` devices, `|` a pipe, `=` a
    /// socket.
    pub file_type: char,
    /// The path, with the escapes of `zfs diff` decoded: absolute, under
    /// the directory where the filesystem is mounted.
    pub path: String,
    /// Where a renamed path went.
    pub renamed_to: Option<String>,
}

/// The version of ZFS: the tools and the kernel module.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Version {
    /// The tools, such as `zfs-2.4.4-1`.
    pub userland: String,
    /// The kernel module, such as `zfs-kmod-2.4.4-1`; empty when it is not
    /// loaded.
    pub kernel: String,
}

impl Version {
    /// The release of the tools as (major, minor), such as (2, 4).
    pub fn release(&self) -> Option<(u32, u32)> {
        let numbers = self.userland.strip_prefix("zfs-")?;
        let mut parts = numbers.split(['.', '-']);
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some((major, minor))
    }

    /// Whether the tools print JSON (`-j`), which OpenZFS 2.3 added.
    pub fn has_json(&self) -> bool {
        self.release().is_some_and(|release| release >= (2, 3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_read_and_print_as_zfs_get_does() {
        for text in [
            "local",
            "default",
            "inherited from zroot/ROOT",
            "received",
            "temporary",
            "-",
        ] {
            assert_eq!(Source::parse(text).to_string(), text);
        }
        assert_eq!(Source::parse("anything else"), Source::None);
    }

    #[test]
    fn health_reads_and_prints_every_state() {
        for text in [
            "ONLINE",
            "DEGRADED",
            "FAULTED",
            "OFFLINE",
            "UNAVAIL",
            "REMOVED",
            "SUSPENDED",
            "NEW",
        ] {
            assert_eq!(Health::parse(text).to_string(), text);
        }
        assert_eq!(Health::parse("NEW"), Health::Other("NEW".to_owned()));
    }

    #[test]
    fn versions_name_their_release() {
        let version = |userland: &str| Version {
            userland: userland.to_owned(),
            kernel: String::new(),
        };
        assert_eq!(version("zfs-2.2.7-1").release(), Some((2, 2)));
        assert!(!version("zfs-2.2.7-1").has_json());
        assert!(version("zfs-2.3.0").has_json());
        assert_eq!(version("zfs-x.1").release(), None);
        assert_eq!(version("openzfs").release(), None);
        let property = Property::new("-", Source::None);
        assert_eq!(property.value(), None);
    }
}
