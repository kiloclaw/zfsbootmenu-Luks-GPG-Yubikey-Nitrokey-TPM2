//! Coverage builds (`cargo llvm-cov`, `--cfg coverage`): a confined
//! process still writes its coverage profile when it exits, where
//! `LLVM_PROFILE_FILE` says, with the calls the profile's writer makes.
//! Other builds have none of this.

use std::path::Path;

use super::{Access, Confinement};

/// The calls of LLVM's profile writer: it opens and locks the file, maps
/// it to merge the counts, and writes.
const PROFILE_CALLS: &[&str] = &[
    "openat",
    "read",
    "write",
    "pread64",
    "pwrite64",
    "lseek",
    "close",
    "fstat",
    "newfstatat",
    "statx",
    "fcntl",
    "flock",
    "ftruncate",
    "mmap",
    "munmap",
    "msync",
    "madvise",
    "mkdir",
    "mkdirat",
    "getpid",
    "uname",
    "fsync",
];

/// `confinement`, with the profile's directory and its writer's calls.
pub(super) fn with_profile(confinement: &Confinement) -> Confinement {
    let mut profiled = confinement.clone();
    let directory = std::env::var_os("LLVM_PROFILE_FILE")
        .and_then(|file| Path::new(&file).parent().map(Path::to_owned));
    if let (Some(files), Some(directory)) = (profiled.files.as_mut(), directory) {
        files.push((directory, Access::Full));
    }
    if let Some(calls) = profiled.calls {
        let mut calls = calls.to_vec();
        calls.extend_from_slice(PROFILE_CALLS);
        profiled.calls = Some(calls.leak());
    }
    profiled
}
