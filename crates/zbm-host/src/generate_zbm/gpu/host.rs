//! The GPUs of the build host, from sysfs: each display controller's PCI
//! address, vendor and device, and, for an AMD GPU that `amdgpu` drives, the
//! versions of its IP blocks.

use std::path::Path;

/// A display controller on the PCI bus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Device {
    /// Its PCI address, `0000:03:00.0`.
    pub address: String,
    /// Its vendor ID.
    pub vendor: u16,
    /// Its device ID.
    pub device: u16,
    /// The versions of its IP blocks, `("GC", (12, 0, 0))`, when a running
    /// `amdgpu` shows them (`ip_discovery/die/0`).
    pub blocks: Vec<(String, (u32, u32, u32))>,
}

/// The IP blocks whose firmware `amdgpu` names after their version, and the
/// start of those names.
pub const BLOCKS: &[(&str, &str)] = &[
    ("GC", "gc"),
    ("MP0", "psp"),
    ("MP1", "smu"),
    ("SDMA0", "sdma"),
    ("DMU", "dcn"),
    ("UVD", "vcn"),
    ("VPE", "vpe"),
];

/// The display controllers (PCI class 03) under `devices`, normally
/// `/sys/bus/pci/devices`, in address order.
pub fn scan(devices: &Path) -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(devices) else {
        return Vec::new();
    };
    let mut found: Vec<Device> = entries
        .flatten()
        .filter_map(|entry| read_device(&entry.path()))
        .collect();
    found.sort_by(|a, b| a.address.cmp(&b.address));
    found
}

/// The device at `dir`, if it is a display controller.
fn read_device(dir: &Path) -> Option<Device> {
    let class = read_hex(&dir.join("class"))?;
    if class >> 16 != 0x03 {
        return None;
    }
    Some(Device {
        address: dir.file_name()?.to_string_lossy().into_owned(),
        vendor: u16::try_from(read_hex(&dir.join("vendor"))?).ok()?,
        device: u16::try_from(read_hex(&dir.join("device"))?).ok()?,
        blocks: read_blocks(&dir.join("ip_discovery/die/0")),
    })
}

/// The versions of the [`BLOCKS`] under an `ip_discovery` die, from their
/// first instance.
fn read_blocks(die: &Path) -> Vec<(String, (u32, u32, u32))> {
    BLOCKS
        .iter()
        .filter_map(|(block, _)| {
            let instance = die.join(block).join("0");
            let number = |name: &str| read_number(&instance.join(name));
            Some((
                (*block).to_owned(),
                (number("major")?, number("minor")?, number("revision")?),
            ))
        })
        .collect()
}

/// A sysfs file holding a hexadecimal number, `0x030000`.
fn read_hex(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    let digits = text.trim().strip_prefix("0x")?;
    u32::from_str_radix(digits, 16).ok()
}

/// A sysfs file holding a decimal number.
fn read_number(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}
