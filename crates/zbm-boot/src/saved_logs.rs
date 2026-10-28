//! Logs saved on the EFI system partition (`zbm.save_logs`, ROADMAP.md #9):
//! the runtime journal, the crash reports and the kernel log of a boot,
//! which the memory file system loses at `kexec` or a restart, go to a
//! directory named for the time below [`DIRECTORY`] on the ESP, and the
//! newest [`KEEP`] of them stay.
//!
//! The partition is mounted only for the write. It is the one `zbm.save_logs`
//! names (`PARTUUID=...`, `LABEL=...` or a device path), or the one the boot
//! loader loaded ZFSBootMenu from (`LoaderDevicePartUUID`, which systemd's
//! EFI stub sets in ZFSBootMenu's EFI bundles), or else the machine's only
//! ESP.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use zbm_platform::dmesg::kernel_log;
use zbm_platform::{Files, System};
use zbm_tools::devices::Devices;
use zbm_tools::mount::Mount;

/// The GPT partition type of an EFI system partition.
pub const ESP_TYPE: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";

/// The EFI variable with the partition the boot loader was loaded from.
pub const LOADER_DEVICE: &str =
    "/sys/firmware/efi/efivars/LoaderDevicePartUUID-4a67b082-0a4c-41cf-b6c7-440b29bb8c4f";

/// Where on the ESP the logs go, one directory per boot.
pub const DIRECTORY: &str = "EFI/zfsbootmenu/logs";

/// How many boots' logs stay.
pub const KEEP: usize = 5;

/// Where the ESP is mounted while the logs are written.
pub const MOUNT_POINT: &str = "/run/zbm-saved-logs";

/// The journal of the runtime, in the state directory.
const JOURNAL: &str = "journal.jsonl";

/// The kernel log, in a saved directory.
const KERNEL_LOG: &str = "kernel.log";

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
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
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
    #[error("unable to write {0}: {1}")]
    Write(String, String),
}

/// What was saved, and where.
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
pub fn device(system: &System<'_>, destination: &Destination) -> Result<String, SaveError> {
    let devices = Devices::new(system.runner, system.files);
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
            if let Some(uuid) = loader_partuuid(system) {
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
fn loader_partuuid(system: &System<'_>) -> Option<String> {
    let efivars = "/sys/firmware/efi/efivars";
    if !system.files.is_dir(Path::new("/sys/firmware/efi")) {
        return None;
    }
    if !crate::boot::is_mount_point(system.files, efivars) {
        let _ = Mount::new(system.runner, system.files).efivarfs(efivars, "ro");
    }
    let bytes = system.files.read(Path::new(LOADER_DEVICE)).ok()?;
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
/// after.
fn with_mounted<T>(
    system: &System<'_>,
    device: &str,
    work: impl FnOnce(&Path) -> Result<T, SaveError>,
) -> Result<T, SaveError> {
    let point = Path::new(MOUNT_POINT);
    let _ = system.files.create_dir_all(point);
    let Ok(mounted) = Mount::new(system.runner, system.files).fat(device, point) else {
        let _ = system.files.remove_dir(point);
        return Err(SaveError::Mount(device.to_owned()));
    };
    let result = work(mounted.path());
    let _ = mounted.quietly().unmount();
    let _ = system.files.remove_dir(point);
    result
}

/// Runs `work` with the partition of `destination` mounted: it gets the
/// device and the mount point.
///
/// # Errors
///
/// When the partition cannot be found or mounted, or `work` fails.
pub fn on_partition<T>(
    system: &System<'_>,
    destination: &Destination,
    work: impl FnOnce(&str, &Path) -> Result<T, SaveError>,
) -> Result<T, SaveError> {
    let device = device(system, destination)?;
    with_mounted(system, &device, |root| work(&device, root))
}

/// Writes `contents` to `path`, or says which file could not be written.
fn write(files: &dyn Files, path: &Path, contents: &[u8]) -> Result<(), SaveError> {
    files
        .write(path, contents)
        .map_err(|error| SaveError::Write(path.display().to_string(), error.to_string()))
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

/// The files of the state directory `base` to save: the journal and the
/// crash reports.
fn state_files(files: &dyn Files, base: &Path) -> Vec<String> {
    let mut names: Vec<String> = files
        .read_dir(base)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| {
            name == JOURNAL
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
/// and the kernel log to `destination`, in a new directory, and removes the
/// oldest saved boots but [`KEEP`].
///
/// # Errors
///
/// When the partition cannot be found or mounted, or a file not written.
pub fn save(
    system: &System<'_>,
    base: &Path,
    destination: &Destination,
) -> Result<Saved, SaveError> {
    let device = device(system, destination)?;
    let seconds = system
        .clock
        .wall()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    with_mounted(system, &device, |root| {
        let logs = root.join(DIRECTORY);
        let mut name = stamp(seconds);
        let taken = boots(system.files, &logs);
        let mut count = 1;
        while taken.contains(&name) {
            count += 1;
            name = format!("{}.{count}", stamp(seconds));
        }
        let dir: PathBuf = logs.join(&name);
        system
            .files
            .create_dir_all(&dir)
            .map_err(|error| SaveError::Write(dir.display().to_string(), error.to_string()))?;
        let written = write_boot(system, base, &dir);
        // A boot not saved whole is removed, and the old ones are pruned
        // all the same: a full partition is the likely cause.
        if written.is_err() {
            remove_boot(system.files, &dir);
        }
        prune(
            system.files,
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

/// Writes the journal and the crash reports of `base` and the kernel log
/// into `dir`; returns the names written.
fn write_boot(system: &System<'_>, base: &Path, dir: &Path) -> Result<Vec<String>, SaveError> {
    let mut written = Vec::new();
    for file in state_files(system.files, base) {
        if let Ok(contents) = system.files.read(&base.join(&file)) {
            write(system.files, &dir.join(&file), &contents)?;
            written.push(file);
        }
    }
    write(
        system.files,
        &dir.join(KERNEL_LOG),
        kernel_log(system).as_bytes(),
    )?;
    written.push(KERNEL_LOG.to_owned());
    Ok(written)
}

/// Writes the kernel log and the journal of the state directory `base`
/// again into the boot `saved`, with what was logged since the save, such
/// as how the GPUs were handed back right before `kexec`, which comes after
/// the teardown hooks and so after the save. The journal holds the info
/// messages that the kernel log leaves out at the default `loglevel=4`.
///
/// # Errors
///
/// When the partition cannot be mounted, as when a teardown hook took its
/// device away, or the saved boot is gone.
pub fn update_logs(system: &System<'_>, base: &Path, saved: &Saved) -> Result<(), SaveError> {
    with_mounted(system, &saved.device, |root| {
        let dir = root.join(&saved.directory);
        if !system.files.is_dir(&dir) {
            return Err(SaveError::Write(
                dir.display().to_string(),
                "the saved boot is gone".to_owned(),
            ));
        }
        if let Ok(journal) = system.files.read(&base.join(JOURNAL)) {
            write(system.files, &dir.join(JOURNAL), &journal)?;
        }
        write(
            system.files,
            &dir.join(KERNEL_LOG),
            kernel_log(system).as_bytes(),
        )
    })
}

/// The boots saved on `destination`, oldest first, and its device.
///
/// # Errors
///
/// When the partition cannot be found or mounted.
pub fn list(
    system: &System<'_>,
    destination: &Destination,
) -> Result<(String, Vec<SavedBoot>), SaveError> {
    let device = device(system, destination)?;
    with_mounted(system, &device, |root| {
        let logs = root.join(DIRECTORY);
        let saved = boots(system.files, &logs)
            .into_iter()
            .map(|name| {
                let dir = logs.join(&name);
                let mut files: Vec<(String, u64)> = system
                    .files
                    .read_dir(&dir)
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|file| file.into_string().ok())
                    .map(|file| {
                        let size = system.files.metadata(&dir.join(&file)).map_or(0, |m| m.len);
                        (file, size)
                    })
                    .collect();
                files.sort();
                SavedBoot { name, files }
            })
            .collect();
        Ok((device.clone(), saved))
    })
}

#[cfg(test)]
mod tests;
