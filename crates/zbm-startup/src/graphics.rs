//! The graphics drivers of the image (ROADMAP.md #17), loaded before
//! anything is asked, so that the display does not change mode under a
//! passphrase prompt.
//!
//! An image with graphics drivers (`Kernel.GraphicsDrivers` of `generate-zbm`)
//! lists them in `zbm_boot::graphics::DRIVERS` and keeps udev from loading them
//! (`modprobe.d`). `zfsbootmenu-preinit`, before the supervisor starts the
//! console on the display, loads them for the GPUs `zbm.gpu` chooses
//! ([`load`]): the other GPUs get a driver override that no driver matches, so
//! that no driver takes them, and `zbm.gpu=off` loads none. The console then
//! draws on the chosen GPU's first connected display ([`console_card`]). Only
//! GPUs known to come back after `kexec` get a driver, unless
//! `zbm.gpu_untested` asks for the others too. Before `kexec`,
//! `zbm_boot::graphics` hands the GPUs back.

use std::path::{Path, PathBuf};

use zbm_boot::graphics::{
    AMD, Gpu, INTEL, PCI, card_name, display_controllers, drivers_of, image_drivers, is_connected,
    wait_for_cards,
};
use zbm_config::args::{Args, ZbmArgs};
use zbm_config::spec::cmdline::{GPU, GPU_UNTESTED};
use zbm_platform::{Context, Files, System};
use zbm_tools::modules::Modules;
use zbm_tools::{Shown, udev};

/// The driver override of the GPUs left alone: no driver has this name.
const NO_DRIVER: &str = "zfsbootmenu-none";

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
    ZbmArgs::from_cmdline(kcl).lookup(&GPU, &mut Vec::new())
}

/// Whether the command line `kcl` has the graphics drivers take GPUs not
/// known to come back after `kexec` (`zbm.gpu_untested`).
pub fn is_untested_allowed(kcl: &str) -> bool {
    ZbmArgs::from_cmdline(kcl).enabled(&GPU_UNTESTED, &mut Vec::new())
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
        zbm_trace::warn!("{what}"; what = %message);
        Choice::All
    });
    if choice == Choice::Off {
        zbm_trace::info!("zbm.gpu=off: the graphics drivers stay unloaded for this boot");
        return;
    }
    let (drivable, chosen) = split(system.files, root, &drivers, &choice);
    if chosen.is_empty() {
        let found = described(&drivable);
        zbm_trace::warn!(
            "no GPU for the graphics drivers {drivers} (zbm.gpu={wanted}; found: {found}); \
             the console stays on the firmware's framebuffer";
            drivers = %drivers.join(" "), wanted = %value.unwrap_or(""), %found
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
    wait_for_cards(&Context::new(*system), root, &taken);
    udev::settle(system.runner, Shown::Hidden);
}

/// The GPUs of `chosen` the drivers take: those known to come back after
/// `kexec`, or all of them when `untested`; says why of each other.
fn known_of(chosen: &[Gpu], untested: bool) -> Vec<Gpu> {
    let mut taken = Vec::new();
    for gpu in chosen {
        match known::known_name(gpu) {
            Some(name) => {
                zbm_trace::info!("graphics: {gpu} is a {name}"; gpu = %gpu.describe(), %name);
                taken.push(gpu.clone());
            }
            None if untested => {
                zbm_trace::warn!(
                    "graphics: {gpu} is not known to come back after kexec; \
                     its driver loads for zbm.gpu_untested";
                    gpu = %gpu.describe()
                );
                taken.push(gpu.clone());
            }
            None => zbm_trace::warn!(
                "graphics: {gpu} is not known to come back after kexec; it stays on \
                 the firmware's framebuffer (zbm.gpu_untested tries its driver)";
                gpu = %gpu.describe()
            ),
        }
    }
    taken
}

/// The GPUs `gpus` for the log: `none`, or each described, joined by commas.
fn described(gpus: &[Gpu]) -> String {
    if gpus.is_empty() {
        return "none".to_owned();
    }
    gpus.iter()
        .map(Gpu::describe)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Keeps every driver from `gpu`.
fn leave_alone(files: &dyn Files, root: &Path, gpu: &Gpu) {
    let path = root.join(PCI).join(&gpu.address).join("driver_override");
    match files.write(&path, format!("{NO_DRIVER}\n").as_bytes()) {
        Ok(()) => {
            zbm_trace::info!("graphics: leaving {gpu} alone (zbm.gpu)"; gpu = %gpu.describe());
        }
        Err(error) => zbm_trace::warn!(
            "graphics: unable to keep the drivers from {gpu}: {error}";
            gpu = %gpu.describe(), %error
        ),
    }
}

/// Loads the drivers of the vendors of `chosen`.
fn load_drivers(system: &System<'_>, drivers: &[String], chosen: &[Gpu]) {
    let modules = Modules::of(system);
    let mut loaded: Vec<&str> = Vec::new();
    for gpu in chosen {
        for driver in drivers_of(drivers, gpu.vendor) {
            if loaded.contains(&driver) {
                continue;
            }
            loaded.push(driver);
            match modules.load(driver) {
                Ok(()) => zbm_trace::info!("graphics: loaded {driver}"; %driver),
                Err(output) => zbm_trace::warn!(
                    "graphics: unable to load {driver}: {output}";
                    %driver, %output
                ),
            }
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

mod known;

#[cfg(test)]
mod tests;
