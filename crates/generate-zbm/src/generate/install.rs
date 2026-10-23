//! Placing the built images: the EFI bundle in `EFI.ImageDir` and the kernel
//! and initramfs in `Components.ImageDir`, either as new revisions with old
//! versions pruned, or replacing the previous images and keeping a backup.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use zbm_config::spec::generate::{
    COMPONENTS_IMAGE_DIR, COMPONENTS_VERSIONS, EFI_IMAGE_DIR, EFI_VERSIONS,
};
use zbm_core::fs::{is_dir, is_file, strerror};

use super::{Generator, Step, Stop};
use crate::glob::glob;
use crate::{images, perl};

/// The kernel and initramfs to install as components, and where.
struct Components<'a> {
    /// The kernel file.
    kernel: &'a str,
    /// The initramfs file.
    initramfs: &'a str,
    /// `Components.ImageDir`.
    image_dir: String,
    /// The image directory followed by the kernel prefix: the start of the
    /// installed kernels' names.
    prefix: String,
}

/// The names of the installed kernel and initramfs.
struct Installed {
    /// The installed kernel.
    kernel: String,
    /// The installed initramfs.
    initramfs: String,
}

impl Generator<'_, '_, '_> {
    /// `safeCopy` without keeping times, as the Perl program always calls it.
    fn safe_copy(&mut self, source: &str, dest: &str) -> bool {
        self.log(&format!("safeCopy called with: {source}, {dest}, 0"));
        match file_copy(source, dest) {
            Ok(()) => true,
            Err(error) => {
                self.print(&format!(
                    "Unable to copy {source} to {dest}: {}\n",
                    strerror(&error)
                ));
                false
            }
        }
    }

    /// `verboseUnlink`: removes a file if it exists, printing `message` first, or a
    /// confirmation when there is no message.
    fn verbose_unlink(&mut self, file: &str, message: Option<&str>) {
        if !is_file(file) {
            return;
        }
        if let Some(message) = message.filter(|message| !message.is_empty()) {
            self.print(&format!("{message}\n"));
        }
        match std::fs::remove_file(file) {
            Ok(()) => {
                if message.is_none() {
                    self.print(&format!("Removed file {file}\n"));
                }
            }
            Err(error) => {
                self.print(&format!(
                    "ERROR: unable to remove {file}: {}\n",
                    strerror(&error)
                ));
            }
        }
    }

    /// `maxRevision`, with its log message.
    fn max_revision(&mut self, files: Option<&Vec<String>>, suffix: &str) -> u64 {
        let revision = files.map_or(0, |files| images::max_revision(files, suffix));
        self.log(&format!("maxRevision discovered: {revision}"));
        revision
    }

    /// `pruneVersions`, with its log messages.
    fn prune(&mut self, groups: &BTreeMap<String, Vec<String>>, current: &str, keep: i64) {
        self.log(&format!("pruneVersions called with: {current}, {keep}"));
        self.log_groups(groups);
        let keep = u64::try_from(keep).unwrap_or(0);
        for file in images::prune_versions(groups, current, keep) {
            self.verbose_unlink(&file, None);
        }
    }

    /// Creates an empty temporary file in `dir` from `template`.
    fn temp_file(&self, dir: &str, template: &str) -> Step<String> {
        zbm_core::fs::create_temp_file(Path::new(dir), template)
            .map(|path| path.display().to_string())
            .map_err(|error| {
                let template = if dir.is_empty() {
                    template.to_owned()
                } else {
                    format!("{dir}/{template}")
                };
                Stop::fatal(
                    format!(
                        "Error in tempfile() using template {template}: \
                         Could not create temp file: {}",
                        strerror(&error)
                    ),
                    &error,
                )
            })
    }

    /// Renames a file, reporting whether it worked.
    fn rename(from: &str, to: &str) -> bool {
        std::fs::rename(from, to).is_ok()
    }

    /// Places the EFI bundle in `EFI.ImageDir`, as a new revision or replacing the
    /// previous image.
    pub(super) fn install_bundle(&mut self, bundle: &str, prefix: &str, version: &str) -> Step<()> {
        let image_dir = self.setting(&EFI_IMAGE_DIR)?.unwrap_or_default();
        let efi_prefix = format!("{image_dir}/{prefix}");
        self.log(&format!("Setting $efi_prefix: {efi_prefix}"));
        let versions = self
            .config
            .option(&EFI_VERSIONS)?
            .map_or(0, |value| value.perl_int());
        make_path(&image_dir)?;

        let target;
        if versions > 0 {
            self.log(&format!("EFI.Versions is {versions}"));
            let existing = glob(&format!("{efi_prefix}-*.EFI"));
            let groups = images::group_versions(&existing, &efi_prefix, ".EFI");
            self.log_groups(&groups);
            let revision = self.max_revision(groups.get(version), ".EFI") + 1;
            target = format!("{efi_prefix}-{version}_{revision}.EFI");
            self.log(&format!("Setting $efi_target: {target}"));
            if !self.safe_copy(bundle, &target) {
                self.verbose_unlink(&target, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
            self.prune(&groups, version, versions);
        } else {
            target = format!("{efi_prefix}.EFI");
            let temp = self.temp_file(&image_dir, "zbm.XXXXXX")?;
            if !self.safe_copy(bundle, &temp) {
                self.verbose_unlink(&temp, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
            let backup = format!("{efi_prefix}-backup.EFI");
            if is_file(&target) && Self::rename(&target, &backup) {
                self.print(&format!("Created backup {target} -> {backup}\n"));
            }
            if !Self::rename(&temp, &target) {
                self.verbose_unlink(&temp, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
        }
        self.print(&format!("Created new UEFI image {target}\n"));
        Ok(())
    }

    /// Places the kernel and the initramfs in `Components.ImageDir`.
    pub(super) fn install_components(
        &mut self,
        kernel: &str,
        initramfs: &str,
        prefix: &str,
        version: &str,
    ) -> Step<()> {
        let image_dir = self.setting(&COMPONENTS_IMAGE_DIR)?.unwrap_or_default();
        let components = Components {
            kernel,
            initramfs,
            prefix: format!("{image_dir}/{prefix}"),
            image_dir,
        };
        let versions = self
            .config
            .option(&COMPONENTS_VERSIONS)?
            .map_or(0, |value| value.perl_int());
        make_path(&components.image_dir)?;

        let installed = if versions > 0 {
            self.install_component_revision(&components, version, versions)?
        } else {
            self.replace_components(&components)?
        };
        self.print(&format!(
            "Created initramfs image {}\n",
            installed.initramfs
        ));
        self.print(&format!("Created kernel image {}\n", installed.kernel));
        Ok(())
    }

    /// Installs the components as the next revision of `version`, then prunes
    /// the kernels and the initramfs images beyond the `versions` to keep.
    fn install_component_revision(
        &mut self,
        components: &Components,
        version: &str,
        versions: i64,
    ) -> Step<Installed> {
        let Components {
            kernel,
            initramfs,
            image_dir,
            prefix,
        } = components;
        let existing = glob(&format!("{prefix}-*"));
        let mut groups = images::group_versions(&existing, prefix, "");
        let revision = self.max_revision(groups.get(version), "") + 1;
        let kernel_target = format!("{prefix}-{version}_{revision}");
        let initramfs_target = format!("{image_dir}/initramfs-{version}_{revision}.img");
        if !self.safe_copy(initramfs, &initramfs_target) {
            self.verbose_unlink(
                &initramfs_target,
                Some(&format!("Failed to create {initramfs_target}")),
            );
            return Err(Stop::Exit(1));
        }
        if !self.safe_copy(kernel, &kernel_target) {
            self.verbose_unlink(
                &kernel_target,
                Some(&format!("Failed to create {kernel_target}")),
            );
            self.verbose_unlink(&initramfs_target, Some(""));
            return Err(Stop::Exit(1));
        }
        self.prune(&groups, version, versions);
        // Each kernel has an initramfs of the same version and revision; prune those
        // too.
        let initramfs_prefix = format!("{image_dir}/initramfs");
        for files in groups.values_mut() {
            for file in files.iter_mut() {
                *file = format!("{}.img", file.replacen(prefix, &initramfs_prefix, 1));
            }
        }
        self.prune(&groups, version, versions);
        Ok(Installed {
            kernel: kernel_target,
            initramfs: initramfs_target,
        })
    }

    /// Replaces the previous components through temporary files, keeping the
    /// previous ones as backups.
    fn replace_components(&mut self, components: &Components) -> Step<Installed> {
        let Components {
            kernel,
            initramfs,
            image_dir,
            prefix,
        } = components;
        let kernel_target = format!("{prefix}-bootmenu");
        let initramfs_target = format!("{image_dir}/initramfs-bootmenu.img");
        let init_temp = self.temp_file(image_dir, "init.XXXXXX")?;
        if !self.safe_copy(initramfs, &init_temp) {
            self.verbose_unlink(
                &init_temp,
                Some(&format!("Failed to create {initramfs_target}")),
            );
            return Err(Stop::Exit(1));
        }
        let kern_temp = self.temp_file(image_dir, "kern.XXXXXX")?;
        if !self.safe_copy(kernel, &kern_temp) {
            self.verbose_unlink(
                &kern_temp,
                Some(&format!("Failed to create {kernel_target}")),
            );
            self.verbose_unlink(&init_temp, Some(""));
            return Err(Stop::Exit(1));
        }
        let kernel_backup = format!("{kernel_target}-backup");
        if is_file(&kernel_target) && Self::rename(&kernel_target, &kernel_backup) {
            self.print(&format!(
                "Created backup {kernel_target} -> {kernel_backup}\n"
            ));
        }
        let initramfs_backup = format!("{image_dir}/initramfs-bootmenu-backup.img");
        if is_file(&initramfs_target) && Self::rename(&initramfs_target, &initramfs_backup) {
            self.print(&format!(
                "Created backup {initramfs_target} -> {initramfs_backup}\n"
            ));
        }
        if !Self::rename(&init_temp, &initramfs_target) {
            self.verbose_unlink(
                &init_temp,
                Some(&format!("Failed to create {initramfs_target}")),
            );
            self.verbose_unlink(&kern_temp, Some(""));
            return Err(Stop::Exit(1));
        }
        if !Self::rename(&kern_temp, &kernel_target) {
            self.verbose_unlink(
                &kern_temp,
                Some(&format!("Failed to create {kernel_target}")),
            );
            return Err(Stop::Exit(1));
        }
        Ok(Installed {
            kernel: kernel_target,
            initramfs: initramfs_target,
        })
    }
}

/// File::Path's `make_path`: creates a directory and its missing parents;
/// failing to create one is fatal.
fn make_path(path: &str) -> Step<()> {
    if path.is_empty() || is_dir(path) {
        return Ok(());
    }
    let parent = perl::dirname(path);
    if !(is_dir(&parent) || parent == path) {
        make_path(&parent)?;
    }
    match std::fs::create_dir(path) {
        Err(error) if !is_dir(path) => Err(Stop::fatal(
            format!("mkdir {path}: {}", strerror(&error)),
            &error,
        )),
        _ => Ok(()),
    }
}

/// File::Copy's `copy`: copies the contents into `dest` (into a file of the
/// same name when `dest` is a directory), creating it with the default
/// permissions or truncating it.
fn file_copy(source: &str, dest: &str) -> std::io::Result<()> {
    let mut dest = PathBuf::from(dest);
    if dest.is_dir() && !Path::new(source).is_dir() {
        dest = dest.join(perl::basename(source));
    }
    let mut from = std::fs::File::open(source)?;
    let mut to = std::fs::File::create(&dest)?;
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let count = std::io::Read::read(&mut from, &mut buffer)?;
        if count == 0 {
            break;
        }
        to.write_all(&buffer[..count])?;
    }
    Ok(())
}
