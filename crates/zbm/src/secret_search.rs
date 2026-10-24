//! `zbm debug secrets HEX...`, only in builds with the `secret-test`
//! feature: searches the machine for secrets, for the VM test of
//! ROADMAP.md #1. Each argument is a secret in hexadecimal, so that the
//! command line, the hook that runs the command and the image hold no copy
//! of it. The search covers:
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

use std::fs::File;
use std::io::Read as _;
use std::os::unix::fs::{FileExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

use crate::{Io, USAGE};

/// The file systems whose files are in memory.
const MEMORY_FILESYSTEMS: [&str; 3] = ["rootfs", "tmpfs", "ramfs"];

/// How much of a mapping is read at once.
const CHUNK: usize = 1 << 20;

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

/// Whether `needle` occurs in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// The indexes of the `secrets` that occur in `bytes`.
fn matches(bytes: &[u8], secrets: &[Vec<u8>]) -> Vec<usize> {
    (0..secrets.len())
        .filter(|&index| contains(bytes, &secrets[index]))
        .collect()
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
/// `mem`, read in chunks that overlap by the longest secret.
fn search_region(mem: &File, start: u64, end: u64, secrets: &[Vec<u8>]) -> Vec<usize> {
    let overlap = secrets
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(1)
        .saturating_sub(1);
    let mut found = Vec::new();
    let mut buffer = vec![0_u8; CHUNK + overlap];
    let mut kept = 0;
    let mut at = start;
    while at < end {
        let want = usize::try_from(end - at).unwrap_or(CHUNK).min(CHUNK);
        let Ok(read) = mem.read_at(&mut buffer[kept..kept + want], at) else {
            break;
        };
        if read == 0 {
            break;
        }
        let filled = kept + read;
        for index in matches(&buffer[..filled], secrets) {
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
fn search_process(proc: &Path, pid: u32, secrets: &[Vec<u8>], findings: &mut Findings) {
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
pub fn search_processes(proc: &Path, except: u32, secrets: &[Vec<u8>], findings: &mut Findings) {
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
pub fn search_files(root: &Path, secrets: &[Vec<u8>], findings: &mut Findings) {
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

/// `zbm debug secrets HEX...`.
pub fn run(args: &[&str], io: &mut Io) -> i32 {
    let secrets: Option<Vec<Vec<u8>>> = args.iter().map(|arg| decode_hex(arg)).collect();
    let Some(secrets) = secrets.filter(|secrets| !secrets.is_empty()) else {
        let _ = writeln!(io.stderr, "Usage: zbm debug secrets HEX...");
        return USAGE;
    };
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
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const SECRET: &[u8] = b"zbm-secret-search-test";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn secrets_are_given_in_hexadecimal() {
        assert_eq!(decode_hex("70770a"), Some(b"pw\n".to_vec()));
        assert_eq!(decode_hex(&hex(SECRET)).as_deref(), Some(SECRET));
        for bad in ["", "7", "zz", "7g"] {
            assert_eq!(decode_hex(bad), None, "{bad}");
        }
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        assert_eq!(run(&["xyz"], &mut io), USAGE);
        assert_eq!(run(&[], &mut io), USAGE);
    }

    #[test]
    fn the_readable_mappings_are_listed() {
        let maps = "\
5600-5700 r--p 00000000 08:01 12 /usr/bin/zbm
5700-5800 ---p 00000000 00:00 0
7ff0-7ff8 rw-p 00000000 00:00 0                          [stack]
7ffa-7ffb r--p 00000000 00:00 0                          [vvar]
bad line
";
        assert_eq!(
            readable_regions(maps),
            [
                (0x5600, 0x5700, "/usr/bin/zbm".to_owned()),
                (0x7ff0, 0x7ff8, "[stack]".to_owned()),
            ]
        );
    }

    #[test]
    fn the_file_systems_in_memory_are_found() {
        let mounts = "rootfs / rootfs rw 0 0\nproc /proc proc rw 0 0\n\
                      tmpfs /run tmpfs rw 0 0\n/dev/vda1 /boot vfat rw 0 0\n\
                      none /a\\040b ramfs rw 0 0\ntmpfs /run tmpfs rw 0 0\n";
        assert_eq!(
            memory_mounts(mounts),
            [
                PathBuf::from("/"),
                PathBuf::from("/a b"),
                PathBuf::from("/run")
            ]
        );
    }

    #[test]
    fn files_holding_a_secret_are_found() {
        let dir = std::env::temp_dir().join(format!("zbm-secret-search-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(
            dir.join("sub/key"),
            [b"x".as_slice(), SECRET, b"y"].concat(),
        )
        .unwrap();
        std::fs::write(dir.join("other"), b"nothing").unwrap();
        let mut findings = Findings::default();

        search_files(&dir, &[SECRET.to_vec()], &mut findings);

        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(findings.files, 2);
        assert_eq!(
            findings.found,
            [(0, format!("file {}", dir.join("sub/key").display()))]
        );
    }

    #[test]
    fn a_process_holding_a_secret_is_found_in_memory_and_environment() {
        let text = String::from_utf8(SECRET.to_vec()).unwrap();
        let mut child = std::process::Command::new("sh")
            .args(["-c", "S=\"$ZBM_TEST_SECRET\"; read -r line"])
            .env("ZBM_TEST_SECRET", &text)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut findings = Findings::default();
        let other = b"zbm-never-anywhere-0451".to_vec();
        let pid = child.id();

        search_process(
            Path::new("/proc"),
            pid,
            &[other, SECRET.to_vec()],
            &mut findings,
        );

        drop(child.stdin.take());
        // Under `cargo test`, the supervisor's tests in the same process may
        // reap the child first.
        let _ = child.wait();
        let places: Vec<&str> = findings
            .found
            .iter()
            .map(|(_, place)| place.as_str())
            .collect();
        assert!(
            findings.found.iter().all(|(index, _)| *index == 1),
            "{places:?}"
        );
        assert!(
            places.iter().any(|place| place.ends_with(" environ")),
            "{places:?}"
        );
        assert!(
            findings.processes == 1 && findings.regions > 0,
            "{findings:?}"
        );
        assert!(
            places.iter().any(|place| place.contains(" memory ")),
            "{places:?}"
        );
    }

    #[test]
    fn the_report_names_each_place_and_the_protection() {
        let findings = Findings {
            processes: 3,
            regions: 40,
            files: 500,
            found: vec![(0, "file /run/x".to_owned())],
        };
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut io = Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        };

        assert_eq!(report(&findings, &mut io), 1);
        assert_eq!(report(&Findings::default(), &mut io), 0);

        let text = String::from_utf8(stdout).unwrap();
        let expected = "found secret 0 in file /run/x\n\
                        searched 3 processes, 40 mappings and 500 files; found 1; \
                        secrets kept as ";
        assert!(text.starts_with(expected), "{text}");
    }
}
