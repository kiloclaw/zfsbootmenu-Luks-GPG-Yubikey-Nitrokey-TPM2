//! Helpers for the tests of this crate.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use zbm_system::files::{Files, Metadata, RealFiles};

/// Log stubs for parity tests whose messages span lines: each line gets the
/// label, as each line of a debugging message gets the prefix in the kernel
/// log.
pub const LINE_LOG_STUBS: &str = r#"
_zstub() { local _l; while IFS= read -r _l; do echo "$1: ${_l}" >&2; done <<< "$2"; }
zdebug() { _zstub DEBUG "$*"; }
zinfo() { _zstub INFO "$*"; }
znotice() { _zstub NOTICE "$*"; }
zwarn() { _zstub WARN "$*"; }
zerror() { _zstub ERROR "$*"; }
"#;

/// The kernel log as the legacy log stubs print it: `LABEL: message` lines.
/// Continuation lines of messages other than debugging output are left out,
/// as `log_lines` leaves them out.
pub fn stub_lines(kmsg: &str) -> String {
    kmsg.lines()
        .filter_map(|line| {
            if let Some(debug) = line.strip_prefix("<7>ZBM:") {
                let message = debug.splitn(3, '|').nth(2)?;
                return Some(format!("DEBUG: {message}\n"));
            }
            let (level, message) = line.split_once(">ZFSBootMenu: ")?;
            let label = match level {
                "<3" => "ERROR",
                "<4" => "WARN",
                "<5" => "NOTICE",
                _ => "INFO",
            };
            Some(format!("{label}: {message}\n"))
        })
        .collect()
}

/// Real files, with some absolute paths moved elsewhere.
pub struct Redirect {
    moves: Vec<(PathBuf, PathBuf)>,
}

impl Redirect {
    /// Real files where each `(from, to)` pair moves `from` to `to`.
    pub fn new(moves: &[(&str, &Path)]) -> Redirect {
        Redirect {
            moves: moves
                .iter()
                .map(|(from, to)| (PathBuf::from(from), to.to_path_buf()))
                .collect(),
        }
    }

    fn map<'a>(&'a self, path: &'a Path) -> &'a Path {
        self.moves
            .iter()
            .find(|(from, _)| from == path)
            .map_or(path, |(_, to)| to)
    }
}

impl Files for Redirect {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        RealFiles.read(self.map(path))
    }
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.write(self.map(path), contents)
    }
    fn append(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        RealFiles.append(self.map(path), contents)
    }
    fn create_dir(&self, path: &Path) -> io::Result<()> {
        RealFiles.create_dir(self.map(path))
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        RealFiles.create_dir_all(self.map(path))
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        RealFiles.remove_file(self.map(path))
    }
    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        RealFiles.remove_dir(self.map(path))
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        RealFiles.rename(self.map(from), self.map(to))
    }
    fn read_dir(&self, path: &Path) -> io::Result<Vec<OsString>> {
        RealFiles.read_dir(self.map(path))
    }
    fn metadata(&self, path: &Path) -> io::Result<Metadata> {
        RealFiles.metadata(self.map(path))
    }
    fn set_mode(&self, path: &Path, mode: u32) -> io::Result<()> {
        RealFiles.set_mode(self.map(path), mode)
    }
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        RealFiles.canonicalize(self.map(path))
    }
}
