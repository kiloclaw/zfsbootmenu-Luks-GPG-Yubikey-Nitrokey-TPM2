//! Keys of encryption roots: where one comes from, in the order the legacy
//! runtime looks for it, and loading it (`load`), with the cache of keys
//! copied from key sources (`cache`):
//!
//! 1. the `load-key.d` hooks, which may unlock the root in their own way;
//! 2. `keylocation=prompt`: the passphrase prompt;
//! 3. a `file://` key that exists in ZFSBootMenu's own file system: that file;
//! 4. a key source (`org.zfsbootmenu:keysource`) that names an encrypted
//!    filesystem: the key file copied from it into `/zfsbootmenu/.keys`, for
//!    a boot chosen in the menu; the countdown's boot skips key sources;
//! 5. a `passphrase` key: the passphrase prompt;
//! 6. otherwise nothing ZFSBootMenu can load it with.

use zbm_config::spec::properties::KEYSOURCE;
use zbm_platform::Runner;
use zbm_types::DatasetName;
use zbm_zfs::ZfsText;

use crate::encryption_root;
use crate::zfs::zfs_value;

mod cache;
mod load;

pub use cache::clear_key_caches;
pub use load::{KeyLoad, load_key};

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

/// The key source of `fs`: the filesystem its
/// `org.zfsbootmenu:keysource` names, if that exists and is encrypted.
pub(crate) fn keysource(runner: &dyn Runner, fs: &DatasetName) -> Option<DatasetName> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let Some(source) = zfs_value(runner, KEYSOURCE.name, fs) else {
        zbm_trace::warn!("failed to read {property} on {fs}"; property = %KEYSOURCE.name, %fs);
        return None;
    };
    if source.is_empty() || source == "-" {
        return None;
    }
    let listed = source
        .parse::<DatasetName>()
        .ok()
        .filter(|source| ZfsText::new(runner).exists(source));
    let Some(source) = listed else {
        zbm_trace::debug!("keysource {source} for {fs} is not a filesystem"; %source, %fs);
        return None;
    };
    if encryption_root(runner, &source).is_none() {
        zbm_trace::warn!("keysource {source} for {fs} is not encrypted; ignoring"; %source, %fs);
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

#[cfg(test)]
mod tests;
