//! What a boot environment's code asks ZFS: dataset properties, encryption
//! roots and their keys.

use zbm_platform::Runner;
use zbm_types::DatasetName;
use zbm_zfs::{Stderr, ZfsName, ZfsText};

/// The value of `property` of `name`, quietly; `None` when `zfs` fails.
pub fn zfs_value(runner: &dyn Runner, property: &str, name: &impl ZfsName) -> Option<String> {
    ZfsText::new(runner).value(name, property, Stderr::Hidden)
}

/// The encryption root of `fs`, or `None` when its pool does not use
/// encryption or `fs` is not encrypted. (A snapshot's is its dataset's.)
pub fn encryption_root(runner: &dyn Runner, fs: &DatasetName) -> Option<DatasetName> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let pool = fs.pool();
    let feature =
        ZfsText::new(runner).pool_list(Some(&pool), &["feature@encryption"], Stderr::Hidden);
    if feature.trim_end_matches('\n') != "active" {
        zbm_trace::debug!("feature@encryption not active on {pool}"; %pool);
        return None;
    }
    let encroot = zfs_value(runner, "encryptionroot", fs)?;
    zbm_trace::debug!("{fs} encryptionroot property: {encroot}"; %fs, %encroot);
    encroot.parse().ok()
}

/// The encryption root of `fs` if its key is not loaded: its `keystatus` is
/// `unavailable`.
pub fn locked_root(runner: &dyn Runner, fs: &DatasetName) -> Option<DatasetName> {
    zbm_trace::debug!("fs set to {fs}"; %fs);
    let encroot = encryption_root(runner, fs)?;
    zbm_trace::debug!("{encroot} discovered as encryption root for {fs}"; %encroot, %fs);
    let keystatus = ZfsText::new(runner)
        .value(&encroot, "keystatus", Stderr::Merged)
        .unwrap_or_default();
    zbm_trace::debug!("{encroot} keystatus: {keystatus}"; %encroot, %keystatus);
    (keystatus == "unavailable").then_some(encroot)
}
