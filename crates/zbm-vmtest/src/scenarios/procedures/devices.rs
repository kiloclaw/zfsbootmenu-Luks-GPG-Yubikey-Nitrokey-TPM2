//! The disks of the procedures, and the placeholder names of the
//! documentation that point to them.

use crate::disks::{disk, partition};
use crate::error::Result;
use crate::qemu::Disk;
use crate::settings::Settings;

/// A partition: its name and its size, or `None` for the rest of the disk.
pub(super) type Part = (&'static str, Option<&'static str>);

/// A disk with an EFI system partition and a LUKS partition.
const PARTED: &[Part] = &[("esp", Some("64M")), ("zfs", None)];

/// Example 12 on SSDs that keep 512-byte sectors: the LUKS partition has a
/// size in MiB, so that 4 KiB encryption sectors fit (see the example).
const SSD: &[Part] = &[("esp", Some("64M")), ("zfs", Some("300M"))];

/// Auxiliary partitions hold a 16 MiB LUKS2 header and at least the 64 MiB
/// ZFS needs for a vdev.
const FAST: &[Part] = &[
    ("esp", Some("32M")),
    ("special", Some("128M")),
    ("log", Some("128M")),
    ("cache", Some("128M")),
    ("dedup", None),
];

/// A disk of the procedures.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Device {
    /// The placeholder name in the documentation, such as `ata-DISK1`.
    pub name: String,
    /// The serial of the virtual disk.
    pub serial: String,
    /// Its partitions; `None`: the whole disk.
    pub parts: Option<&'static [Part]>,
}

impl Device {
    /// A disk named `name` with the serial `serial`.
    fn new(name: String, serial: String, parts: Option<&'static [Part]>) -> Device {
        Device {
            name,
            serial,
            parts,
        }
    }

    /// The size of its image.
    pub fn size(&self) -> &'static str {
        if self.parts == Some(FAST) {
            "768M"
        } else {
            "384M"
        }
    }
}

/// Every disk, in the order of the machine.
pub(super) fn table() -> Vec<Device> {
    let mut table = Vec::new();
    for index in 1..8 {
        let (name, serial) = (format!("ata-DISK{index}"), format!("w{index}"));
        table.push(Device::new(name, serial, Some(PARTED)));
    }
    for index in 1..8 {
        let (name, serial) = (format!("ata-DISK{index}"), format!("x{index}"));
        table.push(Device::new(name, serial, None));
    }
    for index in 1..4 {
        let (name, serial) = (format!("nvme-FAST{index}"), format!("f{index}"));
        table.push(Device::new(name, serial, Some(FAST)));
    }
    table.push(Device::new("ata-NEWDISK".into(), "n1".into(), Some(PARTED)));
    for index in 1..3 {
        let (name, serial) = (format!("nvme-SSD{index}"), format!("s{index}"));
        table.push(Device::new(name, serial, Some(SSD)));
    }
    table
}

/// The `ln` commands that map the placeholder names to the virtual disks.
pub(super) fn links(table: &[Device]) -> Vec<String> {
    let mut commands = Vec::new();
    for Device {
        name,
        serial,
        parts,
    } in table
    {
        match parts {
            None => commands.push(format!(
                "ln -sf /dev/disk/by-id/virtio-{serial} /dev/disk/by-id/{name}"
            )),
            Some(parts) => {
                for number in 1..=parts.len() {
                    commands.push(format!(
                        "ln -sf /dev/disk/by-id/virtio-{serial}-part{number} \
                         /dev/disk/by-id/{name}-part{number}"
                    ));
                }
            }
        }
    }
    commands
}

/// Creates the disks anew and partitions them; returns them as the
/// machine's disks, without the payload disk.
///
/// # Errors
///
/// When an image cannot be created or partitioned.
pub(super) fn create(settings: &Settings) -> Result<Vec<Disk>> {
    let mut disks = Vec::new();
    for device in table() {
        let path = disk(settings, "procedures", &device.serial, device.size(), true)?;
        if let Some(parts) = device.parts {
            partition(&path, parts)?;
        }
        disks.push(Disk::new(path, device.serial));
    }
    Ok(disks)
}
