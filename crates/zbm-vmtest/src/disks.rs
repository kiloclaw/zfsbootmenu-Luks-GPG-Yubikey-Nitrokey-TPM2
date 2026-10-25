//! Disk images: sparse raw disks, partition tables, loop devices over
//! partitions, the payload disk and FAT disks.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::images;
use crate::settings::Settings;

/// `384M`, `2G`, `65536` as bytes.
///
/// # Errors
///
/// [`Error::Setup`] when `size` is not a number with an optional `K`, `M`
/// or `G`.
pub fn bytes(size: &str) -> Result<u64> {
    let (digits, unit) = match size.char_indices().last() {
        Some((index, 'K')) => (&size[..index], 1 << 10),
        Some((index, 'M')) => (&size[..index], 1 << 20),
        Some((index, 'G')) => (&size[..index], 1 << 30),
        _ => (size, 1),
    };
    digits
        .parse::<u64>()
        .map(|count| count * unit)
        .map_err(|_| Error::Setup(format!("bad size {size}")))
}

/// The sparse raw disk `disk<index>.img` of `group`, of `size`, created when
/// missing, or anew when `fresh`.
///
/// # Errors
///
/// [`Error::Io`] when it cannot be created.
pub fn disk(
    settings: &Settings,
    group: &str,
    index: impl std::fmt::Display,
    size: &str,
    fresh: bool,
) -> Result<PathBuf> {
    let dir = settings.disks(group);
    fs::create_dir_all(&dir).map_err(Error::io(format!("creating {}", dir.display())))?;
    let path = dir.join(format!("disk{index}.img"));
    if fresh && path.exists() {
        fs::remove_file(&path).map_err(Error::io(format!("removing {}", path.display())))?;
    }
    if !path.exists() {
        sparse(&path, bytes(size)?)?;
    }
    Ok(path)
}

/// A sparse file of `size` bytes at `path`.
///
/// # Errors
///
/// [`Error::Io`] when it cannot be created.
pub fn sparse(path: &Path, size: u64) -> Result<()> {
    File::create(path)
        .and_then(|file| file.set_len(size))
        .map_err(Error::io(format!("creating {}", path.display())))
}

/// Runs a program on the host; returns its standard output.
///
/// # Errors
///
/// [`Error::Command`] when it cannot run or fails.
pub fn run(command: &mut Command) -> Result<String> {
    let line = format!("{command:?}");
    let output = command.output().map_err(|error| Error::Command {
        command: line.clone(),
        status: "not run".into(),
        output: error.to_string(),
    })?;
    if !output.status.success() {
        return Err(Error::Command {
            command: line,
            status: output.status.to_string(),
            output: String::from_utf8_lossy(&output.stderr).into_owned()
                + &String::from_utf8_lossy(&output.stdout),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Runs a shell command line on the host; returns its standard output.
///
/// # Errors
///
/// [`Error::Command`] when it fails.
pub fn run_shell(line: &str) -> Result<String> {
    run(Command::new("sh").args(["-c", line]))
}

/// Partitions a disk image with `sgdisk`: each part is a name and a size, or
/// `None` for the rest of the disk.
///
/// # Errors
///
/// [`Error::Command`] when `sgdisk` fails.
pub fn partition(path: &Path, parts: &[(&str, Option<&str>)]) -> Result<()> {
    run(Command::new("sgdisk").arg("-Z").arg(path))?;
    for (number, (label, size)) in parts.iter().enumerate() {
        let number = number + 1;
        let end = size.map_or_else(|| "0".to_owned(), |size| format!("+{size}"));
        run(Command::new("sgdisk")
            .arg("-n")
            .arg(format!("{number}:0:{end}"))
            .arg("-c")
            .arg(format!("{number}:{label}"))
            .arg(path))?;
    }
    Ok(())
}

/// Partition `number` of a disk image: its offset and size in bytes, from
/// `sfdisk --json`.
///
/// # Errors
///
/// [`Error::Command`] when `sfdisk` fails, [`Error::Setup`] when it has no
/// such partition.
pub fn partition_extent(path: &Path, number: usize) -> Result<(u64, u64)> {
    let json = run(Command::new("sfdisk").arg("--json").arg(path))?;
    extent_in(&json, number)
        .ok_or_else(|| Error::Setup(format!("{}: no partition {number}", path.display())))
}

/// Partition `number` in the output of `sfdisk --json`.
fn extent_in(json: &str, number: usize) -> Option<(u64, u64)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let table = &value["partitiontable"];
    let sector = table["sectorsize"].as_u64().unwrap_or(512);
    let part = table["partitions"].get(number.checked_sub(1)?)?;
    Some((
        part["start"].as_u64()? * sector,
        part["size"].as_u64()? * sector,
    ))
}

/// A loop device over partition `number` of a disk image, by offset:
/// kernels without GPT support (such as some sandbox kernels) do not scan
/// loop partitions.
///
/// # Errors
///
/// [`Error::Command`] when `sfdisk` or `losetup` fails.
pub fn loop_partition(path: &Path, number: usize) -> Result<String> {
    let (offset, size) = partition_extent(path, number)?;
    let device = run(Command::new("losetup")
        .args(["-f", "--show", "--offset"])
        .arg(offset.to_string())
        .arg("--sizelimit")
        .arg(size.to_string())
        .arg(path))?;
    Ok(device.trim().to_owned())
}

/// The UUID of the LUKS volume on a disk image, or on its partition
/// `number`.
///
/// # Errors
///
/// [`Error::Setup`] when there is no LUKS header there.
pub fn luks_uuid(path: &Path, number: Option<usize>) -> Result<String> {
    let offset = match number {
        Some(number) => partition_extent(path, number)?.0,
        None => 0,
    };
    let mut header = [0_u8; 208];
    File::open(path)
        .and_then(|mut file| {
            file.seek(SeekFrom::Start(offset))?;
            file.read_exact(&mut header)
        })
        .map_err(Error::io(format!("reading {}", path.display())))?;
    uuid_in_header(&header)
        .ok_or_else(|| Error::Setup(format!("no LUKS header on {}", path.display())))
}

/// The UUID in a LUKS header.
fn uuid_in_header(header: &[u8; 208]) -> Option<String> {
    if &header[..6] != b"LUKS\xba\xbe" {
        return None;
    }
    let uuid = &header[168..208];
    let end = uuid
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(uuid.len());
    Some(String::from_utf8_lossy(&uuid[..end]).into_owned())
}

/// The ext4 disk labelled `PAYLOAD` that holds the test boot environment,
/// made again when the boot environment is newer.
///
/// # Errors
///
/// When the boot environment or the file system cannot be built.
pub fn payload_disk(settings: &Settings) -> Result<PathBuf> {
    images::build_be(settings)?;
    let _lock = images::lock(&settings.work, "payload")?;
    let image = settings.work.join("payload.img");
    let stamp = settings.work.join("payload/kver");
    let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified()).ok();
    if modified(&image) < modified(&stamp) {
        fs::remove_file(&image).ok();
        run(Command::new("mkfs.ext4")
            .args(["-q", "-F", "-L", "PAYLOAD", "-d"])
            .arg(settings.work.join("payload"))
            .arg(&image)
            .arg("256M"))?;
    }
    Ok(image)
}

/// A FAT disk image (a file system on the whole disk) at `path`, labelled
/// `label`, holding `files` (path in the file system, contents).
///
/// # Errors
///
/// When `mkfs.vfat` or the mtools fail.
pub fn vfat_disk(
    path: &Path,
    label: &str,
    files: &[(&str, &[u8])],
    size: &str,
    sector: u64,
) -> Result<PathBuf> {
    fs::remove_file(path).ok();
    sparse(path, bytes(size)?)?;
    run(Command::new("mkfs.vfat")
        .args(["-n", label, "-S"])
        .arg(sector.to_string())
        .arg(path))?;
    let source = path.with_extension("file");
    for (name, data) in files {
        fs::write(&source, data).map_err(Error::io("writing a file for the FAT disk"))?;
        let parts: Vec<&str> = name.trim_matches('/').split('/').collect();
        for depth in 1..parts.len() {
            let dir = format!("::/{}", parts[..depth].join("/"));
            // The directory may exist already.
            mtools("mmd").arg("-i").arg(path).arg(dir).output().ok();
        }
        let target = format!("::/{}", name.trim_matches('/'));
        run(mtools("mcopy")
            .args(["-o", "-i"])
            .arg(path)
            .arg(&source)
            .arg(target))?;
    }
    fs::remove_file(&source).ok();
    Ok(path.to_owned())
}

/// An mtools command that does not check the file system's geometry.
fn mtools(program: &str) -> Command {
    let mut command = Command::new(program);
    command.env("MTOOLS_SKIP_CHECK", "1");
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn sizes_take_a_unit() {
        assert_eq!(bytes("384M").unwrap(), 384 << 20);
        assert_eq!(bytes("2G").unwrap(), 2 << 30);
        assert_eq!(bytes("64K").unwrap(), 64 << 10);
        assert_eq!(bytes("512").unwrap(), 512);
        assert!(bytes("M").is_err());
    }

    #[test]
    fn a_partition_is_found_in_the_output_of_sfdisk() {
        let json = r#"{"partitiontable": {"sectorsize": 4096,
            "partitions": [{"start": 256, "size": 32768}, {"start": 33024, "size": 10}]}}"#;

        assert_eq!(extent_in(json, 2), Some((33024 * 4096, 40960)));
        assert_eq!(extent_in(json, 3), None);
        assert_eq!(extent_in(json, 0), None);
    }

    #[test]
    fn the_uuid_is_read_from_a_luks_header() {
        let mut header = [0_u8; 208];
        header[..6].copy_from_slice(b"LUKS\xba\xbe");
        header[168..172].copy_from_slice(b"5d0c");

        assert_eq!(uuid_in_header(&header).as_deref(), Some("5d0c"));
        header[0] = b'X';
        assert_eq!(uuid_in_header(&header), None);
    }

    #[test]
    fn disks_are_sparse_and_kept_unless_fresh() {
        let mut settings = Settings::from_vars(|_| None, false, false);
        settings.work =
            std::env::temp_dir().join(format!("zbm-vmtest-disks-{}", std::process::id()));

        let path = disk(&settings, "g", 3, "1M", false).unwrap();
        fs::write(&path, b"x").unwrap();
        let kept = disk(&settings, "g", 3, "1M", false).unwrap();
        let fresh_len = fs::metadata(disk(&settings, "g", 3, "1M", true).unwrap())
            .unwrap()
            .len();

        assert_eq!(kept, settings.work.join("disks/g/disk3.img"));
        assert_eq!(fresh_len, 1 << 20);
        fs::remove_dir_all(&settings.work).ok();
    }

    #[test]
    fn a_failing_command_reports_its_output() {
        let error = run_shell("echo out; echo err >&2; exit 3").unwrap_err();

        assert!(matches!(&error, Error::Command { output, .. } if output == "err\nout\n"));
        assert_eq!(run_shell("echo ok").unwrap(), "ok\n");
    }
}
