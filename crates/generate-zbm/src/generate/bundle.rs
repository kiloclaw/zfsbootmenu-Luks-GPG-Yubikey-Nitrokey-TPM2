//! The EFI bundle: the stub loader with the release information, the command
//! line, a splash image, a device tree, the initramfs and the kernel added as
//! sections by objcopy.

use zbm_config::spec::generate::{EFI_DEVICE_TREE, EFI_SPLASH_IMAGE, EFI_STUB};
use zbm_core::fs::is_file;
use zbm_core::shell::quote;

use super::settings::Settings;
use super::{Generator, Step, Stop};
use crate::efi;

/// The objcopy command being built, with the address of the next section.
struct Sections {
    /// The command's words so far.
    words: Vec<String>,
    /// The address of the next section.
    offset: u128,
    /// The section alignment of the stub.
    alignment: u128,
}

impl Generator<'_, '_, '_> {
    /// `increaseBundleOffset`.
    fn next_offset(&mut self, size: u128, offset: u128, alignment: u128) -> u128 {
        let next = efi::next_offset(size, offset, alignment);
        self.log(&format!("New offset is: 0x{next:X}"));
        next
    }

    /// `addBundleSection`.
    fn add_section(&mut self, sections: &mut Sections, name: &str, file: &str) -> Step<()> {
        let offset = sections.offset;
        sections.words.push("--add-section".to_owned());
        sections.words.push(format!("{name}={}", quote(file)));
        sections.words.push("--change-section-vma".to_owned());
        sections.words.push(format!("{name}=0x{offset:X}"));
        let size = std::fs::metadata(file)
            .map_err(|error| {
                Stop::fatal(
                    "Can't call method \"size\" on an undefined value".to_owned(),
                    &error,
                )
            })?
            .len();
        sections.offset = self.next_offset(u128::from(size), offset, sections.alignment);
        Ok(())
    }

    /// `createUEFIBundle`: combines the stub, the release information, the command
    /// line, a splash image, a device tree, the initramfs and the kernel with
    /// objcopy.
    pub(super) fn create_bundle(
        &mut self,
        dir: &str,
        kernel: &str,
        initramfs: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<String> {
        let output = format!("{dir}/zfsbootmenu.EFI");
        if !(is_file(kernel) && is_file(initramfs)) {
            self.print("Cannot find kernel or initramfs to create UEFI bundle\n");
            return Err(Stop::Exit(1));
        }
        let stub = self.stub()?;
        let alignment = self.stub_alignment(&stub)?;
        let offset = self.initial_offset(&stub, alignment)?;
        let mut sections = Sections {
            words: vec!["objcopy".to_owned()],
            offset,
            alignment,
        };
        self.add_optional_sections(&mut sections, dir, kernel_version, settings)?;
        self.add_section(&mut sections, ".initrd", initramfs)?;
        // The kernel goes last, so that it can decompress without overwriting other
        // sections.
        self.add_section(&mut sections, ".linux", kernel)?;
        let mut words = sections.words;
        words.push(quote(&stub));
        words.push(quote(&output));
        self.run_build_step(&words, &output)?;
        Ok(output)
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
        match self.context.default_stubs.iter().find(|stub| is_file(stub)) {
            Some(stub) => Ok(stub.clone()),
            None => {
                self.print("Unable to find UEFI stub loader at default locations:\n");
                for stub in &self.context.default_stubs.clone() {
                    self.print(&format!(" {stub}\n"));
                }
                Err(Stop::Exit(1))
            }
        }
    }

    /// The section alignment of the stub, from objdump's private headers.
    fn stub_alignment(&mut self, stub: &str) -> Step<u128> {
        let (lines, status) = self.execute(&format!("objdump -p {}", quote(stub)))?;
        let mut alignment = None;
        if status == 0 {
            for line in &lines {
                if let Some(found) = efi::section_alignment(line) {
                    self.log(&format!("Alignment is: {found}"));
                    alignment = Some(found);
                }
            }
        }
        match alignment {
            Some(alignment) if status == 0 && alignment > 0 => Ok(u128::from(alignment)),
            _ => {
                self.print("Unable to determine stub alignment!\n");
                Err(Stop::Exit(1))
            }
        }
    }

    /// The address of the first added section: past the end of the stub's last
    /// section, aligned.
    fn initial_offset(&mut self, stub: &str, alignment: u128) -> Step<u128> {
        let (lines, status) = self.execute(&format!("objdump -w -h {}", quote(stub)))?;
        let end = lines.last().and_then(|line| efi::last_section_end(line));
        let Some(end) = end.filter(|_| status == 0) else {
            self.print("Unable to determine initial stub offset!\n");
            return Err(Stop::Exit(1));
        };
        let offset = self.next_offset(end, 0, alignment);
        self.log(&format!("Initial offset is: 0x{offset:X}"));
        Ok(offset)
    }

    /// Adds the sections that only some bundles have, in order: the release
    /// information, the command line, the splash image and the device tree.
    fn add_optional_sections(
        &mut self,
        sections: &mut Sections,
        dir: &str,
        kernel_version: &str,
        settings: &Settings,
    ) -> Step<()> {
        let os_release = self.context.os_release.display().to_string();
        if is_file(&os_release) {
            self.add_section(sections, ".osrel", &os_release)?;
        }
        if let Some(cmdline) = settings.cmdline.clone().filter(|text| !text.is_empty()) {
            let file = format!("{dir}/cmdline.txt");
            let _ = std::fs::write(&file, cmdline);
            self.add_section(sections, ".cmdline", &file)?;
        }
        if let Some(splash) = self
            .setting(&EFI_SPLASH_IMAGE)?
            .filter(|text| !text.is_empty())
            && is_file(&splash)
        {
            self.add_section(sections, ".splash", &splash)?;
        }
        if let Some(path) = self.device_tree(kernel_version)? {
            self.add_section(sections, ".dtb", &path)?;
        }
        Ok(())
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
