//! Libraries the builders cannot find by themselves: those loaded with
//! `dlopen()`, and glibc's `libgcc_s`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use zbm_tools::host::{gcc_profile, libraries_of, linker_cache};

use super::{Fatal, Host};

/// The library of soname `soname` on the host: the dynamic linker's cache
/// first, then the usual directories.
pub fn find_library(host: &Host, soname: &str) -> Option<PathBuf> {
    let cache = linker_cache(host.runner);
    let cached = cache.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next() == Some(soname)).then(|| words.last().map(PathBuf::from))?
    });
    if cached.is_some() {
        return cached;
    }
    [
        "/usr/lib/*/",
        "/usr/lib64/",
        "/usr/lib/",
        "/lib/*/",
        "/lib64/",
        "/lib/",
    ]
    .iter()
    .flat_map(|dir| host.glob(&format!("{dir}{soname}")))
    .find(|path| host.exists(path))
}

/// The files of `dir` named `libgcc_s.so*` that exist.
fn libgcc_in(host: &Host, dir: &Path) -> Vec<PathBuf> {
    host.glob(&format!("{}/libgcc_s.so*", dir.display()))
        .into_iter()
        .filter(|path| host.exists(path))
        .collect()
}

/// The directories of the dynamic linker's cache `cache` but the system's
/// own (`/lib`, `/lib64`, `/usr/lib`, `/usr/lib64`), in order.
fn cache_dirs(cache: &str) -> BTreeSet<PathBuf> {
    cache
        .lines()
        .filter_map(|line| Some(PathBuf::from(line.split_once(" => ")?.1.trim())))
        .filter_map(|path| path.parent().map(Path::to_owned))
        .filter(|dir| {
            !["/lib", "/lib64", "/usr/lib", "/usr/lib64"]
                .iter()
                .any(|system| dir == Path::new(system))
        })
        .collect()
}

/// `libgcc_s`, which glibc's `pthread_cancel` loads with `dlopen()`, when
/// `zpool` does not link it already (OpenZFS commit 24554082bd93):
/// GCC's current runtime (`gcc-config`), else the system's library
/// directories, else any of GCC's. None on other C libraries, or with
/// `zfsbootmenu_skip_gcc_s`.
///
/// # Errors
///
/// When glibc needs it and the host has none.
pub fn libgcc_s(host: &Host, skip: Option<&str>) -> Result<Vec<PathBuf>, Fatal> {
    if skip.is_some_and(|skip| matches!(skip.to_ascii_lowercase().as_str(), "yes" | "on" | "1")) {
        return Ok(Vec::new());
    }
    let cache = linker_cache(host.runner);
    if !cache.contains("libc.so.6") {
        return Ok(Vec::new());
    }
    let linked = host
        .which("zpool")
        .map(|zpool| libraries_of(host.runner, &zpool))
        .unwrap_or_default();
    if linked.contains("libgcc_s.so") {
        return Ok(Vec::new());
    }
    if let Some((target, version)) = gcc_profile(host.runner)
        .as_deref()
        .and_then(|profile| profile.rsplit_once('-'))
    {
        let found = libgcc_in(host, &Path::new("/usr/lib/gcc").join(target).join(version));
        if !found.is_empty() {
            return Ok(found);
        }
    }
    let base = if linked.contains("/lib64/") {
        "lib64"
    } else {
        "lib"
    };
    let mut dirs = vec![Path::new("/").join(base), Path::new("/usr").join(base)];
    dirs.extend(cache_dirs(&cache));
    let found: Vec<PathBuf> = dirs.iter().flat_map(|dir| libgcc_in(host, dir)).collect();
    if !found.is_empty() {
        return Ok(found);
    }
    let any: Vec<PathBuf> = host
        .glob("/usr/lib/gcc/*/*/libgcc_s.so*")
        .into_iter()
        .filter(|path| host.is_file(path))
        .collect();
    if any.is_empty() {
        return Err("Unable to locate libgcc_s.so".into());
    }
    Ok(any)
}
