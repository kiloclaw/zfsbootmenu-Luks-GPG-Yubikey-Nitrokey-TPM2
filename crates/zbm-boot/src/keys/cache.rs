//! The key-source cache of the legacy runtime: the key file of a boot
//! environment is copied from its key source (`org.zfsbootmenu:keysource`)
//! into `.keys/<source>` in the state directory, after the key of the key
//! source is loaded and while it is mounted.

use std::path::{Path, PathBuf};

use zbm_platform::{Files, Secret, System};
use zbm_state::State;
use zbm_tools::Shown;
use zbm_tools::mount::Mount;
use zbm_zfs::{Stderr, ZfsCommand};

use super::load::{KeyLoad, load_key};
use super::{keysource, stripped};
use crate::boot::{Noise, mount_zfs};

/// `<dir>/<file>`, joined as text as the legacy runtime joins it.
fn below(dir: &Path, file: &str) -> PathBuf {
    PathBuf::from(format!("{}/{file}", dir.display()))
}

/// Whether `path` is below `dir`, compared as text.
fn is_below(path: &Path, dir: &Path) -> bool {
    path.to_string_lossy()
        .starts_with(&format!("{}/", dir.display()))
}

/// Copies the key file at `keylocation` for `fs` from its key source into
/// the cache, logging why when it cannot. A cache attempt that is already
/// running for the source, as when the key source's own key comes from it,
/// is not repeated.
pub(super) fn cache_key(
    system: &System<'_>,
    state: &State<'_>,
    how: &KeyLoad<'_>,
    fs: &str,
    keylocation: &str,
) {
    zbm_trace::debug!("fs set to {fs}");
    zbm_trace::debug!("keylocation set to {keylocation}");
    let keyfile = stripped(keylocation);
    if keyfile == keylocation || keyfile.is_empty() {
        return;
    }
    let Some(source) = keysource(system.runner, fs) else {
        zbm_trace::debug!("no key source found for {fs}");
        return;
    };
    let keycache = state.key_cache(&source);
    let mutex = keycache.join(format!(".cachemutex.{}", system.processes.id()));
    if system.files.exists(&mutex) {
        zbm_trace::debug!("will not repeat cache attempt for {source}:{keyfile}");
        return;
    }
    zbm_trace::debug!("attempting to cache {source}:{keyfile}");
    if system.files.create_dir_all(&keycache).is_err() {
        return;
    }
    if system.files.write(&mutex, b"").is_err() {
        zbm_trace::error!("failed to acquire mutex {}", mutex.display());
        return;
    }
    copy_from_source(system, state, how, &source, keyfile, &keycache);
    let _ = system.files.remove_file(&mutex);
}

/// Loads the key of `source`, mounts it, copies `keyfile` from it into
/// `keycache` and unmounts it.
fn copy_from_source(
    system: &System<'_>,
    state: &State<'_>,
    how: &KeyLoad<'_>,
    source: &str,
    keyfile: &str,
    keycache: &Path,
) {
    if load_key(system, state, how, source).is_err() {
        zbm_trace::warn!("failed to load key for {source}");
        return;
    }
    let Some(mnt) = mount_zfs(system, state, source, Noise::Shown) else {
        zbm_trace::error!("failed to mount {source}");
        return;
    };
    if let Some(keypath) = key_in_source(system, source, keyfile, &mnt) {
        copy_key(system.files, &keypath, keyfile, keycache);
    }
    let _ = Mount::new(system.runner).unmount(&mnt, Shown::Console);
}

/// `keyfile` relative to the mount point of `source` (`ksmount`), or empty
/// when that tells nothing.
fn relative_key(source: &str, keyfile: &str, ksmount: &str) -> String {
    if ksmount == "none" || ksmount == "legacy" {
        zbm_trace::debug!("no discernable mountpoint for {source}, using only absolute key path");
        return String::new();
    }
    let Some(ksmount) = ksmount.strip_prefix('/') else {
        zbm_trace::warn!("ignoring nonsense mountpoint {ksmount} on filesystem {source}");
        return String::new();
    };
    let relative = keyfile.strip_prefix(ksmount).unwrap_or(keyfile);
    let relative = relative.strip_prefix('/').unwrap_or(relative);
    zbm_trace::debug!("{source} mounts at {ksmount}, trying relative path {relative}");
    if relative == keyfile {
        zbm_trace::debug!("relative path  matches absolute, ignoring");
        return String::new();
    }
    relative.to_owned()
}

/// The key file in `source`, mounted at `mnt`: at its path relative to the
/// mount point of `source`, or else at its absolute path below `mnt`.
fn key_in_source(system: &System<'_>, source: &str, keyfile: &str, mnt: &Path) -> Option<PathBuf> {
    let ksmount = ZfsCommand::new(system.runner)
        .text(
            &["get", "-o", "value", "-H", "mountpoint", source],
            Stderr::Hidden,
        )
        .ok()
        .filter(|output| output.status.is_success())
        .map(|output| output.stdout_text().trim_end_matches('\n').to_owned());
    let relative = ksmount.map_or_else(String::new, |m| relative_key(source, keyfile, &m));
    let keypath = if !relative.is_empty() && system.files.exists(&below(mnt, &relative)) {
        zbm_trace::debug!("caching key from mount-relative path {source}:{relative}");
        Some(below(mnt, &relative))
    } else if system.files.exists(&below(mnt, keyfile)) {
        zbm_trace::debug!("caching key from absolute path {source}:{keyfile}");
        Some(below(mnt, keyfile))
    } else {
        zbm_trace::debug!("no key found at {source}:{keyfile}");
        None
    };
    let Some(keypath) = keypath.and_then(|path| system.files.canonicalize(&path).ok()) else {
        zbm_trace::debug!("failed to canonicalize key location {source}:{keyfile}");
        return None;
    };
    if !is_below(&keypath, mnt) {
        zbm_trace::debug!("key file {keyfile} is not a proper child of {source}");
        return None;
    }
    Some(keypath)
}

/// `readlink -f`: the canonical path of `path`, whose last component need
/// not exist.
fn canonical_destination(files: &dyn Files, path: &Path) -> Option<PathBuf> {
    files.canonicalize(path).ok().or_else(|| {
        let parent = files.canonicalize(path.parent()?).ok()?;
        Some(parent.join(path.file_name()?))
    })
}

/// Copies the key at `keypath` to `keyfile` below `keycache`.
fn copy_key(files: &dyn Files, keypath: &Path, keyfile: &str, keycache: &Path) {
    if let Some((keydir, _)) = keyfile.rsplit_once('/')
        && !keydir.is_empty()
    {
        let _ = files.create_dir_all(&below(keycache, keydir));
    }
    let destination = below(keycache, keyfile);
    let Some(keydest) = canonical_destination(files, &destination) else {
        zbm_trace::debug!(
            "failed to canonicalize key destination {}",
            destination.display()
        );
        return;
    };
    if !is_below(&keydest, keycache) {
        zbm_trace::debug!(
            "key destination {} is no a proper child of {}",
            keydest.display(),
            keycache.display()
        );
        return;
    }
    let copied = files
        .read(keypath)
        .map(Secret::from_vec)
        .and_then(|key| files.write(&keydest, key.expose()));
    if copied.is_err() {
        zbm_trace::error!(
            "failed to copy {} to {}",
            keypath.display(),
            destination.display()
        );
        return;
    }
    zbm_trace::debug!(
        "copied key {} to {}",
        keypath.display(),
        destination.display()
    );
}
