//! The kernels of a boot environment: finding them with their initramfs and
//! command line files, recording them in its kernel list, and choosing the
//! one to boot.

use std::path::Path;

use zbm_config::spec::properties::KERNEL;
use zbm_platform::Context;
use zbm_state::State;
use zbm_types::{FilesystemName, KernelSelector, Printable};

use crate::error::BootEnvError;
use crate::kernels::{self, Kernel};
use crate::mounts::{Noise, be_location, mount_zfs};
use crate::zfs::zfs_value;

/// Finds the kernels of `fs` and records them in its kernel list, and copies
/// the command line files next to them into the command line cache.
///
/// # Errors
///
/// [`BootEnvError::Mount`] when it cannot be mounted,
/// [`BootEnvError::NoKernel`] when no kernel can be selected.
pub fn find_kernels(
    context: &Context<'_>,
    state: &State<'_>,
    filesystem: &FilesystemName,
) -> Result<(), BootEnvError> {
    zbm_trace::debug!("fs set to {filesystem}"; %filesystem);
    let fs = &filesystem.to_string();
    let Some(mounted) = mount_zfs(context, state, filesystem, Noise::Hidden) else {
        zbm_trace::error!("unable to mount {fs}"; %fs);
        return Err(BootEnvError::Mount { fs: fs.to_owned() });
    };
    let records = state.kernels(fs);
    let _ = context.files().write(&records, b"");

    clear_kcl_cache(context, state, fs);
    let list = kernel_list(context, state, fs, mounted.path());
    let _ = context.files().write(&records, &list);
    drop(mounted);

    if !list.is_empty() && select_kernel(context, state, filesystem).is_some() {
        return Ok(());
    }
    zbm_trace::error!("failed to find kernels on {fs}"; %fs);
    let _ = context.files().remove_file(&records);
    Err(BootEnvError::NoKernel { fs: fs.to_owned() })
}

/// Starts the command line cache of `fs` afresh: creates its directory and
/// removes the command line files in it.
fn clear_kcl_cache(context: &Context<'_>, state: &State<'_>, fs: &str) {
    be_location(state, fs);
    if let Some(cache) = state.kcl_cache(fs, ".")
        && let Some(dir) = cache.parent()
    {
        let _ = context.files().create_dir_all(dir);
        for name in context.files().read_dir(dir).unwrap_or_default() {
            if name.as_encoded_bytes().ends_with(b".kcl")
                && !name.as_encoded_bytes().starts_with(b".")
            {
                let _ = context.files().remove_file(&dir.join(name));
            }
        }
    }
}

/// The kernel list of `fs` mounted at `mnt`: a line for each kernel that has
/// an initramfs. Copies the command line file of each kernel that has one into
/// the command line cache.
fn kernel_list(context: &Context<'_>, state: &State<'_>, fs: &str, mnt: &Path) -> Vec<u8> {
    let mnt = mnt.to_string_lossy();
    let mut list = Vec::new();
    for found in kernels::walk(context.files(), fs, Path::new(&*mnt)) {
        let kpath = &found.kernel;
        if let Some(initramfs) = &found.initramfs {
            // The legacy line names the mount point twice: its `ipath` is
            // still the full path there.
            let ipath = format!("{mnt}{initramfs}");
            zbm_trace::debug!(
                "found kernel: {mnt}{kpath}, initramfs {mnt}{ipath}";
                %mnt, %kpath, %ipath
            );
            let kernel = Kernel::new(fs, kpath, initramfs);
            list.extend_from_slice(kernel.line().as_bytes());
        } else {
            zbm_trace::debug!("kernel {mnt}{kpath} has no initramfs"; %mnt, %kpath);
        }
        if let Some(kcl) = &found.kcl {
            cache_kernel_kcl(context, state, fs, kpath, Path::new(kcl));
        }
    }
    list
}

/// Copies the command line file `kcl` of the kernel `kpath` of `fs`, as it
/// is opened, into the command line cache: a regular file of at most 64
/// KiB, read without following a link at its last name.
fn cache_kernel_kcl(context: &Context<'_>, state: &State<'_>, fs: &str, kpath: &str, kcl: &Path) {
    be_location(state, fs);
    let Some(cache) = state.kcl_cache(fs, kpath) else {
        return;
    };
    match context.files().read_regular(kcl, kernels::SMALL_LIMIT) {
        Ok(contents) => {
            let _ = context.files().write(&cache, &contents);
        }
        Err(error) => {
            let why = error.to_string();
            zbm_trace::warn!(
                "the command line file of {kernel} in {fs} is not read: {why}";
                kernel = %Printable(kpath), fs = %Printable(fs), why = %Printable(&why)
            );
        }
    }
}

/// The value of `org.zfsbootmenu:kernel` on `fs`, when it is set.
pub fn kernel_property(context: &Context<'_>, fs: &FilesystemName) -> Option<String> {
    zfs_value(context.runner(), KERNEL.name, fs).filter(|value| !value.is_empty() && value != "-")
}

/// The kernels in the kernel list of `fs`, oldest first: its lines whose
/// dataset is `fs`, without their leading spaces and tabs. Each other line
/// is ignored with a warning, and an empty line, which names nothing, is
/// passed over.
pub fn listed_kernels(
    context: &Context<'_>,
    state: &State<'_>,
    filesystem: &FilesystemName,
) -> Vec<Kernel> {
    let fs = &filesystem.to_string();
    let text = context
        .files()
        .read_to_string(&state.kernels(fs))
        .unwrap_or_default();
    let mut kernels = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        if let Some(own) = own_line(line, fs) {
            kernels.push(Kernel::parse(own));
        } else {
            zbm_trace::warn!(
                "the kernel list of {fs} has a line of another dataset, ignored: {line}";
                fs = %Printable(fs), line = %Printable(line)
            );
        }
    }
    kernels
}

/// The kernel list `line` without its leading spaces and tabs, when its
/// dataset is `fs`: the blanks before a name are not part of it, so every
/// reader of the list takes the same lines of `fs`.
fn own_line<'t>(line: &'t str, fs: &str) -> Option<&'t str> {
    let line = line.trim_start_matches([' ', '\t']);
    (Kernel::parse(line).dataset == fs).then_some(line)
}

/// The kernel to boot from the kernel list of `fs`: the last of its lines
/// whose dataset is `fs`, or the last of those `org.zfsbootmenu:kernel`
/// matches. Other lines are ignored.
pub fn select_kernel(
    context: &Context<'_>,
    state: &State<'_>,
    filesystem: &FilesystemName,
) -> Option<Kernel> {
    zbm_trace::debug!("zfsbe set to {filesystem}"; %filesystem);
    let fs = &filesystem.to_string();
    // As the legacy `be_location` logs it; the list is read where it is,
    // and a missing directory reads as a missing list.
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let list = state.kernels(fs);
    let text = context.files().read_to_string(&list).unwrap_or_default();
    if text.is_empty() {
        zbm_trace::error!("kernel list '{list}' missing or empty"; list = %list.display());
        return None;
    }
    let mut chosen = last_own_line(&text, fs);

    let property = zfs_value(context.runner(), KERNEL.name, filesystem).unwrap_or_default();
    if property != "-" {
        zbm_trace::debug!("{name} set to {property}"; name = %KERNEL.name, %property);
        if let Ok(selector) = property.parse::<KernelSelector>() {
            chosen = selected_by(&text, fs, &selector, &property).or(chosen);
        } else {
            // Bash matched nothing with a pattern that is not valid; a
            // back-reference, which it took, is refused here.
            zbm_trace::warn!(
                "org.zfsbootmenu:kernel is not a pattern the runtime takes; the last kernel boots"
            );
        }
    }
    let Some(chosen) = chosen else {
        zbm_trace::error!("failed to identify kexec arguments for {fs}"; %fs);
        return None;
    };
    zbm_trace::debug!("using kexec args: {chosen}"; %chosen);
    Some(Kernel::parse(chosen))
}

/// The last line of `text` whose dataset is `fs`, as it is past its
/// leading blanks (`tail -1` of those lines); each other line is logged, at
/// debug level, as ignored.
fn last_own_line<'t>(text: &'t str, fs: &str) -> Option<&'t str> {
    let mut last = None;
    for line in text.split('\n').filter(|line| !line.is_empty()) {
        if let Some(own) = own_line(line, fs) {
            last = Some(own);
        } else {
            zbm_trace::debug!(
                "a line of another dataset is ignored: {line}";
                line = %Printable(line)
            );
        }
    }
    last
}

/// The last line of `text` whose dataset is `fs` and whose kernel
/// `selector`, the value `property`, matches: each line trimmed of spaces
/// and tabs, as the legacy `read` trims it.
fn selected_by<'t>(
    text: &'t str,
    fs: &str,
    selector: &KernelSelector,
    property: &str,
) -> Option<&'t str> {
    let mut matched = None;
    for line in text.lines().map(|line| line.trim_matches([' ', '\t'])) {
        let Some(line) = own_line(line, fs) else {
            continue;
        };
        let kernel = Kernel::parse(line);
        if selector.matches(&kernel.kernel) {
            zbm_trace::debug!(
                "matched {kernel} to {property}";
                kernel = %kernel.kernel, %property
            );
            matched = Some(line);
        }
    }
    matched
}

#[cfg(test)]
mod tests;
