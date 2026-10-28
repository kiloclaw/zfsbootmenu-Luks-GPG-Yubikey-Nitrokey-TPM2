//! Placing the built images: the EFI bundle in `EFI.ImageDir` and the kernel
//! and initramfs in `Components.ImageDir`, either as new revisions with old
//! versions pruned, or replacing the previous images and keeping a backup.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use zbm_config::spec::generate::{
    COMPONENTS_IMAGE_DIR, COMPONENTS_VERSIONS, EFI_IMAGE_DIR, EFI_VERSIONS,
};
use zbm_platform::filetest::{is_dir, is_file, strerror};

use super::{Generator, Step, Stop};
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
        match self.setting(spec)?.filter(|dir| !dir.is_empty()) {
            Some(dir) => Ok(dir),
            None => {
                self.print(&format!("ERROR: {} is not set\n", spec.name));
                Err(Stop::Exit(1))
            }
        }
    }

    /// Removes `file` if it exists, printing `message` first, or a confirmation
    /// when there is no message.
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

    /// Creates an empty temporary file in `dir` from `template`.
    fn temp_file(&self, dir: &str, template: &str) -> Step<String> {
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
            .map_or(0, |value| value.perl_int());
        make_path(&image_dir)?;

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
            if is_file(&target) && std::fs::rename(&target, &backup).is_ok() {
                self.print(&format!("Created backup {target} -> {backup}\n"));
            }
            if std::fs::rename(&temp, &target).is_err() {
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
        if is_file(&kernel_target) && std::fs::rename(&kernel_target, &kernel_backup).is_ok() {
            self.print(&format!(
                "Created backup {kernel_target} -> {kernel_backup}\n"
            ));
        }
        let initramfs_backup = format!("{stem}-bootmenu-backup.img");
        if is_file(&initramfs_target)
            && std::fs::rename(&initramfs_target, &initramfs_backup).is_ok()
        {
            self.print(&format!(
                "Created backup {initramfs_target} -> {initramfs_backup}\n"
            ));
        }
        if std::fs::rename(&init_temp, &initramfs_target).is_err() {
            self.verbose_unlink(
                &init_temp,
                Some(&format!("Failed to create {initramfs_target}")),
            );
            self.verbose_unlink(&kern_temp, Some(""));
            return Err(Stop::Exit(1));
        }
        if std::fs::rename(&kern_temp, &kernel_target).is_err() {
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
/// same name when `dest` is a directory), with the default permissions.
/// The contents go to a temporary file beside `dest`, which is written to
/// the disk and then renamed: `dest` is either as it was or whole, even
/// after a failed write, a full disk or a power cut. A failure leaves no
/// temporary file.
fn file_copy(source: &str, dest: &str) -> std::io::Result<()> {
    let mut dest = PathBuf::from(dest);
    if dest.is_dir() && !Path::new(source).is_dir() {
        dest = dest.join(perl::basename(source));
    }
    let mut from = std::fs::File::open(source)?;
    let directory = dest
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (temp, to) = copy_target(directory, &dest)?;
    let copied = copy_into(&mut from, to).and_then(|()| std::fs::rename(&temp, &dest));
    if copied.is_err() {
        let _ = std::fs::remove_file(&temp);
        return copied;
    }
    // The rename itself reaches the disk with the directory.
    if let Ok(directory) = std::fs::File::open(directory) {
        let _ = directory.sync_all();
    }
    Ok(())
}

/// A new file in `directory` to copy `dest` into, with the mode of `dest`
/// when it exists, else the mode a new file gets (the default less the
/// umask), as copying into `dest` would give it.
fn copy_target(directory: &Path, dest: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let name = dest
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let mut attempt = 0_u32;
    loop {
        let temp = directory.join(format!(".{name}.{}.{attempt}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => {
                // A file copied over keeps its mode, as `copy` truncating
                // it would.
                if let Ok(existing) = std::fs::metadata(dest) {
                    let _ = file.set_permissions(existing.permissions());
                }
                return Ok((temp, file));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

/// Copies `from` into the file `to`, and writes it to the disk; a failed
/// write, or one the disk reports late, is an error.
fn copy_into(from: &mut std::fs::File, mut to: std::fs::File) -> std::io::Result<()> {
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let count = std::io::Read::read(from, &mut buffer)?;
        if count == 0 {
            break;
        }
        to.write_all(&buffer[..count])?;
    }
    to.sync_all()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

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

    #[test]
    fn a_copy_is_whole_or_leaves_the_target_as_it_was() {
        let dir = TempDir::new("generate-zbm-copy");
        let source = dir.write("source", "new contents");
        let target = dir.write("target", "old");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let (source, target_text) = (source.display().to_string(), target.display().to_string());

        file_copy(&source, &target_text).unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let missing = dir.join("missing").display().to_string();
        assert!(file_copy(&missing, &target_text).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new contents");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names.len(), 2, "no temporary file is left: {names:?}");
        let into = dir.join("into");
        std::fs::create_dir(&into).unwrap();
        file_copy(&source, &into.display().to_string()).unwrap();
        assert_eq!(
            std::fs::read_to_string(into.join("source")).unwrap(),
            "new contents"
        );
    }
}
