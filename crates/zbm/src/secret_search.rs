//! `zbm debug secrets [--cells] HEX...`, only in builds with the
//! `secret-test` feature: searches the machine for secrets, for the VM
//! tests of ROADMAP.md #1 and of the console's scrollback. Each argument is
//! a secret in hexadecimal, so that the command line, the hook that runs
//! the command and the image hold no copy of it. With `--cells`, each is
//! also looked for as the cells of the console's terminal hold text: its
//! characters as 4-byte values, one cell apart. The search covers:
//!
//! - the memory of every other process: each readable mapping of
//!   `/proc/PID/maps`, read through `/proc/PID/mem`;
//! - their command lines and environments;
//! - every regular file on the file systems in memory (`rootfs`, `tmpfs`,
//!   `ramfs`), without crossing into other file systems.
//!
//! It prints one line for each place a secret is found, then a summary, and
//! exits with 1 when it found one. It also prints how this kernel lets
//! secrets be kept ([`zbm_sys::memory::Protection`]).

#![allow(
    clippy::disallowed_methods,
    reason = "a tool of test builds that searches the machine itself: /proc/PID/mem and every \
              file in memory, which the ports do not offer"
)]

use std::fs::File;
use std::io::Read as _;
use std::os::unix::fs::{FileExt, MetadataExt as _};
use std::path::{Path, PathBuf};

use crate::{Io, USAGE};

/// The file systems whose files are in memory, `/dev`'s among them.
const MEMORY_FILESYSTEMS: [&str; 4] = ["rootfs", "tmpfs", "ramfs", "devtmpfs"];

/// How much of a mapping is read at once.
const CHUNK: usize = 1 << 20;

/// How far a page that cannot be read is skipped: the smallest page.
const PAGE: u64 = 4096;

/// Memory read at an offset, as `/proc/PID/mem` is.
trait ReadAt {
    /// Reads into `buffer` from `offset`; how much was read.
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> std::io::Result<usize>;
}

impl ReadAt for File {
    fn read_at(&self, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
        FileExt::read_at(self, buffer, offset)
    }
}

/// What the search went through and found.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct Findings {
    /// The processes searched.
    pub processes: usize,
    /// The mappings read.
    pub regions: usize,
    /// The files read.
    pub files: usize,
    /// Where a secret was found: the index of the secret and the place.
    pub found: Vec<(usize, String)>,
}

/// `text` in hexadecimal as bytes, or `None`.
pub fn decode_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

/// A secret as it is looked for: its bytes one after another, or, with a
/// stride, its characters as 4-byte values that many bytes apart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Needle {
    /// The index of the secret.
    pub secret: usize,
    /// What is looked for: the bytes, or the characters' 4-byte values.
    pub parts: Vec<Vec<u8>>,
    /// How far apart the parts are; `None` for bytes in a row.
    pub stride: Option<usize>,
}

impl Needle {
    /// How many bytes it spans.
    fn span(&self) -> usize {
        match self.stride {
            None => self.parts.first().map_or(0, Vec::len),
            Some(stride) => stride * self.parts.len().saturating_sub(1) + 4,
        }
    }

    /// Whether it occurs in `haystack`.
    fn is_in(&self, haystack: &[u8]) -> bool {
        let span = self.span();
        if span == 0 || haystack.len() < span {
            return false;
        }
        let Some(stride) = self.stride else {
            let bytes = self.parts.first().map_or(&[][..], Vec::as_slice);
            return haystack.windows(span).any(|window| window == bytes);
        };
        (0..=haystack.len() - span).any(|start| {
            self.parts.iter().enumerate().all(|(index, part)| {
                let at = start + index * stride;
                haystack.get(at..at + 4) == Some(part.as_slice())
            })
        })
    }
}

/// What to look for of `secrets`: each in a row, and with `cells`, the
/// size of a cell, as the cells of the console's terminal hold it.
pub fn needles(secrets: &[Vec<u8>], cells: Option<usize>) -> Vec<Needle> {
    let mut needles = Vec::new();
    for (secret, bytes) in secrets.iter().enumerate() {
        needles.push(Needle {
            secret,
            parts: vec![bytes.clone()],
            stride: None,
        });
        if let (Some(stride), Ok(text)) = (cells, std::str::from_utf8(bytes)) {
            needles.push(Needle {
                secret,
                parts: text
                    .chars()
                    .map(|c| u32::from(c).to_le_bytes().to_vec())
                    .collect(),
                stride: Some(stride),
            });
        }
    }
    needles
}

/// The indexes of the secrets of `needles` that occur in `bytes`.
fn matches(bytes: &[u8], needles: &[Needle]) -> Vec<usize> {
    let mut found: Vec<usize> = needles
        .iter()
        .filter(|needle| needle.is_in(bytes))
        .map(|needle| needle.secret)
        .collect();
    found.dedup();
    found
}

/// The readable mappings of a `/proc/PID/maps` text: start, end and name.
pub fn readable_regions(maps: &str) -> Vec<(u64, u64, String)> {
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let range = fields.next()?;
            let perms = fields.next()?;
            let name = fields.nth(3).unwrap_or("").to_owned();
            let (start, end) = range.split_once('-')?;
            let start = u64::from_str_radix(start, 16).ok()?;
            let end = u64::from_str_radix(end, 16).ok()?;
            let readable = perms.starts_with('r');
            // The kernel's pages of these cannot be read through `mem`.
            let special = matches!(name.as_str(), "[vvar]" | "[vvar_vclock]" | "[vsyscall]");
            (readable && !special && end > start).then_some((start, end, name))
        })
        .collect()
}

/// The indexes of the `secrets` in the region from `start` to `end` of
/// `mem`, read in chunks that overlap by the longest secret. A page that
/// cannot be read, such as one of a file mapped beyond its end, is skipped,
/// and the search goes on after it.
fn search_region(mem: &impl ReadAt, start: u64, end: u64, secrets: &[Needle]) -> Vec<usize> {
    let overlap = secrets
        .iter()
        .map(Needle::span)
        .max()
        .unwrap_or(1)
        .saturating_sub(1);
    let mut found = Vec::new();
    let mut buffer = vec![0_u8; CHUNK + overlap];
    let mut kept = 0;
    let mut at = start;
    while at < end {
        let want = usize::try_from(end - at).unwrap_or(CHUNK).min(CHUNK);
        let Some(window) = buffer.get_mut(kept..kept + want) else {
            break;
        };
        let Ok(read) = mem.read_at(window, at) else {
            at = ((at / PAGE + 1) * PAGE).min(end);
            kept = 0;
            continue;
        };
        if read == 0 {
            break;
        }
        let filled = kept + read;
        for index in matches(buffer.get(..filled).unwrap_or_default(), secrets) {
            if !found.contains(&index) {
                found.push(index);
            }
        }
        kept = overlap.min(filled);
        buffer.copy_within(filled - kept..filled, 0);
        at += read as u64;
    }
    found
}

/// Searches the process `pid` below `proc`.
fn search_process(proc: &Path, pid: u32, secrets: &[Needle], findings: &mut Findings) {
    let dir = proc.join(pid.to_string());
    let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
    let name = format!("process {pid} ({})", comm.trim());
    for file in ["cmdline", "environ"] {
        let bytes = std::fs::read(dir.join(file)).unwrap_or_default();
        for index in matches(&bytes, secrets) {
            findings.found.push((index, format!("{name} {file}")));
        }
    }
    let maps = std::fs::read_to_string(dir.join("maps")).unwrap_or_default();
    let Ok(mem) = File::open(dir.join("mem")) else {
        return;
    };
    findings.processes += 1;
    for (start, end, region) in readable_regions(&maps) {
        findings.regions += 1;
        for index in search_region(&mem, start, end, secrets) {
            let place = format!("{name} memory {start:x}-{end:x} {region}");
            findings.found.push((index, place.trim_end().to_owned()));
        }
    }
}

/// Searches every process below `proc` but `except`.
pub fn search_processes(proc: &Path, except: u32, secrets: &[Needle], findings: &mut Findings) {
    let mut pids: Vec<u32> = std::fs::read_dir(proc)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .filter(|pid| *pid != except)
        .collect();
    pids.sort_unstable();
    for pid in pids {
        search_process(proc, pid, secrets, findings);
    }
}

/// The mount points of the file systems in memory, from a `/proc/mounts`
/// text.
pub fn memory_mounts(mounts: &str) -> Vec<PathBuf> {
    let mut points: Vec<PathBuf> = mounts
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let point = fields.nth(1)?;
            let kind = fields.next()?;
            MEMORY_FILESYSTEMS
                .contains(&kind)
                .then(|| PathBuf::from(point.replace("\\040", " ")))
        })
        .collect();
    points.sort();
    points.dedup();
    points
}

/// Searches the regular files below `root` on its file system.
pub fn search_files(root: &Path, secrets: &[Needle], findings: &mut Findings) {
    let Ok(device) = std::fs::symlink_metadata(root).map(|meta| meta.dev()) else {
        return;
    };
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.dev() != device {
                continue;
            }
            if meta.is_dir() {
                pending.push(path);
            } else if meta.is_file() {
                findings.files += 1;
                let mut bytes = Vec::new();
                if File::open(&path)
                    .and_then(|mut file| file.read_to_end(&mut bytes))
                    .is_ok()
                {
                    for index in matches(&bytes, secrets) {
                        findings
                            .found
                            .push((index, format!("file {}", path.display())));
                    }
                }
            }
        }
    }
}

/// `zbm debug secrets [--cells] HEX...`.
pub fn run(args: &[&str], io: &mut Io) -> i32 {
    let (cells, args) = match args {
        ["--cells", rest @ ..] => (Some(zbm_console::CELL_SIZE), rest),
        _ => (None, args),
    };
    let secrets: Option<Vec<Vec<u8>>> = args.iter().map(|arg| decode_hex(arg)).collect();
    let Some(secrets) = secrets.filter(|secrets| !secrets.is_empty()) else {
        let _ = writeln!(io.stderr, "Usage: zbm debug secrets [--cells] HEX...");
        return USAGE;
    };
    let secrets = needles(&secrets, cells);
    let mut findings = Findings::default();
    search_processes(
        Path::new("/proc"),
        std::process::id(),
        &secrets,
        &mut findings,
    );
    let mounts = std::fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    for point in memory_mounts(&mounts) {
        search_files(&point, &secrets, &mut findings);
    }
    report(&findings, io)
}

/// Prints `findings`; returns the exit status.
pub fn report(findings: &Findings, io: &mut Io) -> i32 {
    for (index, place) in &findings.found {
        let _ = writeln!(io.stdout, "found secret {index} in {place}");
    }
    let protection = zbm_sys::memory::Pages::new(1).map(|pages| pages.protection());
    let _ = writeln!(
        io.stdout,
        "searched {} processes, {} mappings and {} files; found {}; secrets kept as {:?}",
        findings.processes,
        findings.regions,
        findings.files,
        findings.found.len(),
        protection.map_or_else(
            |error| format!("unavailable ({error})"),
            |p| format!("{p:?}")
        ),
    );
    i32::from(!findings.found.is_empty())
}

#[cfg(test)]
mod tests;
