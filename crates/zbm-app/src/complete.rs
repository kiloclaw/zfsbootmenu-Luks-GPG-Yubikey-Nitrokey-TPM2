//! What Tab completes in the recovery shell (ADR 0018, point 23): for each
//! [`Completion`], the names the command takes, read from the machine as
//! the legacy completions read them, but kernels, which they listed by
//! mounting a boot environment. Nothing here mounts or imports anything.

use zbm_platform::System;
use zbm_platform::shell::is_plain;
use zbm_tools::devices::Devices;
use zbm_types::{Completion, DevicePath};
use zbm_zfs::{Zfs as _, ZfsCli};

use crate::{esp, read};

/// The ways `efivarfs` mounts, as the library takes them.
const EFIVARFS_MODES: [&str; 2] = ["ro", "rw"];

/// The names of `kind` on the machine of `system`, in the order the legacy
/// completion listed them, each written as one shell word, which Tab puts
/// on the line as it is: a character the shell reads something in, such
/// as a space, behind a backslash. Those that start with `word`, the word
/// as typed, are listed; none where they cannot be read. A name with a
/// control character, which no shell word can hold as typed, is left out.
pub fn list(system: &System<'_>, kind: Completion, word: &str) -> Vec<String> {
    let names = match kind {
        Completion::Filesystems => filesystems(system),
        Completion::FilesystemsAndSnapshots => filesystems_and_snapshots(system),
        Completion::BootEnvironments => boot_environments(system),
        Completion::ReadOnlyPools => pools(system, false),
        Completion::WritablePools => pools(system, true),
        Completion::EspPartitions => esp_partitions(system),
        Completion::EfivarfsModes => EFIVARFS_MODES.map(str::to_owned).to_vec(),
    };
    names
        .iter()
        .filter(|name| !name.chars().any(char::is_control))
        .map(|name| shell_word(name))
        .filter(|name| name.starts_with(word))
        .collect()
}

/// `name` as one shell word: each character the shell reads something in
/// behind a backslash, as `my\ be`.
fn shell_word(name: &str) -> String {
    let mut word = String::with_capacity(name.len());
    for c in name.chars() {
        if !is_plain(c) {
            word.push('\\');
        }
        word.push(c);
    }
    word
}

/// Every filesystem, without volumes.
fn filesystems(system: &System<'_>) -> Vec<String> {
    ZfsCli::new(system.runner)
        .filesystems(None, &[])
        .unwrap_or_default()
        .iter()
        .map(|dataset| dataset.name.to_string())
        .collect()
}

/// Every snapshot, then every filesystem.
fn filesystems_and_snapshots(system: &System<'_>) -> Vec<String> {
    let zfs = ZfsCli::new(system.runner);
    let filesystems = zfs.filesystems(None, &[]).unwrap_or_default();
    // The snapshots of each pool, below its root filesystem.
    let roots = filesystems
        .iter()
        .filter(|dataset| !dataset.name.as_str().contains('/'));
    let snapshots = roots.flat_map(|root| zfs.snapshots(&root.name).unwrap_or_default());
    snapshots
        .map(|snapshot| snapshot.name.to_string())
        .chain(filesystems.iter().map(|dataset| dataset.name.to_string()))
        .collect()
}

/// The boot environments, as the menu lists them.
fn boot_environments(system: &System<'_>) -> Vec<String> {
    read::environment_names(&ZfsCli::new(system.runner))
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// The imported pools that are imported read/write (`writable`), or
/// read-only.
fn pools(system: &System<'_>, writable: bool) -> Vec<String> {
    ZfsCli::new(system.runner)
        .pool_names()
        .unwrap_or_default()
        .into_iter()
        .filter(|pool| zbm_pool::writable::is_writable(system.runner, pool) == writable)
        .map(|pool| pool.to_string())
        .collect()
}

/// The EFI system partitions that are not mounted, as `mount_esp` finds
/// them, each a device's path.
fn esp_partitions(system: &System<'_>) -> Vec<String> {
    let files = system.files;
    let table = esp::mount_table(files);
    Devices::new(system.runner, files)
        .efi_system_partitions()
        .into_iter()
        .filter(|device| device.parse::<DevicePath>().is_ok())
        .filter(|device| esp::mounted_at(files, &table, device).is_none())
        .collect()
}

#[cfg(test)]
mod tests;
