//! The block devices a device stands on, as the kernel lists them in
//! `/sys/class/block`: a partition stands on its disk, a device-mapper
//! mapping such as an open LUKS volume on its slaves, and those on theirs.
//! The menu reads the kernel's messages about all of them.
//!
//! A name comes from a pool's label or from `/sys`, never trusted as a
//! path: one that is empty, `.` or `..`, or holds a `/`, is no name of
//! `/sys/class/block`, and is refused. A device the kernel no longer lists
//! is known only by a name the kernel gives disks and partitions, as
//! `sdb2`: any other word could be any line's of the kernel's log.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{Devices, SYS_BLOCK};

/// The most devices a stack holds, against a loop in `/sys`.
const MOST: usize = 32;

/// The directories `zpool import` looks for devices in, in its order, and
/// takes off the names it prints: `virtio-d0` is in `/dev/disk/by-id`.
const SEARCH: [&str; 9] = [
    "/dev/disk/by-vdev",
    "/dev/mapper",
    "/dev/disk/by-partlabel",
    "/dev/disk/by-partuuid",
    "/dev/disk/by-label",
    "/dev/disk/by-uuid",
    "/dev/disk/by-id",
    "/dev/disk/by-path",
    "/dev",
];

/// A block device as the kernel lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockDevice {
    /// Its name in `/sys/class/block`: `sda1`, `nvme0n1`, `dm-0`.
    pub name: String,
    /// Its numbers, `8:1`, as its `dev` file says; `None` when unknown, as
    /// for a device the kernel no longer lists.
    pub numbers: Option<String>,
}

/// `name` when it may name an entry of `/sys/class/block`: not empty, `.`
/// or `..`, and without a `/` or a NUL.
fn entry(name: &str) -> Option<&str> {
    let is_entry = !matches!(name, "" | "." | "..") && !name.contains(['/', '\0']);
    is_entry.then_some(name)
}

/// Whether `text` is a number: digits only, at least one.
fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether `name` is a disk the kernel numbers, whose partitions are named
/// by a `p` and a number: `nvme0n1`, `mmcblk0`, `loop0`, `md0`, `nbd0`,
/// `zd0`, and the mapping `dm-0`.
fn is_numbered_disk(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("nvme") {
        return rest
            .split_once('n')
            .is_some_and(|(controller, space)| is_number(controller) && is_number(space));
    }
    ["mmcblk", "loop", "md", "nbd", "zd", "dm-"]
        .iter()
        .any(|prefix| name.strip_prefix(prefix).is_some_and(is_number))
}

/// The disk of `name` when it is a name the kernel gives a disk or a
/// partition: itself for a disk, `sdb` for `sdb2` and `nvme0n1` for
/// `nvme0n1p2`; `None` for any other name.
fn disk_of(name: &str) -> Option<&str> {
    // Disks named by letters, their partitions by a number after them.
    for prefix in ["sd", "vd", "xvd", "hd"] {
        let Some(rest) = name.strip_prefix(prefix) else {
            continue;
        };
        let letters = rest.trim_end_matches(|c: char| c.is_ascii_digit());
        if !letters.is_empty() && letters.bytes().all(|byte| byte.is_ascii_lowercase()) {
            return name.get(..prefix.len() + letters.len());
        }
    }
    let disk = match name.rsplit_once('p') {
        Some((disk, partition)) if is_number(partition) && is_numbered_disk(disk) => disk,
        _ => name,
    };
    is_numbered_disk(disk).then_some(disk)
}

/// The kernel's name of `device`, a path of `/dev` or a name in it, that
/// the kernel no longer lists, and that of its disk, the same for a disk;
/// `None` when it has no name the kernel gives disks and partitions.
fn lost(device: &str) -> Option<(&str, &str)> {
    let name = entry(device.strip_prefix("/dev/").unwrap_or(device))?;
    Some((name, disk_of(name)?))
}

impl Devices<'_> {
    /// The block devices `device` is and stands on, itself first, then the
    /// disk of a partition and the slaves of a mapping, and theirs. A pool
    /// names its devices by path, as `/dev/disk/by-id/...`, which leads to
    /// the kernel's node, or, not imported, by their names in the
    /// directories `zpool import` searches. A device the kernel no longer
    /// lists, as one a pool lost, is its name then its disk's, when the
    /// kernel would name them so (`sdb2` on `sdb`); empty for any other.
    pub fn stack(&self, device: &str) -> Vec<BlockDevice> {
        if let Some(first) = self.kernel_name(device) {
            return self.walk(first);
        }
        let Some((name, disk)) = lost(device) else {
            return Vec::new();
        };
        let mut stack = vec![BlockDevice {
            name: name.to_owned(),
            numbers: None,
        }];
        if disk != name {
            match self.numbers(disk) {
                Some(_) => stack.extend(self.walk(disk.to_owned())),
                None => stack.push(BlockDevice {
                    name: disk.to_owned(),
                    numbers: None,
                }),
            }
        }
        stack
    }

    /// The block device `first` and those it stands on.
    fn walk(&self, first: String) -> Vec<BlockDevice> {
        let mut seen = BTreeSet::new();
        let mut stack = Vec::new();
        let mut next = vec![first];
        while let Some(name) = next.pop() {
            if stack.len() >= MOST || !seen.insert(name.clone()) {
                continue;
            }
            let below = self.below(&name);
            let numbers = self.numbers(&name);
            stack.push(BlockDevice { name, numbers });
            next.extend(below.into_iter().rev());
        }
        stack
    }

    /// The name in `/sys/class/block` of the node of `/dev` that `device`
    /// leads to: a path, or a name in one of the directories `zpool
    /// import` searches, the first that leads to one. A path that leads
    /// elsewhere, as a file a pool is made of, leads to none.
    fn kernel_name(&self, device: &str) -> Option<String> {
        let paths: Vec<PathBuf> = if device.starts_with('/') {
            vec![PathBuf::from(device)]
        } else {
            let name = entry(device)?;
            SEARCH.iter().map(|dir| Path::new(dir).join(name)).collect()
        };
        paths.iter().find_map(|path| {
            let real = self.files.canonicalize(path).ok()?;
            if real.parent() != Some(Path::new("/dev")) {
                return None;
            }
            let name = entry(real.file_name()?.to_str()?)?;
            self.numbers(name).map(|_| name.to_owned())
        })
    }

    /// The numbers of the block device `name`, as its `dev` file says.
    fn numbers(&self, name: &str) -> Option<String> {
        let dev = Path::new(SYS_BLOCK).join(entry(name)?).join("dev");
        let numbers = self.files.read_to_string(&dev).ok()?;
        Some(numbers.trim().to_owned())
    }

    /// What the block device `name` stands on: the disk of a partition,
    /// whose directory holds the partition's, and the slaves of a mapping.
    fn below(&self, name: &str) -> Vec<String> {
        let dir = Path::new(SYS_BLOCK).join(name);
        let mut below = Vec::new();
        if self.files.exists(&dir.join("partition")) {
            below.extend(
                self.block_devices()
                    .into_iter()
                    .filter(|disk| disk != name)
                    .find(|disk| {
                        self.files
                            .exists(&Path::new(SYS_BLOCK).join(disk).join(name))
                    }),
            );
        }
        let slaves = self.files.read_dir(&dir.join("slaves")).unwrap_or_default();
        below.extend(
            slaves
                .iter()
                .filter_map(|slave| entry(slave.to_str()?))
                .filter(|slave| self.numbers(slave).is_some())
                .map(str::to_owned),
        );
        below
    }
}

#[cfg(test)]
mod tests;
