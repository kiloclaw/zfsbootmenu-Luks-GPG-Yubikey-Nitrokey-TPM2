//! The saved boots read back, for the menu's history (ADR 0016, point 7):
//! the newest of them, each with its files and its record, `boot.json`,
//! when it has one.
//!
//! The partition is mounted read-only, at a mount point of its own
//! ([`READ_MOUNT_POINT`]), so that a read never meets a save, nor `zbm
//! debug saved`, on the same directory. The kernel refuses to mount a
//! partition read-write while it is mounted read-only, so a read-only
//! mount an earlier read left, as when the menu ended during it, is
//! unmounted before the next read, and before a save
//! ([`save`](super::save)). What may be read is bounded, so that a
//! crowded partition cannot hold the menu up: the newest [`READ_BOOTS`]
//! boots, [`FILES_READ`] files of each, the only ones whose size is
//! looked at, and records of [`BootRecord::MAX_LEN`] bytes at most. The
//! ESP is a FAT file system anyone with the disk can write: the names and
//! the records read are the caller's to keep untrusted.
//!
//! The mount may be made by a runner an action cancels; the unmount goes
//! through the runner given for it, which nothing cancels, so that a read
//! cancelled halfway leaves nothing mounted.

use std::path::Path;

use zbm_platform::{Context, Files, Runner};
use zbm_tools::Shown;
use zbm_tools::mount::Mount;
use zbm_types::{BootRecord, RecordError};

use super::{DIRECTORY, Destination, RECORD, SaveError, SavedBoot, boot_files, device};

/// Where the ESP is mounted, read-only, while its saved boots are read.
pub const READ_MOUNT_POINT: &str = "/run/zbm-saved-logs-read";

/// The most saved boots read: the newest.
pub const READ_BOOTS: usize = 16;

/// The most files of a saved boot listed: the first by name.
pub const FILES_READ: usize = 32;

/// The most names, the newest, looked at for the saved boots: a name that
/// is not a directory is passed over, but only so many are.
const NAMES_TRIED: usize = 4 * READ_BOOTS;

/// A saved boot as read back: its directory's name and files, and its
/// record.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PastBoot {
    /// Its directory's name and its files, [`FILES_READ`] at most.
    pub saved: SavedBoot,
    /// Its record: `None` without a `boot.json`, as for a save from the
    /// shell or after a crash, and why it is not read when it is not.
    pub record: Option<Result<BootRecord, RecordError>>,
}

/// The newest saved boots on `destination`, newest first, [`READ_BOOTS`]
/// at most. The partition is mounted read-only by the context's runner,
/// and unmounted by `keep`, which nothing cancels.
///
/// # Errors
///
/// When the partition cannot be found or mounted, or a read-only mount
/// an earlier read left cannot be unmounted.
pub fn history(
    context: &Context<'_>,
    destination: &Destination,
    keep: &dyn Runner,
) -> Result<Vec<PastBoot>, SaveError> {
    let device = device(context, destination)?;
    let files = context.files();
    let point = Path::new(READ_MOUNT_POINT);
    if zbm_bootenv::is_mount_point(files, READ_MOUNT_POINT)
        && !Mount::new(keep, files).unmount(point, Shown::Hidden)
    {
        return Err(SaveError::Mount(device));
    }
    let _ = files.create_dir_all(point);
    let Ok(mounted) = Mount::new(context.runner(), files).fat_read_only(&device, point) else {
        // A mount cancelled while it ran may have mounted all the same.
        if zbm_bootenv::is_mount_point(files, READ_MOUNT_POINT) {
            let _ = Mount::new(keep, files).unmount(point, Shown::Hidden);
        }
        let _ = files.remove_dir(point);
        return Err(SaveError::Mount(device));
    };
    let mounted = mounted.quietly().unmounted_by(keep);
    let read = past_boots(files, &mounted.path().join(DIRECTORY));
    if !mounted.unmount() {
        zbm_trace::warn!("unable to unmount {device} after reading its saved boots"; %device);
    }
    let _ = files.remove_dir(point);
    Ok(read)
}

/// The saved boots of `logs`, newest first, [`READ_BOOTS`] at most.
fn past_boots(files: &dyn Files, logs: &Path) -> Vec<PastBoot> {
    let mut names: Vec<String> = files
        .read_dir(logs)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with(|c: char| c.is_ascii_digit()))
        .collect();
    names.sort_by(|a, b| order(b).cmp(&order(a)));
    names
        .into_iter()
        .take(NAMES_TRIED)
        .filter(|name| files.is_dir(&logs.join(name)))
        .take(READ_BOOTS)
        .map(|name| {
            let dir = logs.join(&name);
            let listed = boot_files(files, &dir, FILES_READ);
            let path = dir.join(RECORD);
            PastBoot {
                saved: SavedBoot {
                    name,
                    files: listed,
                },
                record: files.exists(&path).then(|| record(files, &path)),
            }
        })
        .collect()
}

/// What orders the name of a saved boot: its time, then the number a name
/// already taken was given, so that `.10` comes after `.9`.
fn order(name: &str) -> (&str, u64) {
    match name.split_once('.') {
        Some((stamp, count)) => (stamp, count.parse().unwrap_or(u64::MAX)),
        None => (name, 1),
    }
}

/// The record `path` holds, reading no more of it than a record may be.
fn record(files: &dyn Files, path: &Path) -> Result<BootRecord, RecordError> {
    let too_long = RecordError::TooLong {
        max: BootRecord::MAX_LEN,
    };
    let len = files.metadata(path).map_err(|_| RecordError::Unread)?.len;
    if usize::try_from(len).map_or(true, |len| len > BootRecord::MAX_LEN) {
        return Err(too_long);
    }
    let bytes = files
        .read_prefix(path, BootRecord::MAX_LEN + 1)
        .map_err(|_| RecordError::Unread)?;
    BootRecord::parse(&bytes)
}

#[cfg(test)]
mod tests;
