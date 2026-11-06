//! What `zpool status`, `zpool import` and `zdb -e -C` say of pools: the
//! tree of their vdevs, their state and the host that used them last.
//!
//! Vdev names, device paths, host names and the messages of `zpool` come
//! from the pool's labels or the devices, which whoever made them chose:
//! they are [`Untrusted`].

use zbm_types::{HostId, PoolName, Untrusted};

use super::Health;

/// What a vdev is, from its name in the text or its `vdev_type` in JSON.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VdevKind {
    /// A device or a file: a vdev without children.
    Device,
    /// `mirror-N`.
    Mirror,
    /// `raidzP-N`, of any parity.
    Raidz,
    /// `draidP:…`.
    Draid,
    /// `spare-N`: a device and the hot spare that stands in for it.
    Spare,
    /// `replacing-N`: a device and the one replacing it.
    Replacing,
    /// Anything else with children.
    Other,
}

impl VdevKind {
    /// The kind of a vdev named `name` in the text of `zpool`, whose
    /// children say whether it is a device.
    pub(crate) fn of_name(name: &str, has_children: bool) -> VdevKind {
        if !has_children {
            return VdevKind::Device;
        }
        let stem = name.split(['-', ':']).next().unwrap_or_default();
        match stem.trim_end_matches(|c: char| c.is_ascii_digit()) {
            "mirror" => VdevKind::Mirror,
            "raidz" => VdevKind::Raidz,
            "draid" => VdevKind::Draid,
            "spare" => VdevKind::Spare,
            "replacing" => VdevKind::Replacing,
            _ => VdevKind::Other,
        }
    }

    /// The kind of the `vdev_type` of `zpool status -j`.
    pub(crate) fn of_type(vdev_type: &str, has_children: bool) -> VdevKind {
        match vdev_type {
            "mirror" => VdevKind::Mirror,
            "raidz" => VdevKind::Raidz,
            "draid" => VdevKind::Draid,
            "spare" => VdevKind::Spare,
            "replacing" => VdevKind::Replacing,
            _ if !has_children => VdevKind::Device,
            _ => VdevKind::Other,
        }
    }
}

/// What a top-level vdev holds, in the order `zpool status` prints them.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum VdevClass {
    /// The pool's data.
    Data,
    /// The deduplication table (`dedup`).
    Dedup,
    /// Metadata and small blocks (`special`).
    Special,
    /// The intent log (`logs`).
    Log,
    /// The level 2 read cache (`cache`).
    Cache,
    /// Hot spares (`spares`).
    Spare,
}

impl VdevClass {
    /// The class of a section heading of the text, such as `logs`.
    pub(crate) fn of_heading(heading: &str) -> Option<VdevClass> {
        match heading {
            "dedup" => Some(VdevClass::Dedup),
            "special" => Some(VdevClass::Special),
            "logs" => Some(VdevClass::Log),
            "cache" => Some(VdevClass::Cache),
            "spares" => Some(VdevClass::Spare),
            _ => None,
        }
    }
}

/// The error counts of a vdev: the `READ`, `WRITE` and `CKSUM` columns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VdevErrors {
    /// Read errors.
    pub read: u64,
    /// Write errors.
    pub write: u64,
    /// Checksum errors.
    pub cksum: u64,
}

/// A vdev and the vdevs below it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vdev {
    /// Its name: a device path with `-P`, a bare device name in the
    /// listing of `zpool import`, `mirror-0`, or the GUID of a device that
    /// is missing.
    pub name: Untrusted<String>,
    /// What it is.
    pub kind: VdevKind,
    /// What its top-level vdev holds.
    pub class: VdevClass,
    /// Its state, in the words of the text of `zpool status`; `None` where
    /// `zpool` prints none, as for the cache devices and spares of a pool
    /// not imported. A spare that can be used is `AVAIL`, one in use
    /// `INUSE`.
    pub state: Option<Health>,
    /// Its error counts; `None` where `zpool` prints none, as for a pool
    /// not imported.
    pub errors: Option<VdevErrors>,
    /// What `zpool` says after the state: why the device cannot be used,
    /// such as `cannot open`, or the path a missing device had, as `was
    /// /dev/sdb1`. From the text, the notes after it too, such as
    /// `(resilvering)`, which the JSON of `zpool status -j` does not give.
    pub note: Option<Untrusted<String>>,
    /// The vdevs below it, in the order printed.
    pub children: Vec<Vdev>,
}

/// An imported pool as `zpool status -P` describes it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolStatus {
    /// Its name.
    pub name: PoolName,
    /// Its state.
    pub state: Health,
    /// What is wrong with it (`status:`), on one line.
    pub status: Option<Untrusted<String>>,
    /// What to do about it (`action:`), on one line.
    pub action: Option<Untrusted<String>>,
    /// The number of data errors (`errors:` or `error_count`); `None` when
    /// `zpool` does not say.
    pub errors: Option<u64>,
    /// Its top-level vdevs, those of each class after the data's.
    pub vdevs: Vec<Vdev>,
}

/// A pool that `zpool import` lists as one that could be imported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Importable {
    /// Its name, which another pool may share.
    pub name: PoolName,
    /// Its numeric identifier, which tells it from a pool of the same
    /// name.
    pub id: u64,
    /// Its state, if it were imported.
    pub state: Health,
    /// Whether another system used it last or has it imported, so that
    /// importing it needs `-f`, or is refused.
    pub foreign: bool,
    /// What is wrong with it (`status:`), on one line.
    pub status: Option<Untrusted<String>>,
    /// What can be done (`action:`), on one line.
    pub action: Option<Untrusted<String>>,
    /// Its top-level vdevs, without error counts.
    pub vdevs: Vec<Vdev>,
}

/// The host a pool not imported was last imported by, from its
/// configuration (`zdb -e -C`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolHost {
    /// Its host ID; `None` when the pool records none.
    pub hostid: Option<HostId>,
    /// Its host name.
    pub hostname: Option<Untrusted<String>>,
}
