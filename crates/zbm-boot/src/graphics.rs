//! The GPUs of the image's graphics drivers (ROADMAP.md #17): what the PCI
//! bus and the DRM devices say of them, for start-up's choice of the GPUs
//! the drivers take (`zbm_startup::graphics`) and for the hand-back before
//! `kexec`. An image with graphics drivers (`Kernel.GraphicsDrivers` of
//! `generate-zbm`) lists them in [`DRIVERS`]. Before `kexec` the GPUs are
//! handed back ([`hand_back`]), and taken again if it fails ([`take_back`]).

use std::path::Path;
use std::time::Duration;

use zbm_platform::{Context, Files};

/// The graphics drivers of the image, separated by spaces, below the root.
pub const DRIVERS: &str = "etc/zfsbootmenu/graphics-drivers";

/// The PCI devices, below the root.
pub const PCI: &str = "sys/bus/pci/devices";

/// The DRM devices, below the root.
const DRM: &str = "sys/class/drm";

/// How long a driver may take to bring up a chosen GPU.
const WAIT: Duration = Duration::from_secs(10);

/// How often the wait looks.
const POLL: Duration = Duration::from_millis(100);

/// The PCI vendor ID of AMD (`amdgpu`).
pub const AMD: u16 = 0x1002;
/// The PCI vendor ID of Intel (`i915`, `xe`).
pub const INTEL: u16 = 0x8086;
/// The PCI vendor ID of QEMU's virtio GPU (`virtio_gpu`).
pub const VIRTIO: u16 = 0x1af4;
/// The PCI vendor ID of QEMU's standard VGA (`bochs`).
pub const QEMU_VGA: u16 = 0x1234;

/// A display controller on the PCI bus.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gpu {
    /// Its PCI address.
    pub address: String,
    /// Its vendor ID.
    pub vendor: u16,
    /// Its device ID.
    pub device: u16,
}

impl Gpu {
    /// The GPU for the log: its address, vendor and device.
    pub fn describe(&self) -> String {
        format!("{} ({:04x}:{:04x})", self.address, self.vendor, self.device)
    }
}

/// The graphics drivers of the image below `root`; none when it has none.
pub fn image_drivers(files: &dyn Files, root: &Path) -> Vec<String> {
    files
        .read_to_string(&root.join(DRIVERS))
        .map(|text| text.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// A sysfs file holding a hexadecimal number, `0x030000`.
fn read_hex(files: &dyn Files, path: &Path) -> Option<u32> {
    let text = files.read_to_string(path).ok()?;
    u32::from_str_radix(text.trim().strip_prefix("0x")?, 16).ok()
}

/// The display controllers (PCI class 03) below `root`, by address.
pub fn display_controllers(files: &dyn Files, root: &Path) -> Vec<Gpu> {
    let devices = root.join(PCI);
    let mut names: Vec<String> = files
        .read_dir(&devices)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .collect();
    names.sort();
    names
        .into_iter()
        .filter_map(|address| {
            let dir = devices.join(&address);
            if read_hex(files, &dir.join("class"))? >> 16 != 0x03 {
                return None;
            }
            Some(Gpu {
                vendor: u16::try_from(read_hex(files, &dir.join("vendor"))?).ok()?,
                device: u16::try_from(read_hex(files, &dir.join("device"))?).ok()?,
                address,
            })
        })
        .collect()
}

/// The drivers among `drivers` for the GPUs of `vendor`.
pub fn drivers_of(drivers: &[String], vendor: u16) -> Vec<&str> {
    drivers
        .iter()
        .map(String::as_str)
        .filter(|driver| match *driver {
            "amdgpu" => vendor == AMD,
            "i915" | "xe" => vendor == INTEL,
            "virtio_gpu" => vendor == VIRTIO,
            "bochs" => vendor == QEMU_VGA,
            _ => false,
        })
        .collect()
}

/// The DRM card of the GPU at `address`, `cardN`, once a driver has made it:
/// below the PCI device, or below its virtio device (`virtio0/drm`).
pub fn card_name(files: &dyn Files, root: &Path, address: &str) -> Option<String> {
    let device = root.join(PCI).join(address);
    let mut places = vec![device.join("drm")];
    for child in files.read_dir(&device).unwrap_or_default() {
        if child.to_string_lossy().starts_with("virtio") {
            places.push(device.join(child).join("drm"));
        }
    }
    let mut cards: Vec<String> = places
        .iter()
        .find_map(|place| files.read_dir(place).ok())?
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| {
            name.strip_prefix("card")
                .is_some_and(|number| number.parse::<u32>().is_ok())
        })
        .collect();
    cards.sort();
    cards.into_iter().next()
}

/// Whether a display is connected to a connector of `card`
/// (`/sys/class/drm/<card>-<connector>/status`).
pub fn is_connected(files: &dyn Files, root: &Path, card: &str) -> bool {
    let drm = root.join(DRM);
    let prefix = format!("{card}-");
    files
        .read_dir(&drm)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with(&prefix))
        .any(|name| {
            files
                .read_to_string(&drm.join(name).join("status"))
                .is_ok_and(|status| status.trim() == "connected")
        })
}

/// Waits until each GPU of `chosen` has a DRM card, at most ten seconds.
pub fn wait_for_cards(context: &Context<'_>, root: &Path, chosen: &[Gpu]) {
    let deadline = context.clock().now() + WAIT;
    loop {
        let missing: Vec<&Gpu> = chosen
            .iter()
            .filter(|gpu| card_name(context.files(), root, &gpu.address).is_none())
            .collect();
        if missing.is_empty() {
            break;
        }
        if context.clock().now() >= deadline {
            for gpu in missing {
                zbm_trace::warn!(
                    "graphics: no display device for {gpu} after {wait} s";
                    gpu = %gpu.describe(), wait = %WAIT.as_secs()
                );
            }
            break;
        }
        context.clock().sleep(POLL);
    }
    for gpu in chosen {
        if let Some(card) = card_name(context.files(), root, &gpu.address) {
            let connected = if is_connected(context.files(), root, &card) {
                "a display connected"
            } else {
                "no display connected"
            };
            zbm_trace::info!(
                "graphics: {gpu} is {card}, {connected}";
                gpu = %gpu.describe(), %card, %connected
            );
        }
    }
}

mod hand_back;
pub use hand_back::{hand_back, take_back};

#[cfg(test)]
mod tests;
