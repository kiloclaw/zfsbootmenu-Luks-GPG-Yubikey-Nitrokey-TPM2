//! The graphics drivers of the images and the firmware of their GPUs
//! (`Kernel.GraphicsDrivers`, ROADMAP.md #17); the Perl program has no such
//! key. The option is read before anything is built; the GPUs and firmware
//! are chosen once the kernel is known, from the firmware its drivers list.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use zbm_config::spec::generate::KERNEL_GRAPHICS_DRIVERS;
use zbm_core::shell::quote;

use super::settings::Settings;
use super::{Generator, Step, Stop};
use crate::config::Value;
use crate::gpu::{self, Driver, Request, Selection};
use crate::perl;

impl Generator<'_, '_, '_> {
    /// `Kernel.GraphicsDrivers`; a value that names an unknown driver, chip
    /// or family stops the run before anything is built.
    pub(super) fn graphics_requests(&mut self) -> Step<Vec<Request>> {
        let words: Vec<String> = match self.config.option(&KERNEL_GRAPHICS_DRIVERS)? {
            Some(Value::Seq(items)) => items
                .borrow()
                .iter()
                .filter_map(Value::perl_string)
                .collect(),
            Some(value) => value.perl_string().into_iter().collect(),
            None => Vec::new(),
        };
        gpu::parse(&words).map_err(|message| self.graphics_error(&message))
    }

    /// Chooses the GPUs and their firmware for the kernel `version`, reports
    /// them, and removes `nomodeset` from the command line.
    pub(super) fn choose_graphics(&mut self, version: &str, settings: &mut Settings) -> Step<()> {
        if settings.graphics_requests.is_empty() {
            return Ok(());
        }
        let devices = if settings
            .graphics_requests
            .iter()
            .any(|request| request.host)
        {
            gpu::host::scan(&self.context.pci_devices)
        } else {
            Vec::new()
        };
        let choices = gpu::choose(&settings.graphics_requests, &devices)
            .map_err(|message| self.graphics_error(&message))?;
        let mut listed: BTreeMap<Driver, Vec<String>> = BTreeMap::new();
        for choice in &choices {
            if let Entry::Vacant(entry) = listed.entry(choice.driver) {
                entry.insert(self.listed_firmware(choice.driver, version)?);
            }
        }
        let selection = gpu::select(&choices, &listed)
            .map_err(|message| self.graphics_error(&format!("{message} (kernel {version})")))?;
        self.report_graphics(&selection);
        if let Some(cmdline) = &settings.cmdline
            && let Some(without) = without_word(cmdline, "nomodeset")
        {
            self.print("Removing nomodeset from the command line, for the graphics drivers\n");
            settings.cmdline = Some(without);
        }
        settings.graphics = Some(selection);
        Ok(())
    }

    fn report_graphics(&mut self, selection: &Selection) {
        for line in &selection.report {
            self.print(&format!("Graphics: {line}\n"));
        }
        if !selection.unlisted.is_empty() {
            self.log(&format!(
                "Firmware stems the drivers do not list: {}",
                selection.unlisted.join(" ")
            ));
        }
        self.log(&format!(
            "Graphics firmware: {}",
            selection.firmware.join(" ")
        ));
    }

    /// The firmware files `driver` of kernel `version` lists.
    fn listed_firmware(&mut self, driver: Driver, version: &str) -> Step<Vec<String>> {
        let command = format!(
            "modinfo -k {} -F firmware {}",
            quote(version),
            driver.name()
        );
        self.log(&format!("Executing: {command}"));
        let (lines, status) = self.execute(&command)?;
        if status != 0 {
            self.print_lines(&lines);
            self.print(&format!(
                "Unable to read the firmware list of {} for kernel {version}\n",
                driver.name()
            ));
            return Err(Stop::Exit(1));
        }
        Ok(lines
            .iter()
            .map(|line| String::from_utf8_lossy(perl::chomp(line)).trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect())
    }

    fn graphics_error(&mut self, message: &str) -> Stop {
        self.print(&format!("Kernel.GraphicsDrivers: {message}\n"));
        Stop::Exit(1)
    }
}

/// The words `dracut` and `mkinitcpio` read from the environment for the
/// graphics drivers; the dracut module and the mkinitcpio hook install them.
pub(super) fn build_options(selection: &Selection) -> [String; 2] {
    let drivers: Vec<&str> = selection
        .drivers
        .iter()
        .map(|driver| driver.name())
        .collect();
    [
        format!("zfsbootmenu_graphics_drivers={}", quote(&drivers.join(" "))),
        format!(
            "zfsbootmenu_graphics_firmware={}",
            quote(&selection.firmware.join(" "))
        ),
    ]
}

/// `text` without the space-separated word `word`, or `None` when it does
/// not have it. Other spacing is kept, so that quoted values stay as they
/// are.
fn without_word(text: &str, word: &str) -> Option<String> {
    let parts: Vec<&str> = text.split(' ').collect();
    let kept: Vec<&str> = parts.iter().copied().filter(|part| *part != word).collect();
    (kept.len() != parts.len()).then(|| kept.join(" ").trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nomodeset_is_removed_as_a_word() {
        assert_eq!(
            without_word("ro quiet nomodeset loglevel=4", "nomodeset").as_deref(),
            Some("ro quiet loglevel=4")
        );
        assert_eq!(without_word("nomodeset", "nomodeset").as_deref(), Some(""));
        assert_eq!(without_word("a=\"nomodeset b\"", "nomodeset"), None);
        assert_eq!(without_word("ro quiet", "nomodeset"), None);
    }

    #[test]
    fn the_build_options_name_the_drivers_and_files() {
        let selection = Selection {
            drivers: vec![Driver::Amdgpu, Driver::I915],
            firmware: vec!["amdgpu/a.bin".to_owned(), "i915/b.bin".to_owned()],
            ..Selection::default()
        };

        assert_eq!(
            build_options(&selection),
            [
                "zfsbootmenu_graphics_drivers='amdgpu i915'",
                "zfsbootmenu_graphics_firmware='amdgpu/a.bin i915/b.bin'",
            ]
        );
    }
}
