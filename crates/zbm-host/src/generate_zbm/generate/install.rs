//! Placing the built images: the EFI bundle in `EFI.ImageDir` and the kernel
//! and initramfs in `Components.ImageDir`, either as new revisions with old
//! versions pruned, or replacing the previous images and keeping a backup.

use std::collections::BTreeMap;
use std::path::Path;

use zbm_config::spec::generate::{
    COMPONENTS_IMAGE_DIR, COMPONENTS_VERSIONS, EFI_IMAGE_DIR, EFI_VERSIONS,
};
use zbm_platform::filetest::{is_dir, is_file, strerror};

use super::files::{file_copy, make_path};
use super::{Generator, Step, Stop};
use crate::generate_zbm::{images, perl};

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
    /// The image directory followed by the start of the initramfs images'
    /// names ([`initramfs_stem`]).
    stem: String,
}

/// The start of the names of the initramfs images of the kernel prefix
/// `prefix`: `initramfs` for the usual `vmlinuz`, and the prefix's own
/// name after it otherwise (`initramfs-linux-lts` for `vmlinuz-linux-lts`,
/// `initramfs-zbm` for `zbm`), so that the images of two prefixes in one
/// directory do not replace each other.
fn initramfs_stem(prefix: &str) -> String {
    let rest = ["vmlinuz", "vmlinux"]
        .iter()
        .find_map(|kernel| prefix.strip_prefix(kernel))
        .unwrap_or(prefix)
        .trim_start_matches('-');
    if rest.is_empty() {
        "initramfs".to_owned()
    } else {
        format!("initramfs-{rest}")
    }
}

/// The revision `revision`, or the next one whose `names` are all free: an
/// image is never written over in place.
fn free_revision(mut revision: u64, names: impl Fn(u64) -> Vec<String>) -> u64 {
    while names(revision).iter().any(is_file) {
        revision += 1;
    }
    revision
}

/// The names of the installed kernel and initramfs.
struct Installed {
    /// The installed kernel.
    kernel: String,
    /// The installed initramfs.
    initramfs: String,
}

impl Generator<'_, '_, '_> {
    /// Copies `source` to `dest` without keeping file times. A failure is
    /// printed with its reason, then returned.
    fn safe_copy(&mut self, source: &str, dest: &str) -> std::io::Result<()> {
        if self.is_dry_run_of(&format!("copy {source} to {dest}")) {
            return Ok(());
        }
        self.log(&format!("safeCopy called with: {source}, {dest}, 0"));
        file_copy(source, dest).inspect_err(|error| {
            self.print(&format!(
                "Unable to copy {source} to {dest}: {}\n",
                strerror(error)
            ));
        })
    }

    /// Says that `target` could not be created: the copy went to a
    /// temporary file, which is gone, and `target` is as it was.
    fn failed(&mut self, target: &str) -> Stop {
        self.print(&format!("Failed to create {target}\n"));
        Stop::Exit(1)
    }

    /// The image directory of a section, which must be set when the section
    /// is enabled: without it the images would go to `/`.
    fn image_dir(&mut self, spec: &zbm_config::spec::OptionSpec) -> Step<String> {
        if let Some(dir) = self.setting(spec)?.filter(|dir| !dir.is_empty()) {
            return Ok(dir);
        }
        self.print(&format!("ERROR: {} is not set\n", spec.name));
        Err(Stop::Exit(1))
    }

    /// Removes `file` if it exists, printing `message` first, or a confirmation
    /// when there is no message.
    fn verbose_unlink(&mut self, file: &str, message: Option<&str>) {
        if !is_file(file) {
            return;
        }
        if self.is_dry_run_of(&format!("remove {file}")) {
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

    /// The highest revision among `files` (see [`images::max_revision`]),
    /// logged.
    fn max_revision(&mut self, files: Option<&Vec<String>>, suffix: &str) -> u64 {
        let revision = files.map_or(0, |files| images::max_revision(files, suffix));
        self.log(&format!("maxRevision discovered: {revision}"));
        revision
    }

    /// Removes the files that [`images::prune_versions`] selects, logging the
    /// groups it chooses from.
    fn prune(&mut self, groups: &BTreeMap<String, Vec<String>>, current: &str, keep: i64) {
        self.log(&format!("pruneVersions called with: {current}, {keep}"));
        self.log_groups(groups);
        let keep = u64::try_from(keep).unwrap_or(0);
        for file in images::prune_versions(groups, current, keep) {
            self.verbose_unlink(&file, None);
        }
    }

    /// Renames `from` to `to`; a dry run says it.
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        if self.is_dry_run_of(&format!("rename {from} to {to}")) {
            return Ok(());
        }
        std::fs::rename(from, to)
    }

    /// Creates the directory `path` and its parents; a dry run says it.
    fn make_dir(&mut self, path: &str) -> Step<()> {
        if is_dir(path) || self.is_dry_run_of(&format!("create the directory {path}")) {
            return Ok(());
        }
        make_path(path)
    }

    /// Creates an empty temporary file in `dir` from `template`; a dry run
    /// names it by its template.
    fn temp_file(&self, dir: &str, template: &str) -> Step<String> {
        if self.dry_run {
            return Ok(format!("{dir}/{template}"));
        }
        zbm_platform::filetest::create_temp_file(Path::new(dir), template)
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

    /// Places the EFI bundle in `EFI.ImageDir`, as a new revision or replacing the
    /// previous image.
    pub(super) fn install_bundle(&mut self, bundle: &str, prefix: &str, version: &str) -> Step<()> {
        let image_dir = self.image_dir(&EFI_IMAGE_DIR)?;
        let efi_prefix = format!("{image_dir}/{prefix}");
        self.log(&format!("Setting $efi_prefix: {efi_prefix}"));
        let versions = self
            .config
            .option(&EFI_VERSIONS)?
            .and_then(|value| value.integer())
            .unwrap_or(0);
        self.make_dir(&image_dir)?;

        let target;
        if versions > 0 {
            self.log(&format!("EFI.Versions is {versions}"));
            let existing = images::on_disk(&efi_prefix, ".EFI");
            let groups = images::group_versions(&existing, &efi_prefix, ".EFI");
            self.log_groups(&groups);
            let revision = self.max_revision(groups.get(version), ".EFI") + 1;
            let revision = free_revision(revision, |revision| {
                vec![format!("{efi_prefix}-{version}_{revision}.EFI")]
            });
            target = format!("{efi_prefix}-{version}_{revision}.EFI");
            self.log(&format!("Setting $efi_target: {target}"));
            if self.safe_copy(bundle, &target).is_err() {
                return Err(self.failed(&target));
            }
            self.prune(&groups, version, versions);
        } else {
            target = format!("{efi_prefix}.EFI");
            let temp = self.temp_file(&image_dir, "zbm.XXXXXX")?;
            if self.safe_copy(bundle, &temp).is_err() {
                self.verbose_unlink(&temp, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
            let backup = format!("{efi_prefix}-backup.EFI");
            if is_file(&target) && self.rename(&target, &backup).is_ok() {
                self.done(&format!("Created backup {target} -> {backup}\n"));
            }
            if self.rename(&temp, &target).is_err() {
                self.verbose_unlink(&temp, Some(&format!("Failed to create {target}")));
                return Err(Stop::Exit(1));
            }
        }
        self.done(&format!("Created new UEFI image {target}\n"));
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
        let image_dir = self.image_dir(&COMPONENTS_IMAGE_DIR)?;
        let components = Components {
            kernel,
            initramfs,
            prefix: format!("{image_dir}/{prefix}"),
            stem: format!("{image_dir}/{}", initramfs_stem(&perl::basename(prefix))),
            image_dir,
        };
        let versions = self
            .config
            .option(&COMPONENTS_VERSIONS)?
            .and_then(|value| value.integer())
            .unwrap_or(0);
        self.make_dir(&components.image_dir)?;

        let installed = if versions > 0 {
            self.install_component_revision(&components, version, versions)?
        } else {
            self.replace_components(&components)?
        };
        self.done(&format!(
            "Created initramfs image {}\n",
            installed.initramfs
        ));
        self.done(&format!("Created kernel image {}\n", installed.kernel));
        self.warn_renamed_initramfs(&components);
        Ok(())
    }

    /// Fix 23: the initramfs of a kernel prefix other than `vmlinuz` is
    /// named after it ([`initramfs_stem`]). Where images of the old name
    /// are still beside its kernels, says so: `initramfs-X.img` beside the
    /// kernel `PREFIX-X` and no `vmlinuz-X`, whose it would be. They are
    /// neither used nor pruned with the new ones, and a boot loader's
    /// entries that name them must change.
    fn warn_renamed_initramfs(&mut self, components: &Components) {
        let dir = &components.image_dir;
        let old = format!("{dir}/initramfs");
        if components.stem == old {
            return;
        }
        let leftovers: Vec<String> = images::on_disk(&old, ".img")
            .into_iter()
            .filter(|file| {
                let tail = file
                    .strip_prefix(&format!("{old}-"))
                    .and_then(|rest| rest.strip_suffix(".img"))
                    .unwrap_or_default();
                let theirs = ["vmlinuz", "vmlinux"]
                    .iter()
                    .any(|kernel| is_file(format!("{dir}/{kernel}-{tail}")));
                is_file(format!("{}-{tail}", components.prefix)) && !theirs
            })
            .collect();
        if leftovers.is_empty() {
            return;
        }
        let new = components
            .stem
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        self.print(&format!(
            "WARNING: the initramfs of this kernel prefix is now named {new}-*.img; \
             {} image(s) of the old name initramfs-*.img remain in {} and are not pruned. \
             Boot loader entries that name them, such as those of \
             contrib/syslinux-update.sh, must use the new name.\n",
            leftovers.len(),
            components.image_dir
        ));
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
            prefix,
            stem,
            ..
        } = components;
        let existing = images::on_disk(prefix, "");
        let mut groups = images::group_versions(&existing, prefix, "");
        let revision = self.max_revision(groups.get(version), "") + 1;
        let names = |revision: u64| {
            vec![
                format!("{prefix}-{version}_{revision}"),
                format!("{stem}-{version}_{revision}.img"),
            ]
        };
        let revision = free_revision(revision, names);
        let kernel_target = format!("{prefix}-{version}_{revision}");
        let initramfs_target = format!("{stem}-{version}_{revision}.img");
        if self.safe_copy(initramfs, &initramfs_target).is_err() {
            return Err(self.failed(&initramfs_target));
        }
        if self.safe_copy(kernel, &kernel_target).is_err() {
            self.print(&format!("Failed to create {kernel_target}\n"));
            self.verbose_unlink(&initramfs_target, Some(""));
            return Err(Stop::Exit(1));
        }
        self.prune(&groups, version, versions);
        // Each kernel has an initramfs of the same version and revision; prune those
        // too.
        for files in groups.values_mut() {
            for file in files.iter_mut() {
                *file = format!("{}.img", file.replacen(prefix, stem, 1));
            }
        }
        self.prune(&groups, version, versions);
        Ok(Installed {
            kernel: kernel_target,
            initramfs: initramfs_target,
        })
    }

    /// Copies the components to temporary files in the image directory,
    /// the initramfs then the kernel; returns their names. On a failure
    /// neither is left.
    fn copy_to_temps(
        &mut self,
        components: &Components,
        initramfs_target: &str,
        kernel_target: &str,
    ) -> Step<(String, String)> {
        let Components {
            kernel,
            initramfs,
            image_dir,
            ..
        } = components;
        let init_temp = self.temp_file(image_dir, "init.XXXXXX")?;
        if self.safe_copy(initramfs, &init_temp).is_err() {
            self.verbose_unlink(
                &init_temp,
                Some(&format!("Failed to create {initramfs_target}")),
            );
            return Err(Stop::Exit(1));
        }
        let kern_temp = match self.temp_file(image_dir, "kern.XXXXXX") {
            Ok(temp) => temp,
            Err(stop) => {
                // The full copy of the initramfs must not stay on the ESP.
                self.verbose_unlink(&init_temp, Some(""));
                return Err(stop);
            }
        };
        if self.safe_copy(kernel, &kern_temp).is_err() {
            self.verbose_unlink(
                &kern_temp,
                Some(&format!("Failed to create {kernel_target}")),
            );
            self.verbose_unlink(&init_temp, Some(""));
            return Err(Stop::Exit(1));
        }
        Ok((init_temp, kern_temp))
    }

    /// Replaces the previous components through temporary files, keeping the
    /// previous ones as backups.
    fn replace_components(&mut self, components: &Components) -> Step<Installed> {
        let Components { prefix, stem, .. } = components;
        let kernel_target = format!("{prefix}-bootmenu");
        let initramfs_target = format!("{stem}-bootmenu.img");
        let (init_temp, kern_temp) =
            self.copy_to_temps(components, &initramfs_target, &kernel_target)?;
        let kernel_backup = format!("{kernel_target}-backup");
        if is_file(&kernel_target) && self.rename(&kernel_target, &kernel_backup).is_ok() {
            self.done(&format!(
                "Created backup {kernel_target} -> {kernel_backup}\n"
            ));
        }
        let initramfs_backup = format!("{stem}-bootmenu-backup.img");
        if is_file(&initramfs_target) && self.rename(&initramfs_target, &initramfs_backup).is_ok() {
            self.done(&format!(
                "Created backup {initramfs_target} -> {initramfs_backup}\n"
            ));
        }
        if self.rename(&init_temp, &initramfs_target).is_err() {
            self.verbose_unlink(
                &init_temp,
                Some(&format!("Failed to create {initramfs_target}")),
            );
            self.verbose_unlink(&kern_temp, Some(""));
            return Err(Stop::Exit(1));
        }
        if self.rename(&kern_temp, &kernel_target).is_err() {
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

#[cfg(test)]
mod tests {
    use zbm_testkit::TempDir;

    use super::*;

    #[test]
    fn the_initramfs_is_named_after_a_prefix_other_than_vmlinuz() {
        assert_eq!(initramfs_stem("vmlinuz"), "initramfs");
        assert_eq!(initramfs_stem("vmlinux"), "initramfs");
        assert_eq!(initramfs_stem("vmlinuz-linux-lts"), "initramfs-linux-lts");
        assert_eq!(initramfs_stem("zbm"), "initramfs-zbm");
    }

    #[test]
    fn a_revision_in_use_is_skipped() {
        let dir = TempDir::new("generate-zbm-revision");
        let taken = dir.write("k-1_2", "");
        let name = |revision: u64| vec![format!("{}/k-1_{revision}", dir.path().display())];

        assert_eq!(free_revision(1, name), 1);
        assert_eq!(free_revision(2, name), 3);
        assert!(taken.exists());
    }
}
