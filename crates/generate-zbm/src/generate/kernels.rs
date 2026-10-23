//! The choice of the kernel: its file, its version and the prefix of the
//! image names, from what the options and the configuration ask for and what
//! the boot directory holds.

use std::collections::BTreeMap;

use zbm_core::fs::{is_file, is_readable};
use zbm_core::shell::quote;

use super::{Generator, Step, Stop};
use crate::glob::glob;
use crate::kernel::{self, Decision};
use crate::perl;
use crate::version::versioncmp;

/// The kernel the options and the configuration ask for; each part may be
/// missing or empty, and is then found from the system.
pub(super) struct KernelRequest {
    /// The kernel file.
    pub(super) path: Option<String>,
    /// The prefix of the image names.
    pub(super) prefix: Option<String>,
    /// The kernel version, which may name the running kernel as `%{current}`.
    pub(super) version: Option<String>,
}

/// The kernel the images are built from.
pub(super) struct Kernel {
    /// The kernel file.
    pub(super) path: String,
    /// Its version.
    pub(super) version: String,
    /// The prefix of the image names.
    pub(super) prefix: String,
}

impl Generator<'_, '_, '_> {
    /// Finds the kernel file, its version and the image prefix for `request`,
    /// looking in `bootdir` for a kernel that is not named.
    pub(super) fn choose_kernel(&mut self, bootdir: &str, request: KernelRequest) -> Step<Kernel> {
        let mut version = self.with_running_version(request.version)?;
        let path = self.kernel_file(bootdir, request.path, &mut version)?;
        let version = self.version_of(&path, version)?;
        let prefix = self.prefix_of(&path, &version, request.prefix)?;
        Ok(Kernel {
            path,
            version,
            prefix,
        })
    }

    /// Replaces `%{current}` in the kernel version with the version of the
    /// running kernel.
    fn with_running_version(&mut self, version: Option<String>) -> Step<Option<String>> {
        let Some(value) = version
            .clone()
            .filter(|text| !text.is_empty() && perl::has_current(text))
        else {
            return Ok(version);
        };
        let (uname, status) = self.execute("uname -r")?;
        if status != 0 || uname.is_empty() {
            self.print("Cannot determine current kernel version\n");
            return Err(Stop::Exit(if status == 0 { 1 } else { status }));
        }
        let running = String::from_utf8_lossy(perl::chomp(&uname[0])).into_owned();
        Ok(Some(perl::replace_current(&value, &running)))
    }

    /// The kernel file: the one named, else one in `bootdir` for the wanted
    /// version, else the latest one there. A kernel found for the version by
    /// glob rather than by its exact name clears `version`, which is then
    /// found from the kernel.
    fn kernel_file(
        &mut self,
        bootdir: &str,
        path: Option<String>,
        version: &mut Option<String>,
    ) -> Step<String> {
        if let Some(kernel) = path.filter(|text| !text.is_empty()) {
            if !is_file(&kernel) {
                self.print(&format!(
                    "The provided kernel {kernel} was not found, unable to continue\n"
                ));
                return Err(Stop::Exit(1));
            }
            return Ok(kernel);
        }
        if let Some(wanted) = version.clone().filter(|text| !text.is_empty()) {
            let Some((kernel, exact)) = versioned_kernel(bootdir, &wanted) else {
                self.print(&format!(
                    "Unable to find file for kernel version {wanted}\n"
                ));
                return Err(Stop::Exit(1));
            };
            // An inexact match leaves the version to be found from the kernel.
            if !exact {
                *version = None;
            }
            return Ok(kernel);
        }
        match self.latest_kernel(bootdir)? {
            Some(kernel) => Ok(kernel),
            None => {
                self.print("Unable to find latest kernel; specify version or path manually\n");
                Err(Stop::Exit(1))
            }
        }
    }

    /// The kernel version: `version` when it is given, else the one found from
    /// the kernel file.
    fn version_of(&mut self, kernel: &str, version: Option<String>) -> Step<String> {
        if let Some(version) = version.filter(|text| !text.is_empty()) {
            return Ok(version);
        }
        match self.kernel_version(kernel)? {
            Some(version) => Ok(version),
            None => {
                self.print(&format!(
                    "Unable to determine kernel version from {kernel}\n"
                ));
                Err(Stop::Exit(1))
            }
        }
    }

    /// The prefix of the image names: `prefix` when it is given, else the
    /// kernel's file name without its version.
    fn prefix_of(&mut self, kernel: &str, version: &str, prefix: Option<String>) -> Step<String> {
        if let Some(prefix) = prefix.filter(|text| !text.is_empty()) {
            return Ok(prefix);
        }
        let prefix = perl::strip_version_suffix(&perl::basename(kernel), version);
        if prefix.is_empty() {
            self.print(&format!(
                "Unable to determine kernel prefix from {kernel}\n"
            ));
            return Err(Stop::Exit(1));
        }
        Ok(prefix)
    }

    /// `latestKernel`: the kernel with the highest version among the first prefix
    /// that has any.
    fn latest_kernel(&mut self, bootdir: &str) -> Step<Option<String>> {
        for prefix in ["vmlinuz*", "vmlinux*", "linux*", "kernel*"] {
            let mut kernels: BTreeMap<String, String> = BTreeMap::new();
            for kernel in glob(&format!("{bootdir}/{prefix}")) {
                let Some(version) = self.kernel_version(&kernel)? else {
                    continue;
                };
                self.log(&format!("Identified version {version} for kernel {kernel}"));
                kernels.insert(version, kernel);
            }
            let mut versions: Vec<&String> = kernels.keys().collect();
            versions.sort_by(|a, b| versioncmp(b, a));
            if let Some(latest) = versions.first() {
                self.log(&format!("Latest kernel: {latest}"));
                return Ok(Some(kernels[*latest].clone()));
            }
        }
        Ok(None)
    }

    /// `kernelVersion`: the version from the file name and from the kernel's
    /// strings.
    fn kernel_version(&mut self, kernel: &str) -> Step<Option<String>> {
        if !is_readable(kernel) {
            self.log(&format!(
                "Unable to read path {kernel}, assuming no version"
            ));
            return Ok(None);
        }
        let from_name = kernel::version_from_name(&perl::basename(kernel));
        let (lines, status) = self.execute(&format!("strings {}", quote(kernel)))?;
        let lines = if status == 0 { lines } else { Vec::new() };
        let decision = kernel::decide(from_name.as_deref(), lines.iter().map(Vec::as_slice));
        Ok(match decision {
            Decision::None => None,
            Decision::FromName(version) => {
                self.log(&format!(
                    "No version found in kernel strings, using {version} from path {kernel}"
                ));
                Some(version)
            }
            Decision::FromStrings(version) => {
                self.log(&format!(
                    "No version found in path {kernel}, using {version} from kernel strings"
                ));
                Some(version)
            }
            Decision::Agreed(version) => Some(version),
            Decision::Inconsistent { name, strings } => {
                self.print(&format!(
                    "WARNING: ignoring inconsistent versions in kernel {kernel}:\n  \
                     Path suggests version {name}.\n  Kernel strings suggest version {strings}.\n  \
                     To use this kernel, explicitly specify the path and version.\n"
                ));
                None
            }
        })
    }
}

/// `versionedKernel`: a kernel in the boot directory for a version, exactly or
/// by glob.
fn versioned_kernel(bootdir: &str, version: &str) -> Option<(String, bool)> {
    for prefix in ["vmlinuz", "linux", "vmlinux", "kernel"] {
        let pattern = format!("{bootdir}/{prefix}-{version}");
        if is_file(&pattern) {
            return Some((pattern, true));
        }
        let mut kernels = glob(&pattern);
        kernels.sort_by(|a, b| versioncmp(a, b));
        if let Some(kernel) = kernels.into_iter().find(|kernel| is_file(kernel)) {
            return Some((kernel, false));
        }
    }
    None
}
