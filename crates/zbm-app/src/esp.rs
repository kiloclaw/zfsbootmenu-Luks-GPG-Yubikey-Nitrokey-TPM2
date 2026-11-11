//! Mounting a partition for the recovery shell, as its `mount_esp` does
//! (ADR 0018, point 10): a block device, or without one the EFI system
//! partition that ZFSBootMenu was loaded from or the machine's only one,
//! mounted read/write at `/mnt/NAME` with the file system's type found by
//! `mount`, and left mounted. A device mounted already is given where it
//! is.

use std::io;
use std::path::{Path, PathBuf};

use zbm_boot::saved_logs::{self, Destination, SaveError};
use zbm_platform::{Context, FileKind, Files, System};
use zbm_tools::mount::Mount;
use zbm_types::DevicePath;
use zbm_zfs::mounts::MountTable;

/// The kernel's table of this process's mounts.
const MOUNTS: &str = "/proc/self/mounts";

/// Why a partition was not mounted.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EspError {
    /// No device was named, and the machine has no EFI system partition.
    #[error("no EFI system partition found")]
    NoEsp,
    /// No device was named, and the machine has several EFI system
    /// partitions, of which the boot loader named none.
    #[error("{0}: more than one EFI system partition; name the one to mount")]
    Ambiguous(String),
    /// The device is missing, or is no block device.
    #[error("{0} does not exist or is not a block device")]
    NotBlockDevice(String),
    /// Something else is mounted on the device's mount point.
    #[error("mount point {} already in use", .0.display())]
    Busy(PathBuf),
    /// The mount point could not be made.
    #[error("unable to make the mount point {}", path.display())]
    Directory {
        /// The mount point.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// `mount` failed, saying why on standard error.
    #[error("unable to mount {device} at {} (mount exited with {status})", point.display())]
    Mount {
        /// The device.
        device: String,
        /// The mount point.
        point: PathBuf,
        /// The exit status of `mount`.
        status: i32,
    },
}

/// Mounts `device` of `system` read/write at `/mnt/NAME`, NAME the last
/// part of its path, and leaves it mounted; without a device, the EFI
/// system partition `zbm.save_logs` takes by default: the one the boot
/// loader names, or the machine's only one. The mount point, or where the
/// device is mounted already.
///
/// # Errors
///
/// When there is no single EFI system partition to take, the device is no
/// block device, its mount point is in use, or it cannot be mounted there.
pub fn mount_esp(system: &System<'_>, device: Option<&DevicePath>) -> Result<PathBuf, EspError> {
    let device = match device {
        Some(device) => device.as_str().to_owned(),
        None => esp(system)?,
    };
    let files = system.files;
    let is_block = files
        .metadata(Path::new(&device))
        .is_ok_and(|metadata| metadata.kind == FileKind::BlockDevice);
    if !is_block {
        return Err(EspError::NotBlockDevice(device));
    }
    // An unreadable table says nothing is mounted.
    let table = files
        .read_to_string(Path::new(MOUNTS))
        .map(|text| MountTable::parse(&text))
        .unwrap_or_default();
    if let Some(point) = mounted_at(files, &table, &device) {
        return Ok(point);
    }
    let name = device.rsplit('/').next().unwrap_or_default();
    let point = format!("/mnt/{name}");
    if table.entries().iter().any(|entry| entry.target == point) {
        return Err(EspError::Busy(point.into()));
    }
    let path = PathBuf::from(point);
    if let Err(source) = files.create_dir_all(&path) {
        return Err(EspError::Directory { path, source });
    }
    match Mount::new(system.runner, files).block(&device, &path) {
        Ok(mounted) => Ok(mounted.leave()),
        Err(status) => Err(EspError::Mount {
            device,
            point: path,
            status,
        }),
    }
}

/// The EFI system partition of `system` that `zbm.save_logs` takes by
/// default.
///
/// # Errors
///
/// When there is none, or several and the boot loader named none.
fn esp(system: &System<'_>) -> Result<String, EspError> {
    saved_logs::device(&Context::new(*system), &Destination::Esp).map_err(|error| match error {
        SaveError::Ambiguous(all) => EspError::Ambiguous(all),
        _ => EspError::NoEsp,
    })
}

/// Where `device` is mounted, as `table` says: the first mount of it by
/// the name given, else by the device a link names, such as
/// `/dev/disk/by-id/...`.
fn mounted_at(files: &dyn Files, table: &MountTable, device: &str) -> Option<PathBuf> {
    if let Some(point) = table.mountpoint_of(device) {
        return Some(point.into());
    }
    let real = files.canonicalize(Path::new(device)).ok()?;
    table
        .entries()
        .iter()
        .find(|entry| {
            entry.source.starts_with('/')
                && files.canonicalize(Path::new(&entry.source)).ok().as_ref() == Some(&real)
        })
        .map(|entry| PathBuf::from(&entry.target))
}

#[cfg(test)]
mod tests;
