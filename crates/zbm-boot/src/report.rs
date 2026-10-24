//! A bug report (`zbm report`, ROADMAP.md #9): what a developer needs to
//! understand a machine's boot, in one directory that the user can attach
//! to an issue. It never holds key material:
//!
//! - `report.txt`: the ZFSBootMenu release, the kernel and ZFS versions, the
//!   pools' status, a summary of each LUKS header (its UUID, and its
//!   tokens' IDs, types and key slots, never their fields, which hold
//!   sealed keys and credentials), whether a TPM is there and locked out,
//!   and the crash reports;
//! - `journal.jsonl`, the runtime journal, whose secret fields are written
//!   as `[redacted]`, and the crash reports;
//! - `kernel.log`, the kernel log.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use zbm_crypt::Cryptsetup;
use zbm_crypt::tpm2::Tpm2;
use zbm_platform::{Files, System};
use zbm_tools::devices::Devices;
use zbm_tools::programs::kernel_log;
use zbm_zfs::{Stderr, ZpoolCommand};

use crate::saved_logs::{self, SaveError, stamp};

/// The TPM device of the resource manager.
const TPM: &str = "/dev/tpmrm0";

/// Where reports go on the EFI system partition.
pub const DIRECTORY: &str = "EFI/zfsbootmenu/reports";

/// A file's first non-empty lines, trimmed, or `unknown`.
fn first_line(files: &dyn Files, path: &str) -> String {
    files
        .read_to_string(Path::new(path))
        .ok()
        .and_then(|text| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

/// The versions: ZFSBootMenu's release, the kernel's and ZFS's.
fn versions(files: &dyn Files, text: &mut String) {
    let _ = writeln!(text, "== versions");
    let release = files
        .read_to_string(Path::new("/etc/zbm-release"))
        .unwrap_or_default();
    for line in release.lines().filter(|line| {
        line.starts_with("VERSION=") || line.starts_with("BUILD_ID=") || line.starts_with("NAME=")
    }) {
        let _ = writeln!(text, "{line}");
    }
    let kernel = first_line(files, "/proc/sys/kernel/osrelease");
    let zfs = first_line(files, "/sys/module/zfs/version");
    let _ = writeln!(text, "kernel: {kernel}\nzfs: {zfs}\n");
}

/// The pools' status (`zpool status -P`).
fn pools(system: &System<'_>, text: &mut String) {
    let _ = writeln!(text, "== pools");
    let status = ZpoolCommand::new(system.runner)
        .text(&["status", "-P"], Stderr::Hidden)
        .map(|output| output.stdout_text())
        .unwrap_or_default();
    let status = if status.trim().is_empty() {
        "no pool is imported".to_owned()
    } else {
        status.trim_end().to_owned()
    };
    let _ = writeln!(text, "{status}\n");
}

/// Each LUKS volume: its UUID and its tokens' IDs, types and key slots.
fn luks(system: &System<'_>, text: &mut String) {
    let _ = writeln!(text, "== LUKS volumes");
    let devices = Devices::new(system.runner).luks_devices();
    let cryptsetup = Cryptsetup::new(system.runner);
    let mut any = false;
    for device in devices.lines().filter(|line| !line.trim().is_empty()) {
        any = true;
        let uuid = cryptsetup
            .luks_uuid(device)
            .unwrap_or_else(|| "unknown".to_owned());
        let tokens: Vec<String> = cryptsetup
            .list_tokens(device)
            .iter()
            .map(|token| {
                let slots = token.keyslots().join(",");
                format!("{} {} (key slots {slots})", token.id, token.kind)
            })
            .collect();
        let tokens = if tokens.is_empty() {
            "no tokens".to_owned()
        } else {
            tokens.join("; ")
        };
        let _ = writeln!(text, "{device}: UUID {uuid}; tokens: {tokens}");
    }
    if !any {
        let _ = writeln!(text, "none");
    }
    let _ = writeln!(text);
}

/// Whether a TPM is there, and whether it is locked out.
fn tpm(system: &System<'_>, text: &mut String) {
    let _ = writeln!(text, "== TPM");
    if system.files.exists(Path::new(TPM)) {
        let tcti = format!("device:{TPM}");
        let locked = Tpm2::new(system.runner, &tcti).in_lockout();
        let lockout = if locked {
            "in lockout"
        } else {
            "not in lockout"
        };
        let _ = writeln!(text, "{TPM}: present, {lockout}\n");
    } else {
        let _ = writeln!(text, "none\n");
    }
}

/// The crash reports of the state directory `base`, by name.
fn crash_reports(files: &dyn Files, base: &Path) -> Vec<String> {
    let mut names: Vec<String> = files
        .read_dir(base)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with("crash-") && name.ends_with(".txt"))
        .collect();
    names.sort();
    names
}

/// The text of `report.txt`, made at `seconds` after the epoch.
pub fn summary(system: &System<'_>, base: &Path, seconds: u64) -> String {
    let mut text = format!("ZFSBootMenu report, {}\n\n", stamp(seconds));
    versions(system.files, &mut text);
    pools(system, &mut text);
    luks(system, &mut text);
    tpm(system, &mut text);
    let _ = writeln!(text, "== crash reports");
    let reports = crash_reports(system.files, base);
    let _ = writeln!(
        text,
        "{}",
        if reports.is_empty() {
            "none".to_owned()
        } else {
            reports.join("\n")
        }
    );
    text
}

/// Writes a report, from the state directory `base`, in a new directory
/// `zbm-report-TIME` below `parent`; returns its path.
///
/// # Errors
///
/// When a file cannot be written.
pub fn write(system: &System<'_>, base: &Path, parent: &Path) -> Result<PathBuf, SaveError> {
    let seconds = system
        .clock
        .wall()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs());
    let dir = parent.join(format!("zbm-report-{}", stamp(seconds)));
    let failed = |path: &Path, error: std::io::Error| {
        SaveError::Write(path.display().to_string(), error.to_string())
    };
    system
        .files
        .create_dir_all(&dir)
        .map_err(|error| failed(&dir, error))?;
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "report.txt".to_owned(),
            summary(system, base, seconds).into_bytes(),
        ),
        (
            "kernel.log".to_owned(),
            kernel_log(system.runner).into_bytes(),
        ),
    ];
    let mut copied = crash_reports(system.files, base);
    copied.push("journal.jsonl".to_owned());
    for name in copied {
        if let Ok(contents) = system.files.read(&base.join(&name)) {
            files.push((name, contents));
        }
    }
    for (name, contents) in files {
        let path = dir.join(name);
        system
            .files
            .write(&path, &contents)
            .map_err(|error| failed(&path, error))?;
    }
    Ok(dir)
}

/// Writes a report on `destination`'s partition, below [`DIRECTORY`];
/// returns the partition and the report's directory on it.
///
/// # Errors
///
/// When the partition cannot be found or mounted, or a file not written.
pub fn write_on(
    system: &System<'_>,
    base: &Path,
    destination: &saved_logs::Destination,
) -> Result<(String, String), SaveError> {
    saved_logs::on_partition(system, destination, |device, root| {
        let dir = write(system, base, &root.join(DIRECTORY))?;
        let name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok((device.to_owned(), format!("{DIRECTORY}/{name}")))
    })
}

#[cfg(test)]
mod tests;
