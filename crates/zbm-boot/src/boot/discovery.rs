//! The kernels of a boot environment: finding them with their initramfs and
//! command line files, recording them in its kernel list, and choosing the
//! one to boot.

use std::path::{Path, PathBuf};

use zbm_config::spec::properties::KERNEL;
use zbm_platform::System;
use zbm_state::State;
use zbm_types::KernelSelector;

use super::mounts::{Noise, be_location, mount_zfs, umount};
use super::zfs::zfs_value;
use crate::error::BootError;
use crate::kernels::{self, Kernel};

/// Finds the kernels of `fs` and records them in its kernel list, and copies
/// the command line files next to them into the command line cache.
///
/// # Errors
///
/// [`BootError::NoFilesystem`] without a boot environment,
/// [`BootError::Mount`] when it cannot be mounted, [`BootError::NoKernel`]
/// when no kernel can be selected.
pub fn find_kernels(system: &System<'_>, state: &State<'_>, fs: &str) -> Result<(), BootError> {
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return Err(BootError::NoFilesystem);
    }
    zbm_trace::debug!("fs set to {fs}");
    let Some(mnt) = mount_zfs(system, state, fs, Noise::Hidden) else {
        zbm_trace::error!("unable to mount {fs}");
        return Err(BootError::Mount { fs: fs.to_owned() });
    };
    let records = state.kernels(fs);
    let _ = system.files.write(&records, b"");

    clear_kcl_cache(system, state, fs);
    let list = kernel_list(system, state, fs, &mnt);
    let _ = system.files.write(&records, &list);
    umount(system.runner, &mnt);

    if !list.is_empty() && select_kernel(system, state, fs).is_some() {
        return Ok(());
    }
    zbm_trace::error!("failed to find kernels on {fs}");
    let _ = system.files.remove_file(&records);
    Err(BootError::NoKernel { fs: fs.to_owned() })
}

/// Starts the command line cache of `fs` afresh: creates its directory and
/// removes the command line files in it.
fn clear_kcl_cache(system: &System<'_>, state: &State<'_>, fs: &str) {
    be_location(state, fs);
    if let Some(cache) = state.kcl_cache(fs, ".")
        && let Some(dir) = cache.parent()
    {
        let _ = system.files.create_dir_all(dir);
        for name in system.files.read_dir(dir).unwrap_or_default() {
            if name.as_encoded_bytes().ends_with(b".kcl")
                && !name.as_encoded_bytes().starts_with(b".")
            {
                let _ = system.files.remove_file(&dir.join(name));
            }
        }
    }
}

/// The kernel list of `fs` mounted at `mnt`: a line for each kernel that has
/// an initramfs. Copies the command line file of each kernel that has one into
/// the command line cache.
fn kernel_list(system: &System<'_>, state: &State<'_>, fs: &str, mnt: &Path) -> Vec<u8> {
    let mnt_text = mnt.to_string_lossy();
    let mut list = Vec::new();
    for path in kernels::kernel_files(system.files, mnt) {
        let kpath = in_environment(&path, &mnt_text);
        if let Some(kernel) = paired_kernel(system, fs, &mnt_text, &path, &kpath) {
            list.extend_from_slice(kernel.line().as_bytes());
        }
        cache_kernel_kcl(system, state, fs, &mnt_text, &kpath);
    }
    list
}

/// `path`, a file of the boot environment mounted at `mnt`, as an absolute
/// path inside the boot environment.
fn in_environment(path: &str, mnt: &str) -> String {
    let rest = path.strip_prefix(mnt).unwrap_or(path);
    format!("/{}", rest.strip_prefix('/').unwrap_or(rest))
}

/// The kernel `kpath` of `fs`, found at `path` below `mnt`, with its
/// initramfs, or `None` when it has none.
fn paired_kernel(
    system: &System<'_>,
    fs: &str,
    mnt: &str,
    path: &str,
    kpath: &str,
) -> Option<Kernel> {
    let Some(initramfs) = kernels::find_initramfs(system.files, Path::new(path)) else {
        zbm_trace::debug!("kernel {mnt}{kpath} has no initramfs");
        return None;
    };
    let initramfs = initramfs.to_string_lossy();
    zbm_trace::debug!("found kernel: {mnt}{kpath}, initramfs {mnt}{initramfs}");
    Some(Kernel {
        dataset: fs.to_owned(),
        kernel: kpath.to_owned(),
        initramfs: in_environment(&initramfs, mnt),
    })
}

/// Copies the command line file of the kernel `kpath` of `fs`, mounted at
/// `mnt`, into the command line cache, if the kernel has one.
fn cache_kernel_kcl(system: &System<'_>, state: &State<'_>, fs: &str, mnt: &str, kpath: &str) {
    let kcl = PathBuf::from(format!("{mnt}/{kpath}.kcl"));
    if system.files.exists(&kcl) {
        be_location(state, fs);
        if let Some(cache) = state.kcl_cache(fs, kpath) {
            let _ = system.files.copy(&kcl, &cache);
        }
    }
}

/// The value of `org.zfsbootmenu:kernel` on `fs`, when it is set.
pub fn kernel_property(system: &System<'_>, fs: &str) -> Option<String> {
    zfs_value(system.runner, KERNEL.name, fs).filter(|value| !value.is_empty() && value != "-")
}

/// The kernel to boot from the kernel list of `fs`: the last one, or the last
/// one `org.zfsbootmenu:kernel` matches.
pub fn select_kernel(system: &System<'_>, state: &State<'_>, fs: &str) -> Option<Kernel> {
    if fs.is_empty() {
        zbm_trace::error!("zfsbe is undefined");
        return None;
    }
    zbm_trace::debug!("zfsbe set to {fs}");
    let list = be_location(state, fs).join("kernels");
    let text = system.files.read_to_string(&list).unwrap_or_default();
    if text.is_empty() {
        zbm_trace::error!("kernel list '{}' missing or empty", list.display());
        return None;
    }
    // tail -1
    let mut chosen = text
        .strip_suffix('\n')
        .unwrap_or(&text)
        .rsplit('\n')
        .next()
        .unwrap_or_default()
        .to_owned();

    let property = zfs_value(system.runner, KERNEL.name, fs).unwrap_or_default();
    if property != "-" {
        zbm_trace::debug!("{} set to {property}", KERNEL.name);
        match property.parse::<KernelSelector>() {
            Ok(selector) => {
                for line in text.lines().map(|line| line.trim_matches([' ', '\t'])) {
                    let kernel = Kernel::parse(line);
                    if selector.matches(&kernel.kernel) {
                        zbm_trace::debug!("matched {} to {property}", kernel.kernel);
                        line.clone_into(&mut chosen);
                    }
                }
            }
            // Bash matched nothing with a pattern that is not valid; a
            // back-reference, which it took, is refused here.
            Err(_) => zbm_trace::warn!(
                "org.zfsbootmenu:kernel is not a pattern the runtime takes; the last kernel boots"
            ),
        }
    }
    if chosen.is_empty() {
        zbm_trace::error!("failed to identify kexec arguments for {fs}");
        return None;
    }
    zbm_trace::debug!("using kexec args: {chosen}");
    Some(Kernel::parse(&chosen))
}
