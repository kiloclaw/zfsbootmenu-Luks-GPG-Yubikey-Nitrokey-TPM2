//! What the menu reads of a pool besides its name and health (ADR 0016,
//! point 5): the tree of its devices, and the host that used it last when
//! it is another system.
//!
//! Device names, notes and host names come from the pools' labels or the
//! devices, which whoever made them chose: they are [`Untrusted`].

use zbm_types::{HostId, Untrusted};

/// The host a pool not imported was last imported by, from its
/// configuration (`zdb -e -C`).
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PoolHost {
    /// Its host ID, which an import as that host adopts; `None` when
    /// unknown.
    pub hostid: Option<HostId>,
    /// Its host name, as the host gave it; `None` when unknown.
    pub hostname: Option<Untrusted<String>>,
}

/// The state of a device of a pool, as `zpool status` names it: the
/// states the pool's own [`Health`](super::Health) folds together, kept
/// apart.
#[derive(
    Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum DeviceState {
    /// It works.
    Online,
    /// A group that lost redundancy, its data there.
    Degraded,
    /// Taken offline by an administrator.
    Offline,
    /// Removed while the system ran.
    Removed,
    /// It cannot be opened.
    Unavailable,
    /// Too many errors, so ZFS stopped using it.
    Faulted,
}

impl DeviceState {
    /// The word `zpool status` uses.
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceState::Online => "ONLINE",
            DeviceState::Degraded => "DEGRADED",
            DeviceState::Offline => "OFFLINE",
            DeviceState::Removed => "REMOVED",
            DeviceState::Unavailable => "UNAVAIL",
            DeviceState::Faulted => "FAULTED",
        }
    }

    /// Whether the pool has lost the device: faulted, unavailable or
    /// removed. One taken offline was meant to be.
    pub fn is_missing(self) -> bool {
        matches!(
            self,
            DeviceState::Faulted | DeviceState::Unavailable | DeviceState::Removed
        )
    }
}

/// The error counts of a device: the `READ`, `WRITE` and `CKSUM` columns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceErrors {
    /// Read errors.
    pub read: u64,
    /// Write errors.
    pub write: u64,
    /// Checksum errors.
    pub cksum: u64,
}

/// What a top-level vdev holds, and so the devices below it: the pool's
/// data, or one of the classes `zpool status` prints under a heading of
/// its own, in the order it prints them.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum DeviceClass {
    /// The pool's data.
    #[default]
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

/// A device of a pool, or a group of them such as `mirror-0`, and the
/// devices below it.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Device {
    /// Its name: a device path, a bare device name for a pool not
    /// imported, `mirror-0`, or the GUID of a device that is missing.
    pub name: Untrusted<String>,
    /// What its top-level vdev holds. A spare in use shows twice, as
    /// `zpool status` prints it: below the vdev it stands in for, and among
    /// the spares.
    #[serde(default)]
    pub class: DeviceClass,
    /// Its state; `None` where `zpool` prints none or one the menu does
    /// not tell apart, as for cache devices and spares.
    pub state: Option<DeviceState>,
    /// Its error counts; `None` for a pool not imported.
    pub errors: Option<DeviceErrors>,
    /// What `zpool` says after the state, such as `cannot open` or `was
    /// /dev/sdb1`.
    pub note: Option<Untrusted<String>>,
    /// The devices below it.
    pub children: Vec<Device>,
}

impl Device {
    /// Its path, or the path it had: ZFS names a device it lost by its
    /// GUID, and its note says `was /dev/sdb1`. Untrusted, as its name is.
    pub fn path(&self) -> &str {
        let was = self
            .note
            .as_ref()
            .and_then(|note| note.get().strip_prefix("was "));
        was.unwrap_or(self.name.get())
    }

    /// Adds this device, when nothing is below it, or the devices below
    /// it that nothing is below, to `found`.
    fn collect_disks<'d>(&'d self, found: &mut Vec<&'d Device>) {
        if self.children.is_empty() {
            found.push(self);
        }
        for child in &self.children {
            child.collect_disks(found);
        }
    }

    /// Adds this device, when it is missing, then the missing devices
    /// below it, to `found`.
    pub(super) fn collect_missing<'d>(&'d self, found: &mut Vec<&'d Device>) {
        if self.state.is_some_and(DeviceState::is_missing) {
            found.push(self);
        }
        for child in &self.children {
            child.collect_missing(found);
        }
    }
}

impl super::Pool {
    /// Its disks: the devices nothing is below, such as partitions and
    /// files, without the groups such as `mirror-0` that hold them, in the
    /// order `zpool status` prints them.
    pub fn disks(&self) -> Vec<&Device> {
        let mut found = Vec::new();
        for device in &self.devices {
            device.collect_disks(&mut found);
        }
        found
    }

    /// Whether `device` is one of its disks, by its name or by the path it
    /// had ([`Device::path`]).
    pub(crate) fn has_disk(&self, device: &str) -> bool {
        self.disks()
            .iter()
            .any(|disk| disk.name.get() == device || disk.path() == device)
    }
}

/// The kernel's messages about a device of a pool
/// ([`Action::LoadDeviceLog`](super::Action::LoadDeviceLog)): those that
/// name it or a device it stands on, the disk of a partition and the
/// devices below a mapping, as the kernel names them, or that the kernel
/// tagged with one of them. A message that names only a port, as
/// `ata3.00:` and `usb 1-2:` do, is not among them.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DeviceLog {
    /// The device, as the pool names it.
    pub device: Untrusted<String>,
    /// The kernel's names of it and of those it stands on, `sda2` then
    /// `sda`. A device the kernel no longer lists is its name and its
    /// disk's, when the kernel names disks and partitions so (`sdb2`,
    /// `sdb`); empty for any other, as a link of `/dev/disk/by-id` gone
    /// with its device, or a file.
    pub names: Vec<Untrusted<String>>,
    /// The messages, oldest first, as `dmesg` prints them; the newest of
    /// them when there are many.
    pub lines: Vec<Untrusted<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Health, Pool};

    fn device(name: &str, state: Option<DeviceState>, children: Vec<Device>) -> Device {
        Device {
            name: Untrusted::new(name.to_owned()),
            class: DeviceClass::Data,
            state,
            errors: None,
            note: None,
            children,
        }
    }

    #[test]
    fn the_disks_are_the_devices_nothing_is_below_in_the_order_printed() {
        let mut pool = Pool::imported("tank".parse().unwrap(), Health::Online);
        let lost = Device {
            note: Some(Untrusted::new("was /dev/sdb1".to_owned())),
            ..device("1234", Some(DeviceState::Unavailable), Vec::new())
        };
        pool.devices = vec![
            device(
                "mirror-0",
                Some(DeviceState::Degraded),
                vec![
                    device("/dev/sda1", Some(DeviceState::Online), Vec::new()),
                    lost,
                ],
            ),
            device("/dev/nvme0n1", None, Vec::new()),
        ];

        let names: Vec<&str> = pool
            .disks()
            .iter()
            .map(|disk| disk.name.get().as_str())
            .collect();
        assert_eq!(names, ["/dev/sda1", "1234", "/dev/nvme0n1"]);
        assert!(pool.has_disk("/dev/sdb1"));
        assert!(pool.has_disk("/dev/nvme0n1"));
        assert!(!pool.has_disk("mirror-0"));
    }

    #[test]
    fn the_missing_devices_are_listed_worst_first_and_offline_ones_are_not() {
        let mut pool = Pool::imported("tank".parse().unwrap(), Health::Degraded);
        pool.devices = vec![
            device(
                "mirror-0",
                Some(DeviceState::Degraded),
                vec![
                    device("sda", Some(DeviceState::Removed), Vec::new()),
                    device("sdb", Some(DeviceState::Offline), Vec::new()),
                ],
            ),
            device(
                "mirror-1",
                Some(DeviceState::Faulted),
                vec![device("sdc", Some(DeviceState::Unavailable), Vec::new())],
            ),
            device("cache", None, Vec::new()),
        ];

        let names: Vec<&str> = pool
            .missing_devices()
            .iter()
            .map(|device| device.name.get().as_str())
            .collect();
        assert_eq!(names, ["mirror-1", "sdc", "sda"]);
        let lost = Device {
            note: Some(Untrusted::new("was /dev/sdb1".to_owned())),
            ..device("9876543210987654321", None, Vec::new())
        };
        assert_eq!(lost.path(), "/dev/sdb1");
        let closed = Device {
            note: Some(Untrusted::new("cannot open".to_owned())),
            ..device("/dev/sdc", None, Vec::new())
        };
        assert_eq!(closed.path(), "/dev/sdc");
        let words: Vec<&str> = [
            DeviceState::Online,
            DeviceState::Degraded,
            DeviceState::Offline,
            DeviceState::Removed,
            DeviceState::Unavailable,
            DeviceState::Faulted,
        ]
        .map(DeviceState::as_str)
        .to_vec();
        assert_eq!(
            words,
            [
                "ONLINE", "DEGRADED", "OFFLINE", "REMOVED", "UNAVAIL", "FAULTED"
            ]
        );
    }
}
