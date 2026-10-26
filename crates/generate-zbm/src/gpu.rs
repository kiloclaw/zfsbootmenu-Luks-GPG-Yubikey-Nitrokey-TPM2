//! Graphics drivers in the image, and the firmware of the GPUs it is for
//! (`Kernel.GraphicsDrivers`, ROADMAP.md #17).
//!
//! `amdgpu` lists hundreds of firmware files, tens of megabytes, and one chip
//! loads a few of them; `i915` and `xe` list a few for each platform. The
//! image takes only the files of the GPUs it is built for:
//!
//! - a driver named alone, `amdgpu`, is for the GPUs of the build host, found
//!   by their PCI IDs ([`host::scan`]);
//! - a driver with names after it, `amdgpu:gfx12,navi10`, is for those chips
//!   and families of [the table](tables::CHIPS), for an image built on
//!   another machine or for several.
//!
//! [`parse`] reads the option, [`choose`] picks the chips, and [`select`]
//! picks their files from the list the build's kernel gives for each driver.

pub mod host;
mod ids;
pub mod tables;

use std::collections::BTreeMap;

use host::Device;
use tables::Chip;

/// A graphics driver `generate-zbm` can add to an image.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Driver {
    /// AMD GPUs from GCN 3 on.
    Amdgpu,
    /// Intel graphics up to Meteor Lake.
    I915,
    /// Intel graphics from Lunar Lake on.
    Xe,
}

impl Driver {
    /// Every driver.
    pub const ALL: [Driver; 3] = [Driver::Amdgpu, Driver::I915, Driver::Xe];

    /// The name of its kernel module.
    pub fn name(self) -> &'static str {
        match self {
            Driver::Amdgpu => "amdgpu",
            Driver::I915 => "i915",
            Driver::Xe => "xe",
        }
    }

    /// The PCI vendor ID of its GPUs.
    pub fn vendor(self) -> u16 {
        match self {
            Driver::Amdgpu => 0x1002,
            Driver::I915 | Driver::Xe => 0x8086,
        }
    }

    fn vendor_name(self) -> &'static str {
        match self {
            Driver::Amdgpu => "AMD",
            Driver::I915 | Driver::Xe => "Intel",
        }
    }

    /// The firmware directories it loads from.
    pub fn firmware_dirs(self) -> &'static [&'static str] {
        match self {
            Driver::Amdgpu => &["amdgpu"],
            Driver::I915 => &["i915"],
            Driver::Xe => &["xe", "i915"],
        }
    }

    fn from_name(name: &str) -> Option<Driver> {
        Driver::ALL.into_iter().find(|driver| driver.name() == name)
    }
}

/// One driver of the option and the GPUs it is for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    /// The driver.
    pub driver: Driver,
    /// Whether it is for the build host's GPUs.
    pub host: bool,
    /// The chips and families named after it.
    pub names: Vec<String>,
}

/// Reads `Kernel.GraphicsDrivers`: words `driver` or `driver:name,name`,
/// where a name is a chip or a family of the driver.
///
/// # Errors
///
/// A message naming an unknown driver, chip or family.
pub fn parse(words: &[String]) -> Result<Vec<Request>, String> {
    let mut requests: Vec<Request> = Vec::new();
    for word in words.iter().flat_map(|text| text.split_whitespace()) {
        let word = word.to_lowercase();
        let (driver_name, names) = match word.split_once(':') {
            Some((driver, names)) => (driver, Some(names)),
            None => (word.as_str(), None),
        };
        let Some(driver) = Driver::from_name(driver_name) else {
            return Err(format!(
                "unknown graphics driver '{driver_name}': the drivers are amdgpu, i915 and xe"
            ));
        };
        let index = match requests.iter().position(|request| request.driver == driver) {
            Some(index) => index,
            None => {
                requests.push(Request {
                    driver,
                    host: false,
                    names: Vec::new(),
                });
                requests.len() - 1
            }
        };
        let request = &mut requests[index];
        let Some(names) = names else {
            request.host = true;
            continue;
        };
        for name in names.split(',') {
            check_name(driver, name)?;
            if !request.names.iter().any(|known| known == name) {
                request.names.push(name.to_owned());
            }
        }
    }
    Ok(requests)
}

/// Fails unless `name` is a chip or a family of `driver`.
fn check_name(driver: Driver, name: &str) -> Result<(), String> {
    if !tables::chips_named(driver, name).is_empty() {
        return Ok(());
    }
    let (families, chips) = tables::names(driver);
    Err(format!(
        "{} has no chip or family '{name}': its families are {}, and its chips {}",
        driver.name(),
        families.join(", "),
        chips.join(", ")
    ))
}

/// One GPU, or group of GPUs, and the stems of its firmware. Each stem has
/// alternatives, the first the driver lists is taken; the first stem must be
/// listed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Choice {
    /// The driver.
    pub driver: Driver,
    /// What the choice is for, as `generate-zbm` reports it.
    pub label: String,
    /// The stems, each with its alternatives.
    pub stems: Vec<Vec<String>>,
    /// Whether the stems are IP versions, which another version must not
    /// continue ([`has_version_stem`]).
    pub versioned: bool,
    /// The family it was asked for by, if any: a chip of a family that the
    /// kernel does not know is left out, as long as it knows another.
    pub family: Option<String>,
}

impl Choice {
    fn of_chip(chip: &Chip, label: String) -> Choice {
        Choice {
            driver: chip.driver,
            label,
            stems: chip
                .stems
                .iter()
                .map(|stem| vec![(*stem).to_owned()])
                .collect(),
            versioned: false,
            family: None,
        }
    }
}

/// The choices for the requests: the chips they name and the GPUs of the
/// build host, `devices`, for drivers named alone.
///
/// # Errors
///
/// A message when a driver named alone has no GPU of the host to drive, or
/// a GPU of the host is not one the table or the running driver knows.
pub fn choose(requests: &[Request], devices: &[Device]) -> Result<Vec<Choice>, String> {
    let mut choices = Vec::new();
    for request in requests {
        for name in &request.names {
            for chip in tables::chips_named(request.driver, name) {
                let mut choice = Choice::of_chip(chip, format!("{} ({})", chip.name, chip.family));
                if chip.family == name {
                    choice.family = Some(name.clone());
                }
                choices.push(choice);
            }
        }
        if request.host {
            choices.extend(host_choices(request.driver, devices)?);
        }
    }
    Ok(choices)
}

/// The choices for the host's GPUs that `driver` drives.
fn host_choices(driver: Driver, devices: &[Device]) -> Result<Vec<Choice>, String> {
    let mut choices = Vec::new();
    let mut others = Vec::new();
    for device in devices
        .iter()
        .filter(|device| device.vendor == driver.vendor())
    {
        let id = format!(
            "{} ({:04x}:{:04x})",
            device.address, device.vendor, device.device
        );
        let chip = tables::chip_of(device.vendor, device.device);
        if let Some(chip) = chip.filter(|chip| chip.driver != driver) {
            others.push(format!(
                "{id} is {}, which {} drives",
                chip.name,
                chip.driver.name()
            ));
            continue;
        }
        choices.push(device_choice(driver, device, chip, &id)?);
    }
    if choices.is_empty() {
        let mut message = format!(
            "{} is for the GPUs of this machine, and it has no {} GPU for it",
            driver.name(),
            driver.vendor_name()
        );
        for other in others {
            message.push_str(&format!("; {other}"));
        }
        message.push_str(&format!(
            ": name the chips or families to build for, such as {}",
            example(driver)
        ));
        return Err(message);
    }
    Ok(choices)
}

/// The choice for one GPU of the host, whose chip in the table is `chip`.
fn device_choice(
    driver: Driver,
    device: &Device,
    chip: Option<&Chip>,
    id: &str,
) -> Result<Choice, String> {
    match chip {
        Some(chip) if !chip.discovery || device.blocks.is_empty() => {
            Ok(Choice::of_chip(chip, format!("{} at {id}", chip.name)))
        }
        _ if !device.blocks.is_empty() => Ok(discovered_choice(device, chip, id)),
        _ => Err(format!(
            "the {} GPU at {id} is not one generate-zbm knows, and no running {} shows its \
             model: name the chips or families to build for, such as {}",
            driver.vendor_name(),
            driver.name(),
            example(driver)
        )),
    }
}

/// The choice for an AMD GPU whose IP versions the running driver shows:
/// the stems are named after them, with the revision left out when it is 0
/// as a second choice (`dcn_3_5` for DCN 3.5.0).
fn discovered_choice(device: &Device, chip: Option<&Chip>, id: &str) -> Choice {
    let mut stems = Vec::new();
    let mut gc = String::new();
    for (block, prefix) in host::BLOCKS {
        let Some((_, (major, minor, revision))) =
            device.blocks.iter().find(|(name, _)| name == block)
        else {
            continue;
        };
        if *block == "GC" {
            gc = format!("GC {major}.{minor}.{revision}");
        }
        let mut alternatives = vec![format!("amdgpu/{prefix}_{major}_{minor}_{revision}")];
        if *revision == 0 {
            alternatives.push(format!("amdgpu/{prefix}_{major}_{minor}"));
        }
        stems.push(alternatives);
    }
    let label = match chip {
        Some(chip) => format!("{}, {gc}, at {id}", chip.name),
        None => format!("{gc} at {id}"),
    };
    Choice {
        driver: Driver::Amdgpu,
        label,
        stems,
        versioned: true,
        family: None,
    }
}

fn example(driver: Driver) -> &'static str {
    match driver {
        Driver::Amdgpu => "amdgpu:gfx12",
        Driver::I915 => "i915:gen12",
        Driver::Xe => "xe:xe2",
    }
}

/// The drivers and firmware files of an image.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    /// The drivers, in the order the option names them.
    pub drivers: Vec<Driver>,
    /// The firmware files, as the drivers list them, sorted.
    pub firmware: Vec<String>,
    /// A line for each choice: what it is for and how many files it takes.
    pub report: Vec<String>,
    /// Stems the drivers do not list, which are left out.
    pub unlisted: Vec<String>,
}

/// Whether the firmware file `name` has the stem `stem`.
pub fn has_stem(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem)
        .is_some_and(|rest| rest.starts_with(['_', '.']))
}

/// Whether the firmware file `name` has the IP version stem `stem`: as
/// [`has_stem`], but not followed by a further number, so that `dcn_3_5`
/// takes `dcn_3_5_dmcub.bin` and not `dcn_3_5_1_dmcub.bin`.
pub fn has_version_stem(name: &str, stem: &str) -> bool {
    name.strip_prefix(stem).is_some_and(|rest| {
        rest.starts_with('.')
            || rest
                .strip_prefix('_')
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit()))
    })
}

/// The files of `choice` among those its driver lists, `files`, and the
/// stems it does not list; `None` when it lists none of the first stem.
fn files_of<'a>(choice: &Choice, files: &'a [String]) -> Option<(Vec<&'a String>, Vec<String>)> {
    let matches = if choice.versioned {
        has_version_stem
    } else {
        has_stem
    };
    let mut taken: Vec<&String> = Vec::new();
    let mut unlisted = Vec::new();
    for (index, alternatives) in choice.stems.iter().enumerate() {
        let matching = alternatives.iter().find_map(|stem| {
            let found: Vec<&String> = files.iter().filter(|file| matches(file, stem)).collect();
            (!found.is_empty()).then_some(found)
        });
        match matching {
            Some(found) => taken.extend(found),
            None if index == 0 => return None,
            None => unlisted.push(alternatives[0].clone()),
        }
    }
    taken.sort();
    taken.dedup();
    Some((taken, unlisted))
}

/// The firmware of the choices, from the files each driver lists.
///
/// # Errors
///
/// A message when a driver lists no file of the first stem of a chip named
/// alone or of the build host, or of any chip of a family: the kernel does
/// not know that GPU.
pub fn select(
    choices: &[Choice],
    listed: &BTreeMap<Driver, Vec<String>>,
) -> Result<Selection, String> {
    let mut selection = Selection::default();
    // Each family asked for, and whether the kernel knows a chip of it
    let mut families: Vec<(Driver, &str, bool)> = Vec::new();
    for choice in choices {
        let files = listed.get(&choice.driver).map_or(&[][..], Vec::as_slice);
        let found = files_of(choice, files);
        if let Some(family) = &choice.family {
            match families
                .iter_mut()
                .find(|(driver, name, _)| *driver == choice.driver && name == family)
            {
                Some(entry) => entry.2 |= found.is_some(),
                None => families.push((choice.driver, family, found.is_some())),
            }
        }
        let name = choice.driver.name();
        let Some((taken, unlisted)) = found else {
            if choice.family.is_none() {
                return Err(format!(
                    "{name} lists no firmware {}* for {}: the kernel does not know this GPU",
                    choice.stems[0][0], choice.label
                ));
            }
            let label = &choice.label;
            selection.report.push(format!(
                "{name}: {label}: not known to the kernel, left out"
            ));
            continue;
        };
        if !selection.drivers.contains(&choice.driver) {
            selection.drivers.push(choice.driver);
        }
        selection.unlisted.extend(unlisted);
        let count = taken.len();
        selection
            .report
            .push(format!("{name}: {}: {count} firmware files", choice.label));
        selection.firmware.extend(taken.into_iter().cloned());
    }
    if let Some((driver, family, _)) = families.iter().find(|(_, _, known)| !known) {
        return Err(format!(
            "{} lists no firmware for any GPU of {family}: the kernel does not know them",
            driver.name()
        ));
    }
    selection.firmware.sort();
    selection.firmware.dedup();
    selection.unlisted.sort();
    selection.unlisted.dedup();
    Ok(selection)
}

#[cfg(test)]
mod tests;
