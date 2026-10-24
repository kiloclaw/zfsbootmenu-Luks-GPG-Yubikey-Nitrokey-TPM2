//! Where the key of an encryption root comes from, in the order the legacy
//! runtime looks for it, decided without loading anything:
//!
//! 1. `keylocation=prompt`: the passphrase prompt;
//! 2. a `file://` key that exists in ZFSBootMenu's own file system: that file;
//! 3. a key source (`org.zfsbootmenu:keysource`) that names an encrypted
//!    filesystem: the key file copied from it into `/zfsbootmenu/.keys`, for
//!    a boot chosen in the menu; the countdown's boot skips key sources;
//! 4. a `passphrase` key: the passphrase prompt;
//! 5. otherwise nothing ZFSBootMenu can load it with.
//!
//! The legacy runtime runs the `load-key.d` hooks first, which may unlock
//! the root in their own way; this decision leaves them out.
//!
//! Shadow mode compares this decision with what the legacy runtime did
//! (`crate::shadow`), and the loading in `load` follows the same order.

use std::fmt;
use std::path::Path;

use zbm_config::spec::properties::KEYSOURCE;
use zbm_platform::{Files, Runner};
use zbm_zfs::{Stderr, ZfsCommand};

use crate::boot::{encryption_root, zfs_value};

mod cache;
mod load;

pub use load::{Clear, KeyLoad, load_key};

/// How the boot was chosen, which decides whether key sources are tried.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BootPath {
    /// The countdown ran out and booted the default boot environment; key
    /// sources are not tried.
    #[default]
    Countdown,
    /// The boot environment was chosen in the menu; key sources are tried.
    Menu,
}

/// Where the key of an encryption root comes from.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeySource {
    /// The passphrase prompt.
    Prompt,
    /// A key file in ZFSBootMenu's own file system.
    File(String),
    /// A key file copied from this key source filesystem.
    Keysource(String),
    /// Nothing ZFSBootMenu can load the key with.
    Nothing,
}

impl fmt::Display for KeySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeySource::Prompt => formatter.write_str("prompt"),
            KeySource::File(path) => write!(formatter, "file {path}"),
            KeySource::Keysource(source) => write!(formatter, "keysource {source}"),
            KeySource::Nothing => formatter.write_str("nothing"),
        }
    }
}

/// The value of `org.zfsbootmenu:keysource` on `fs`, unset (`-`) as `None`.
pub fn keysource_property(runner: &dyn Runner, fs: &str) -> Option<String> {
    zfs_value(runner, KEYSOURCE.name, fs).filter(|value| !value.is_empty() && value != "-")
}

/// The key source of `fs`: the filesystem its
/// `org.zfsbootmenu:keysource` names, if that exists and is encrypted.
pub fn keysource(runner: &dyn Runner, fs: &str) -> Option<String> {
    if fs.is_empty() {
        zbm_trace::error!("fs is undefined");
        return None;
    }
    zbm_trace::debug!("fs set to {fs}");
    let Some(source) = zfs_value(runner, KEYSOURCE.name, fs) else {
        zbm_trace::warn!("failed to read {} on {fs}", KEYSOURCE.name);
        return None;
    };
    if source.is_empty() || source == "-" {
        return None;
    }
    let listed = ZfsCommand::new(runner)
        .text(&["list", "-o", "name", "-H", &source], Stderr::Hidden)
        .is_ok_and(|output| output.status.is_success());
    if !listed {
        zbm_trace::debug!("keysource {source} for {fs} is not a filesystem");
        return None;
    }
    if encryption_root(runner, &source).is_none() {
        zbm_trace::warn!("keysource {source} for {fs} is not encrypted; ignoring");
        return None;
    }
    Some(source)
}

/// A key location without `file://` and one leading `/`, as the legacy
/// runtime strips it; other locations stay as they are.
fn stripped(keylocation: &str) -> &str {
    let key = keylocation.strip_prefix("file://").unwrap_or(keylocation);
    key.strip_prefix('/').unwrap_or(key)
}

/// The key file of a `file://` key location, relative to the root, as the
/// legacy runtime strips it; `None` for other locations.
pub fn key_file(keylocation: &str) -> Option<&str> {
    let file = keylocation.strip_prefix("file://")?;
    let file = file.strip_prefix('/').unwrap_or(file);
    (!file.is_empty()).then_some(file)
}

/// The encryption root of `fs` and where its key would come from on a boot
/// chosen by `path`; `None` when `fs` is not encrypted.
pub fn key_source(
    runner: &dyn Runner,
    files: &dyn Files,
    fs: &str,
    path: BootPath,
) -> Option<(String, KeySource)> {
    let root = encryption_root(runner, fs)?;
    let keylocation = zfs_value(runner, "keylocation", &root).unwrap_or_else(|| "prompt".into());
    if keylocation == "prompt" {
        return Some((root, KeySource::Prompt));
    }
    if let Some(file) = key_file(&keylocation) {
        let in_image = format!("/{file}");
        if files.exists(Path::new(&in_image)) {
            return Some((root, KeySource::File(in_image)));
        }
        let source = (path == BootPath::Menu)
            .then(|| keysource(runner, fs))
            .flatten();
        if let Some(source) = source {
            return Some((root, KeySource::Keysource(source)));
        }
    }
    let passphrase = zfs_value(runner, "keyformat", &root).as_deref() == Some("passphrase");
    let source = if passphrase {
        KeySource::Prompt
    } else {
        KeySource::Nothing
    };
    Some((root, source))
}

#[cfg(test)]
mod tests;
