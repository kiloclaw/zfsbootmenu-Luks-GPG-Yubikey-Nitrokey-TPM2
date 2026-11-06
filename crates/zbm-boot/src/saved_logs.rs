//! Logs saved on the EFI system partition (`zbm.save_logs`, ROADMAP.md #9):
//! the runtime journal, the crash reports and the kernel log of a boot,
//! which the memory file system loses at `kexec` or a restart, go to a
//! directory named for the time below `EFI/zfsbootmenu/logs` on the ESP,
//! and the newest five of them stay.
//!
//! The partition is mounted only for the write. It is the one `zbm.save_logs`
//! names (`PARTUUID=...`, `LABEL=...` or a device path), or the one the boot
//! loader loaded ZFSBootMenu from (`LoaderDevicePartUUID`, which systemd's
//! EFI stub sets in ZFSBootMenu's EFI bundles), or else the machine's only
//! ESP.
//!
//! A boot's directory also holds its record, `boot.json` (ADR 0016, point
//! 7): what was booted and how, written last, so that a directory with a
//! record holds the boot's logs whole. The record, and the logs written
//! again before `kexec`, go to a file of their name with `.tmp` after it,
//! renamed once whole: a write that fails, as on a full partition, leaves
//! what was there whole.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use zbm_platform::dmesg::kernel_log;
use zbm_platform::{Context, Files};
use zbm_tools::Shown;
use zbm_tools::devices::Devices;
use zbm_tools::mount::Mount;
use zbm_types::BootRecord;

/// The GPT partition type of an EFI system partition.
pub(crate) const ESP_TYPE: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

/// The EFI variable with the partition the boot loader was loaded from.
pub(crate) const LOADER_DEVICE: &str =
    "/sys/firmware/efi/efivars/LoaderDevicePartUUID-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f";

/// Where on the ESP the logs go, one directory per boot.
pub(crate) const DIRECTORY: &str = "EFI/zfsbootmenu/logs";

/// How many boots' logs stay.
pub(crate) const KEEP: usize = 5;

/// Where the ESP is mounted while the logs are written.
pub const MOUNT_POINT: &str = "/run/zbm-saved-logs";

/// The journal of the runtime, in the state directory.
const JOURNAL: &str = "journal.jsonl";

/// The kernel log, in a saved directory.
const KERNEL_LOG: &str = "kernel.log";

/// The record of the boot, in a saved directory.
pub(crate) const RECORD: &str = "boot.json";

/// The extension added to a file's name while it is written, before it is
/// renamed to its name once whole; a listing leaves such files out.
const PARTIAL: &str = "tmp";

/// Where the logs go.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Destination {
    /// The ESP ZFSBootMenu was loaded from, or the machine's only one.
    Esp,
    /// The partition a tag (`PARTUUID=...`, `LABEL=...`) or a path names.
    Device(String),
}

impl Destination {
    /// The destination `zbm.save_logs` asks for, given its value: `None` when
    /// the option is not given or off.
    pub fn from_option(value: Option<&str>) -> Option<Destination> {
        let value = value?.trim();
        match value.to_ascii_lowercase().as_str() {
            "" | "1" | "on" | "yes" | "esp" => Some(Destination::Esp),
            "0" | "off" | "no" => None,
            _ => Some(Destination::Device(value.to_owned())),
        }
    }
}

/// Why the logs could not be saved or listed.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// The machine has no EFI system partition.
    #[error("no EFI system partition found")]
    NoEsp,
    /// The machine has several, and the boot loader did not say which.
    #[error("{0}: more than one EFI system partition; name one with zbm.save_logs=PARTUUID=...")]
    Ambiguous(String),
    /// The partition named does not exist.
    #[error("{0}: no such partition")]
    NoDevice(String),
    /// The partition cannot be mounted as a FAT file system.
    #[error("unable to mount {0}")]
    Mount(String),
    /// A file cannot be written.
    #[error("unable to write {path}")]
    Write {
        /// The file or directory.
        path: String,
        /// Why.
        #[source]
        source: std::io::Error,
    },
}

impl SaveError {
    /// [`SaveError::Write`] for `path`.
    pub fn write(path: &Path, source: std::io::Error) -> SaveError {
        SaveError::Write {
            path: path.display().to_string(),
            source,
        }
    }
}

zbm_types::error_codes! {
    SaveError, SAVE_CODES;
    NoEsp = 621 => "no EFI system partition was found":
        "zbm.save_logs found no EFI system partition to save the logs on. Name one with \
         zbm.save_logs=PARTUUID=... or LABEL=....",
    Ambiguous = 622 => "more than one EFI system partition":
        "The machine has several EFI system partitions and the boot loader did not say which it \
         booted from. Name one with zbm.save_logs=PARTUUID=....",
    NoDevice = 623 => "the partition for the logs does not exist":
        "The partition zbm.save_logs names was not found. Check its PARTUUID or label.",
    Mount = 624 => "the partition for the logs could not be mounted":
        "The partition zbm.save_logs names could not be mounted as FAT. Check that it is the EFI \
         system partition and that the image has the vfat driver.",
    Write = 625 => "the logs could not be written":
        "A file could not be written on the partition for the logs; it may be full or read-only.",
}

/// What was saved, and where.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Saved {
    /// The partition.
    pub device: String,
    /// The directory on it, such as `EFI/zfsbootmenu/logs/2026-10-24T01-29-15Z`.
    pub directory: String,
    /// The files written.
    pub files: Vec<String>,
}

/// One boot's saved logs: its directory's name and its files with their
/// sizes in bytes.
#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedBoot {
    /// The directory's name, the time of the save.
    pub name: String,
    /// The files and their sizes.
    pub files: Vec<(String, u64)>,
}

/// The time `seconds` after the epoch, in UTC, as a name FAT takes:
/// `2026-10-24T01-29-15Z`.
pub fn stamp(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (hour, minute, second) = (rest / 3_600, rest % 3_600 / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}-{minute:02}-{second:02}Z")
}

/// The partition of `destination`, as a device path.
///
/// # Errors
///
/// When it does not exist, or there is no single ESP to take.
pub(crate) fn device(
    context: &Context<'_>,
    destination: &Destination,
) -> Result<String, SaveError> {
    let devices = Devices::new(context.runner(), context.files());
    match destination {
        Destination::Device(path) if path.starts_with('/') => Ok(path.clone()),
        Destination::Device(tag) => {
            let found = tag
                .split_once('=')
                .map(|(key, value)| devices.find(key, value))
                .unwrap_or_default();
            if found.is_empty() {
                Err(SaveError::NoDevice(tag.clone()))
            } else {
                Ok(found)
            }
        }
        Destination::Esp => {
            if let Some(uuid) = loader_partuuid(context) {
                let found = devices.find("PARTUUID", &uuid);
                if !found.is_empty() {
                    return Ok(found);
                }
            }
            match devices.partitions_of_type(ESP_TYPE).as_slice() {
                [] => Err(SaveError::NoEsp),
                [one] => Ok(one.clone()),
                many => Err(SaveError::Ambiguous(many.join(", "))),
            }
        }
    }
}

/// The partition UUID that `LoaderDevicePartUUID` holds: four bytes of
/// attributes, then the UUID in UTF-16. `efivarfs` is mounted read-only
/// first when it is not mounted.
fn loader_partuuid(context: &Context<'_>) -> Option<String> {
    let efivars = "/sys/firmware/efi/efivars";
    if !context.files().is_dir(Path::new("/sys/firmware/efi")) {
        return None;
    }
    if !zbm_bootenv::is_mount_point(context.files(), efivars) {
        let _ = Mount::new(context.runner(), context.files()).efivarfs(efivars, "ro");
    }
    let bytes = context.files().read(Path::new(LOADER_DEVICE)).ok()?;
    let units: Vec<u16> = bytes
        .get(4..)?
        .chunks_exact(2)
        .filter_map(|pair| pair.first_chunk::<2>().copied().map(u16::from_le_bytes))
        .take_while(|&unit| unit != 0)
        .collect();
    let uuid = String::from_utf16(&units).ok()?.trim().to_ascii_lowercase();
    (!uuid.is_empty()).then_some(uuid)
}

/// Runs `work` with `device` mounted at [`MOUNT_POINT`], and unmounts it
/// after. The unmount is the only flush of what `work` may have written,
/// so a failed one is warned about. A read-only mount a read of the
/// history left is unmounted first: while it stays, the kernel refuses to
/// mount the same partition read-write.
fn with_mounted<T>(
    context: &Context<'_>,
    device: &str,
    work: impl FnOnce(&Path) -> Result<T, SaveError>,
) -> Result<T, SaveError> {
    if zbm_bootenv::is_mount_point(context.files(), READ_MOUNT_POINT) {
        let read = Path::new(READ_MOUNT_POINT);
        let mount = Mount::new(context.runner(), context.files());
        let _ = mount.unmount(read, Shown::Hidden);
    }
    let point = Path::new(MOUNT_POINT);
    let _ = context.files().create_dir_all(point);
    let Ok(mounted) = Mount::new(context.runner(), context.files()).fat(device, point) else {
        let _ = context.files().remove_dir(point);
        return Err(SaveError::Mount(device.to_owned()));
    };
    let result = work(mounted.path());
    if !mounted.quietly().unmount() {
        zbm_trace::warn!(
            "unable to unmount {device}: anything written on it may not be whole";
            %device
        );
    }
    let _ = context.files().remove_dir(point);
    result
}

/// Runs `work` with the partition of `destination` mounted: it gets the
/// device and the mount point.
///
/// # Errors
///
/// When the partition cannot be found or mounted, or `work` fails.
pub fn on_partition<T>(
    context: &Context<'_>,
    destination: &Destination,
    work: impl FnOnce(&str, &Path) -> Result<T, SaveError>,
) -> Result<T, SaveError> {
    let device = device(context, destination)?;
    with_mounted(context, &device, |root| work(&device, root))
}

/// Writes `contents` to `path`, or says which file could not be written.
fn write(files: &dyn Files, path: &Path, contents: &[u8]) -> Result<(), SaveError> {
    files
        .write(path, contents)
        .map_err(|source| SaveError::write(path, source))
}

/// The names of the saved boots in `logs`, oldest first.
fn boots(files: &dyn Files, logs: &Path) -> Vec<String> {
    let mut names: Vec<String> = files
        .read_dir(logs)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with(|c: char| c.is_ascii_digit()))
        .filter(|name| files.is_dir(&logs.join(name)))
        .collect();
    names.sort();
    names
}

/// Removes the saved boot `dir` and its files.
fn remove_boot(files: &dyn Files, dir: &Path) {
    for file in files.read_dir(dir).unwrap_or_default() {
        let _ = files.remove_file(&dir.join(file));
    }
    let _ = files.remove_dir(dir);
}

/// Removes the saved boots of `logs` but the newest [`KEEP`], counting
/// `saved`, the boot just saved, which stays whatever its name: with the
/// clock behind, it sorts before older ones.
fn prune(files: &dyn Files, logs: &Path, saved: Option<&str>) {
    let others: Vec<String> = boots(files, logs)
        .into_iter()
        .filter(|name| Some(name.as_str()) != saved)
        .collect();
    let keep = KEEP.saturating_sub(usize::from(saved.is_some()));
    for name in others.iter().take(others.len().saturating_sub(keep)) {
        remove_boot(files, &logs.join(name));
    }
}

/// The files of the state directory `base` to save: the journal, the
/// crash reports and the TPM's event log of measured boot.
fn state_files(files: &dyn Files, base: &Path) -> Vec<String> {
    let mut names: Vec<String> = files
        .read_dir(base)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| {
            name == JOURNAL
                || name == crate::boot::measured::LOG
                || (name.starts_with("crash-")
                    && std::path::Path::new(name)
                        .extension()
                        .is_some_and(|ext| ext == "txt"))
        })
        .collect();
    names.sort();
    names
}

/// Saves the journal and the crash reports of the state directory `base`
/// and the kernel log to `destination`, in a new directory, with `record`,
/// if given, as `boot.json`, and removes the oldest saved boots but the
/// newest five. The record's time is the save's, which names the directory.
///
/// # Errors
///
/// When the partition cannot be found or mounted, or a file not written.
pub fn save(
    context: &Context<'_>,
    base: &Path,
    destination: &Destination,
    record: Option<&BootRecord>,
) -> Result<Saved, SaveError> {
    let device = device(context, destination)?;
    let seconds = context
        .clock()
        .wall()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let record = record.map(|record| BootRecord {
        time: seconds,
        ..record.clone()
    });
    with_mounted(context, &device, |root| {
        let logs = root.join(DIRECTORY);
        let mut name = stamp(seconds);
        let taken = boots(context.files(), &logs);
        let mut count = 1;
        while taken.contains(&name) {
            count += 1;
            name = format!("{}.{count}", stamp(seconds));
        }
        let dir: PathBuf = logs.join(&name);
        context
            .files()
            .create_dir_all(&dir)
            .map_err(|source| SaveError::write(&dir, source))?;
        let written = write_boot(context, base, &dir, record.as_ref());
        // A boot not saved whole is removed, and the old ones are pruned
        // all the same: a full partition is the likely cause.
        if written.is_err() {
            remove_boot(context.files(), &dir);
        }
        prune(
            context.files(),
            &logs,
            written.is_ok().then_some(name.as_str()),
        );
        let written = written?;
        Ok(Saved {
            device: device.clone(),
            directory: format!("{DIRECTORY}/{name}"),
            files: written,
        })
    })
}

/// Writes the journal and the crash reports of `base`, the kernel log and
/// then `record`, if given, into `dir`; returns the names written.
fn write_boot(
    context: &Context<'_>,
    base: &Path,
    dir: &Path,
    record: Option<&BootRecord>,
) -> Result<Vec<String>, SaveError> {
    let mut written = Vec::new();
    for file in state_files(context.files(), base) {
        if let Ok(contents) = context.files().read(&base.join(&file)) {
            write(context.files(), &dir.join(&file), &contents)?;
            written.push(file);
        }
    }
    write(
        context.files(),
        &dir.join(KERNEL_LOG),
        kernel_log(context.files(), context.runner()).as_bytes(),
    )?;
    written.push(KERNEL_LOG.to_owned());
    if let Some(record) = record {
        write_record(context.files(), dir, record)?;
        written.push(RECORD.to_owned());
    }
    Ok(written)
}

/// Writes `record` into `dir` as [`RECORD`], whole or not at all.
fn write_record(files: &dyn Files, dir: &Path, record: &BootRecord) -> Result<(), SaveError> {
    let mut text = record
        .to_json()
        .map_err(|source| SaveError::write(&dir.join(RECORD), source))?;
    text.push('\n');
    write_whole(files, dir, RECORD, text.as_bytes())
}

/// Writes `contents` into `dir` as `name`: to `name` and [`PARTIAL`] first,
/// renamed once whole, so that no reader finds half a file, and a file of
/// that name written before stays as it was when the write fails.
fn write_whole(
    files: &dyn Files,
    dir: &Path,
    name: &str,
    contents: &[u8],
) -> Result<(), SaveError> {
    let partial = dir.join(format!("{name}.{PARTIAL}"));
    let whole = dir.join(name);
    let written = write(files, &partial, contents).and_then(|()| {
        files
            .rename(&partial, &whole)
            .map_err(|source| SaveError::write(&whole, source))
    });
    if written.is_err() {
        let _ = files.remove_file(&partial);
    }
    written
}

/// Writes the kernel log and the journal of the state directory `base`
/// again into the boot `saved`, with what was logged since the save, such
/// as how the GPUs were handed back right before `kexec`, which comes after
/// the teardown hooks and so after the save. The journal holds the info
/// messages that the kernel log leaves out at the default `loglevel=4`.
/// Each is written whole or not at all, so that the saved boot keeps its
/// logs whole when the partition has no room for the longer ones.
///
/// # Errors
///
/// When the partition cannot be mounted, as when a teardown hook took its
/// device away, the saved boot is gone, or a file not written.
pub(crate) fn update_logs(
    context: &Context<'_>,
    base: &Path,
    saved: &Saved,
) -> Result<(), SaveError> {
    with_mounted(context, &saved.device, |root| {
        let dir = root.join(&saved.directory);
        if !context.files().is_dir(&dir) {
            let gone = std::io::Error::new(std::io::ErrorKind::NotFound, "the saved boot is gone");
            return Err(SaveError::write(&dir, gone));
        }
        if let Ok(journal) = context.files().read(&base.join(JOURNAL)) {
            write_whole(context.files(), &dir, JOURNAL, &journal)?;
        }
        write_whole(
            context.files(),
            &dir,
            KERNEL_LOG,
            kernel_log(context.files(), context.runner()).as_bytes(),
        )
    })
}

/// The boots saved on `destination`, oldest first, and its device.
///
/// # Errors
///
/// When the partition cannot be found or mounted.
pub fn list(
    context: &Context<'_>,
    destination: &Destination,
) -> Result<(String, Vec<SavedBoot>), SaveError> {
    let device = device(context, destination)?;
    with_mounted(context, &device, |root| {
        let logs = root.join(DIRECTORY);
        let saved = boots(context.files(), &logs)
            .into_iter()
            .map(|name| {
                let files = boot_files(context.files(), &logs.join(&name), usize::MAX);
                SavedBoot { name, files }
            })
            .collect();
        Ok((device.clone(), saved))
    })
}

/// The first `most` files of the saved boot `dir`, by name, with their
/// sizes in bytes, but those being written ([`PARTIAL`]). Only the files
/// kept are looked at for their size.
fn boot_files(files: &dyn Files, dir: &Path, most: usize) -> Vec<(String, u64)> {
    let mut names: Vec<String> = files
        .read_dir(dir)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|file| file.into_string().ok())
        .filter(|file| Path::new(file).extension().is_none_or(|ext| ext != PARTIAL))
        .collect();
    names.sort();
    names.truncate(most);
    names
        .into_iter()
        .map(|file| {
            let size = files.metadata(&dir.join(&file)).map_or(0, |m| m.len);
            (file, size)
        })
        .collect()
}

mod history;

pub use history::{FILES_READ, PastBoot, READ_BOOTS, READ_MOUNT_POINT, history};

#[cfg(test)]
mod tests;
