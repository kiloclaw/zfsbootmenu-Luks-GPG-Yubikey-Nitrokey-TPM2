//! The graphics drivers of the image (ROADMAP.md #17), loaded before
//! anything is asked, so that the display does not change mode under a
//! passphrase prompt.
//!
//! An image with graphics drivers (`Kernel.GraphicsDrivers` of
//! `generate-zbm`) lists them in [`DRIVERS`] and keeps udev from loading
//! them (`modprobe.d`). `zfsbootmenu-preinit`, before the supervisor starts
//! the console on the display, loads them for the GPUs `zbm.gpu` chooses
//! ([`load`]): the other GPUs get a driver override that no driver
//! matches, so that no driver takes them, and `zbm.gpu=off` loads none.
//! The console then draws on the chosen GPU's first connected display
//! ([`console_card`]). Only GPUs known to come back after `kexec` get a
//! driver, unless `zbm.gpu_untested` asks for the others too. Before
//! `kexec` the GPUs are handed back ([`hand_back`]), and taken again if it
//! fails ([`take_back`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_config::args::{Args, ZbmArgs};
use zbm_config::spec::cmdline::{GPU, GPU_UNTESTED};
use zbm_platform::{Files, System};
use zbm_tools::modules::Modules;
use zbm_tools::{Shown, udev};

/// The graphics drivers of the image, separated by spaces, below the root.
pub const DRIVERS: &str = "etc/zfsbootmenu/graphics-drivers";

/// The PCI devices, below the root.
const PCI: &str = "sys/bus/pci/devices";

/// The DRM devices, below the root.
const DRM: &str = "sys/class/drm";

/// The driver override of the GPUs left alone: no driver has this name.
const NO_DRIVER: &str = "zfsbootmenu-none";

/// How long a driver may take to bring up a chosen GPU.
const WAIT: Duration = Duration::from_secs(10);

/// How often the wait looks.
const POLL: Duration = Duration::from_millis(100);

/// The PCI vendor IDs of AMD and Intel, and of the virtual GPUs of QEMU
/// (`virtio_gpu`, and `bochs` for its standard VGA).
const AMD: u16 = 0x1002;
const INTEL: u16 = 0x8086;
const VIRTIO: u16 = 0x1af4;
const QEMU_VGA: u16 = 0x1234;

/// What `zbm.gpu` asks for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Choice {
    /// Every GPU the drivers can drive: no `zbm.gpu`.
    All,
    /// No graphics driver for this boot: `zbm.gpu=off`.
    Off,
    /// The GPUs of a vendor: `zbm.gpu=amd` or `zbm.gpu=intel`.
    Vendor(u16),
    /// One GPU by its PCI address, with its domain: `zbm.gpu=0000:03:00.0`.
    Address(String),
}

impl Choice {
    /// Reads `zbm.gpu`; a value that is none of the forms is an error.
    ///
    /// # Errors
    ///
    /// The message to log for a value that is not understood.
    pub fn parse(value: Option<&str>) -> Result<Choice, String> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(Choice::All);
        };
        match value.to_ascii_lowercase().as_str() {
            "off" => Ok(Choice::Off),
            "amd" => Ok(Choice::Vendor(AMD)),
            "intel" => Ok(Choice::Vendor(INTEL)),
            address => pci_address(address).map(Choice::Address).ok_or_else(|| {
                format!(
                    "zbm.gpu={value} is not a PCI address, amd, intel or off; \
                     the graphics drivers take every GPU"
                )
            }),
        }
    }

    /// Whether this choice takes `gpu`.
    fn is_chosen(&self, gpu: &Gpu) -> bool {
        match self {
            Choice::All => true,
            Choice::Off => false,
            Choice::Vendor(vendor) => gpu.vendor == *vendor,
            Choice::Address(address) => gpu.address == *address,
        }
    }
}

/// The value of `zbm.gpu` on the command line `kcl`.
pub fn option(kcl: &str) -> Option<String> {
    ZbmArgs::from_kcl(kcl).lookup(&GPU, &mut Vec::new())
}

/// Whether the command line `kcl` has the graphics drivers take GPUs not
/// known to come back after `kexec` (`zbm.gpu_untested`).
pub fn is_untested_allowed(kcl: &str) -> bool {
    ZbmArgs::from_kcl(kcl).enabled(&GPU_UNTESTED, &mut Vec::new())
}

/// A PCI address with its domain, `0000:03:00.0`, from one with or without
/// it, in lower case.
fn pci_address(text: &str) -> Option<String> {
    let full = if text.matches(':').count() == 1 {
        format!("0000:{text}")
    } else {
        text.to_owned()
    };
    let (domain, rest) = full.split_once(':')?;
    let (bus, rest) = rest.split_once(':')?;
    let (slot, function) = rest.split_once('.')?;
    let hex = |part: &str, len: usize| {
        part.len() == len && part.bytes().all(|byte| byte.is_ascii_hexdigit())
    };
    (hex(domain, 4) && hex(bus, 2) && hex(slot, 2) && hex(function, 1))
        .then(|| full.to_ascii_lowercase())
}

/// A display controller on the PCI bus.
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
    fn describe(&self) -> String {
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
fn drivers_of(drivers: &[String], vendor: u16) -> Vec<&str> {
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

/// The GPUs below `root` the image's `drivers` can drive, and those of them
/// `choice` takes.
fn split(
    files: &dyn Files,
    root: &Path,
    drivers: &[String],
    choice: &Choice,
) -> (Vec<Gpu>, Vec<Gpu>) {
    let drivable: Vec<Gpu> = display_controllers(files, root)
        .into_iter()
        .filter(|gpu| !drivers_of(drivers, gpu.vendor).is_empty())
        .collect();
    let chosen = drivable
        .iter()
        .filter(|gpu| choice.is_chosen(gpu))
        .cloned()
        .collect();
    (drivable, chosen)
}

/// The DRM card of the GPU at `address`, `cardN`, once a driver has made it:
/// below the PCI device, or below its virtio device (`virtio0/drm`).
fn card_name(files: &dyn Files, root: &Path, address: &str) -> Option<String> {
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
fn is_connected(files: &dyn Files, root: &Path, card: &str) -> bool {
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

/// Loads the image's graphics drivers for the GPUs `zbm.gpu` (`value`)
/// chooses and known to come back after `kexec` (all of them with
/// `untested`), after keeping the others from them, and waits until each
/// has its DRM card. Does nothing in an image without graphics drivers;
/// what goes wrong is logged, and start-up goes on.
pub fn load(system: &System<'_>, root: &Path, value: Option<&str>, untested: bool) {
    let drivers = image_drivers(system.files, root);
    if drivers.is_empty() {
        return;
    }
    let choice = Choice::parse(value).unwrap_or_else(|message| {
        zbm_trace::warn!("{message}");
        Choice::All
    });
    if choice == Choice::Off {
        zbm_trace::info!("zbm.gpu=off: the graphics drivers stay unloaded for this boot");
        return;
    }
    let (drivable, chosen) = split(system.files, root, &drivers, &choice);
    if chosen.is_empty() {
        let found: Vec<String> = drivable.iter().map(Gpu::describe).collect();
        zbm_trace::warn!(
            "no GPU for the graphics drivers {} (zbm.gpu={}; found: {}); \
             the console stays on the firmware's framebuffer",
            drivers.join(" "),
            value.unwrap_or(""),
            if found.is_empty() {
                "none".to_owned()
            } else {
                found.join(", ")
            }
        );
        return;
    }
    let taken = known_of(&chosen, untested);
    for gpu in drivable.iter().filter(|gpu| !taken.contains(gpu)) {
        leave_alone(system.files, root, gpu);
    }
    if taken.is_empty() {
        return;
    }
    load_drivers(system, &drivers, &taken);
    wait_for_cards(system, root, &taken);
    udev::settle(system.runner, Shown::Hidden);
}

/// The GPUs of `chosen` the drivers take: those known to come back after
/// `kexec`, or all of them when `untested`; says why of each other.
fn known_of(chosen: &[Gpu], untested: bool) -> Vec<Gpu> {
    let mut taken = Vec::new();
    for gpu in chosen {
        match known::known_name(gpu) {
            Some(name) => {
                zbm_trace::info!("graphics: {} is a {name}", gpu.describe());
                taken.push(gpu.clone());
            }
            None if untested => {
                zbm_trace::warn!(
                    "graphics: {} is not known to come back after kexec; \
                     its driver loads for zbm.gpu_untested",
                    gpu.describe()
                );
                taken.push(gpu.clone());
            }
            None => zbm_trace::warn!(
                "graphics: {} is not known to come back after kexec; it stays on \
                 the firmware's framebuffer (zbm.gpu_untested tries its driver)",
                gpu.describe()
            ),
        }
    }
    taken
}

/// Keeps every driver from `gpu`.
fn leave_alone(files: &dyn Files, root: &Path, gpu: &Gpu) {
    let path = root.join(PCI).join(&gpu.address).join("driver_override");
    match files.write(&path, format!("{NO_DRIVER}\n").as_bytes()) {
        Ok(()) => zbm_trace::info!("graphics: leaving {} alone (zbm.gpu)", gpu.describe()),
        Err(error) => zbm_trace::warn!(
            "graphics: unable to keep the drivers from {}: {error}",
            gpu.describe()
        ),
    }
}

/// Loads the drivers of the vendors of `chosen`.
fn load_drivers(system: &System<'_>, drivers: &[String], chosen: &[Gpu]) {
    let modules = Modules::new(system.runner);
    let mut loaded: Vec<&str> = Vec::new();
    for gpu in chosen {
        for driver in drivers_of(drivers, gpu.vendor) {
            if loaded.contains(&driver) {
                continue;
            }
            loaded.push(driver);
            match modules.load(driver) {
                Ok(output) if output.status.is_success() => {
                    zbm_trace::info!("graphics: loaded {driver}");
                }
                Ok(output) => zbm_trace::warn!(
                    "graphics: unable to load {driver}: {}",
                    output.stdout_text().trim_end()
                ),
                Err(error) => zbm_trace::warn!("graphics: unable to load {driver}: {error}"),
            }
        }
    }
}

/// Waits until each GPU of `chosen` has a DRM card, at most [`WAIT`].
fn wait_for_cards(system: &System<'_>, root: &Path, chosen: &[Gpu]) {
    let deadline = system.clock.now() + WAIT;
    loop {
        let missing: Vec<&Gpu> = chosen
            .iter()
            .filter(|gpu| card_name(system.files, root, &gpu.address).is_none())
            .collect();
        if missing.is_empty() {
            break;
        }
        if system.clock.now() >= deadline {
            for gpu in missing {
                zbm_trace::warn!(
                    "graphics: no display device for {} after {} s",
                    gpu.describe(),
                    WAIT.as_secs()
                );
            }
            break;
        }
        system.clock.sleep(POLL);
    }
    for gpu in chosen {
        if let Some(card) = card_name(system.files, root, &gpu.address) {
            let connected = if is_connected(system.files, root, &card) {
                "a display connected"
            } else {
                "no display connected"
            };
            zbm_trace::info!("graphics: {} is {card}, {connected}", gpu.describe());
        }
    }
}

/// The card the console on the display draws on in an image with graphics
/// drivers: of the GPUs `zbm.gpu` (`value`) chooses, the first with a
/// connected display, else the first with a card. `None` without graphics
/// drivers, with `zbm.gpu=off` and when no chosen GPU has a card: the
/// console takes the first card then.
pub fn console_card(files: &dyn Files, root: &Path, value: Option<&str>) -> Option<PathBuf> {
    let drivers = image_drivers(files, root);
    if drivers.is_empty() {
        return None;
    }
    let choice = Choice::parse(value).unwrap_or(Choice::All);
    let (_, chosen) = split(files, root, &drivers, &choice);
    let cards: Vec<String> = chosen
        .iter()
        .filter_map(|gpu| card_name(files, root, &gpu.address))
        .collect();
    let card = cards
        .iter()
        .find(|card| is_connected(files, root, card))
        .or_else(|| cards.first())?;
    Some(root.join("dev/dri").join(card))
}

mod hand_back;
mod known;
pub use hand_back::{hand_back, take_back};

#[cfg(test)]
mod tests;
