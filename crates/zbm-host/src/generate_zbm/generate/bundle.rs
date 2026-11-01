//! The EFI bundle (ADR 0014): the stub loader with the release information,
//! the command line, a splash image, a device tree, the initramfs and the
//! kernel added as sections by [`crate::pe`]. The bundle is planned, its
//! stub and the files of its sections, before anything is built; a dry run
//! prints the plan.

use super::settings::Settings;
use super::{Generator, Step, Stop};
use crate::pe::{Image, WriteOptions};
use zbm_config::spec::generate::{EFI_DEVICE_TREE, EFI_SPLASH_IMAGE, EFI_STUB};
use zbm_platform::filetest::{is_file, strerror};

/// What a section holds.
enum Contents {
    /// The contents of a file.
    File(String),
    /// The command line, as given.
    CommandLine(String),
}

/// A section of the bundle, before it is built.
struct Planned {
    name: &'static str,
    contents: Contents,
}

impl Planned {
    /// Where its contents come from, for the log and the dry run.
    fn source(&self) -> String {
        match &self.contents {
            Contents::File(path) => format!("from {path}"),
            Contents::CommandLine(_) => "with the command line".to_owned(),
        }
    }
}

/// The bundle, before it is built (ADR 0014, point 8): the stub and its
/// sections, in order. The kernel goes last, so that it can decompress
/// without overwriting the other sections.
struct BundlePlan {
    stub: String,
    output: String,
    sections: Vec<Planned>,
}

impl BundlePlan {
    /// The plan in a line: the output, the stub and each section.
    fn describe(&self) -> String {
        let sections: Vec<String> = self
            .sections
            .iter()
            .map(|section| format!("{} {}", section.name, section.source()))
            .collect();
        format!(
            "build {} from {}: {}",
            self.output,
            self.stub,
            sections.join(", ")
        )
    }
}

impl Generator<'_, '_, '_> {
    /// Builds the bundle of the stub, the release information, the command
    /// line, a splash image, a device tree, the initramfs and the kernel.
    pub(super) fn create_bundle(
        &mut self,
        dir: &str,
        kernel: &str,
        initramfs: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<String> {
        let output = format!("{dir}/zfsbootmenu.EFI");
        if !(self.dry_run || is_file(kernel) && is_file(initramfs)) {
            self.print("Cannot find kernel or initramfs to create UEFI bundle\n");
            return Err(Stop::Exit(1));
        }
        let stub = self.stub()?;
        let mut sections = self.optional_sections(kernel_version, settings)?;
        sections.push(Planned {
            name: ".initrd",
            contents: Contents::File(initramfs.to_owned()),
        });
        sections.push(Planned {
            name: ".linux",
            contents: Contents::File(kernel.to_owned()),
        });
        let plan = BundlePlan {
            stub,
            output: output.clone(),
            sections,
        };
        if !self.is_dry_run_of(&plan.describe()) {
            self.build_bundle(&plan)?;
        }
        Ok(output)
    }

    /// Builds the bundle `plan` describes, and writes it.
    fn build_bundle(&mut self, plan: &BundlePlan) -> Step<()> {
        let stub = &plan.stub;
        let bytes = match std::fs::read(stub) {
            Ok(bytes) => bytes,
            Err(error) => {
                let why = strerror(&error);
                return self.stop(&format!("Unable to read UEFI stub loader '{stub}': {why}"));
            }
        };
        let mut image = match Image::read(bytes) {
            Ok(image) => image,
            Err(error) => {
                return self.stop(&format!(
                    "UEFI stub loader '{stub}' cannot be used: {error}"
                ));
            }
        };
        for section in &plan.sections {
            let data = match &section.contents {
                Contents::CommandLine(text) => text.clone().into_bytes(),
                Contents::File(path) => match std::fs::read(path) {
                    Ok(data) => data,
                    Err(error) => {
                        let (name, why) = (section.name, strerror(&error));
                        return self.stop(&format!("Unable to read {path} for {name}: {why}"));
                    }
                },
            };
            if let Err(error) = image.add(section.name, &data) {
                return self.stop(&format!("Unable to add {}: {error}", section.name));
            }
        }
        let options = WriteOptions {
            timestamp: self.context.timestamp,
        };
        let written = match image.write(&options) {
            Ok(written) => written,
            Err(error) => {
                return self.stop(&format!("Unable to build {}: {error}", plan.output));
            }
        };
        for warning in &written.warnings {
            self.print(&format!("WARNING: {warning}\n"));
        }
        self.log_layout(plan, &written.bytes, image.image_base());
        if std::fs::write(&plan.output, &written.bytes).is_err() {
            return self.stop(&format!("Failed to create {}", plan.output));
        }
        Ok(())
    }

    /// Prints `message` and ends the run with status 1.
    fn stop(&mut self, message: &str) -> Step<()> {
        self.print(&format!("{message}\n"));
        Err(Stop::Exit(1))
    }

    /// Logs where each section of the bundle `bytes` went, at its address
    /// with the image base, as the stub's sections are loaded.
    fn log_layout(&mut self, plan: &BundlePlan, bytes: &[u8], base: u64) {
        let Ok(built) = Image::read(bytes.to_vec()) else {
            return;
        };
        for planned in &plan.sections {
            if let Some(section) = built.section(planned.name) {
                let address = base + u64::from(section.virtual_address());
                self.log(&format!(
                    "Adding {} {} at 0x{address:X}",
                    planned.name,
                    planned.source()
                ));
            }
        }
        self.log(&format!("Writing {} from {}", plan.output, plan.stub));
    }

    /// The stub loader: `EFI.Stub`, or the first default stub that exists.
    fn stub(&mut self) -> Step<String> {
        if let Some(stub) = self.setting(&EFI_STUB)?.filter(|text| !text.is_empty()) {
            if !is_file(&stub) {
                self.print(&format!("UEFI stub loader '{stub}' does not exist\n"));
                return Err(Stop::Exit(1));
            }
            return Ok(stub);
        }
        if let Some(stub) = self.context.default_stubs.iter().find(|stub| is_file(stub)) {
            return Ok(stub.clone());
        }
        self.print("Unable to find UEFI stub loader at default locations:\n");
        for stub in &self.context.default_stubs.clone() {
            self.print(&format!(" {stub}\n"));
        }
        Err(Stop::Exit(1))
    }

    /// The sections that only some bundles have, in order: the release
    /// information, the command line, the splash image and the device tree.
    fn optional_sections(
        &mut self,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<Vec<Planned>> {
        let mut sections = Vec::new();
        let os_release = self.context.os_release.display().to_string();
        if is_file(&os_release) {
            sections.push(Planned {
                name: ".osrel",
                contents: Contents::File(os_release),
            });
        }
        if let Some(cmdline) = settings.cmdline.clone().filter(|text| !text.is_empty()) {
            sections.push(Planned {
                name: ".cmdline",
                contents: Contents::CommandLine(cmdline),
            });
        }
        if let Some(splash) = self
            .setting(&EFI_SPLASH_IMAGE)?
            .filter(|text| !text.is_empty())
            && is_file(&splash)
        {
            sections.push(Planned {
                name: ".splash",
                contents: Contents::File(splash),
            });
        }
        if let Some(path) = self.device_tree(kernel_version)? {
            sections.push(Planned {
                name: ".dtb",
                contents: Contents::File(path),
            });
        }
        Ok(sections)
    }

    /// The device tree file `EFI.DeviceTree` names for the kernel version, if
    /// it is set; a file that does not exist ends the program.
    fn device_tree(&mut self, kernel_version: &str) -> Step<Option<String>> {
        let Some(tree) = self
            .setting(&EFI_DEVICE_TREE)?
            .filter(|text| !text.is_empty())
        else {
            return Ok(None);
        };
        let path = if tree.starts_with('/') {
            tree.replacen("%{kernel}", kernel_version, 1)
        } else {
            format!("{}/dtbs-{kernel_version}/{tree}", self.context.dtb_dir)
        };
        if !is_file(&path) {
            self.print(&format!(
                "EFI.DeviceTree key set to '{tree}', file not found at '{path}'\n"
            ));
            return Err(Stop::Exit(1));
        }
        Ok(Some(path))
    }
}
