//! The boot environment list of the menu pilot (R0): a small snapshot of
//! the machine, which the pilot menus show. [`crate::model::Snapshot`] is
//! the full one.
//!
//! It is read through the ZFS port of `zbm-zfs` (REFACTOR-ROADMAP.md, "The
//! ZFS boundary"): the pools for their `bootfs`, and the filesystems with
//! their mount point and key status.

use std::collections::BTreeSet;

use zbm_types::DatasetName;
use zbm_zfs::{Zfs, ZfsError};

use crate::Encryption;

/// The machine, as far as a front end needs to know it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    /// The boot environments, in the order of their names.
    pub boot_environments: Vec<BootEnvironment>,
}

/// A filesystem that can be booted: its `mountpoint` is `/`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BootEnvironment {
    /// The filesystem.
    pub name: DatasetName,
    /// Whether it is the `bootfs` of its pool.
    pub is_default: bool,
    /// Whether it is encrypted, and whether its key is loaded.
    pub encryption: Encryption,
}

impl Snapshot {
    /// A machine with three boot environments in two pools, one of them
    /// locked, for demonstrations and tests of front ends.
    pub fn example() -> Snapshot {
        let environments = [
            ("tank/ROOT/arch", false, Encryption::None),
            ("zroot/ROOT/ubuntu", false, Encryption::Locked),
            ("zroot/ROOT/void", true, Encryption::Unlocked),
        ];
        Snapshot {
            boot_environments: environments
                .into_iter()
                .filter_map(|(name, is_default, encryption)| {
                    Some(BootEnvironment {
                        name: name.parse().ok()?,
                        is_default,
                        encryption,
                    })
                })
                .collect(),
        }
    }
}

/// Why the machine could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SnapshotError {
    /// ZFS could not answer.
    #[error(transparent)]
    Zfs(#[from] ZfsError),
}

/// The properties a boot environment is recognized by.
const PROPERTIES: &[&str] = &["mountpoint", "canmount", "keystatus"];

/// Reads the boot environments of the imported pools: the filesystems
/// mounted at `/` whose `canmount` is not `off`.
pub(crate) fn read(zfs: &dyn Zfs) -> Result<Snapshot, SnapshotError> {
    let defaults: BTreeSet<DatasetName> = zfs
        .pools()?
        .into_iter()
        .filter_map(|pool| pool.bootfs)
        .collect();
    let boot_environments = zfs
        .filesystems(None, PROPERTIES)?
        .into_iter()
        .filter(|dataset| {
            dataset.value("mountpoint") == Some("/") && dataset.value("canmount") != Some("off")
        })
        .map(|dataset| {
            let encryption = match dataset.value("keystatus") {
                None => Encryption::None,
                Some("available") => Encryption::Unlocked,
                Some(_) => Encryption::Locked,
            };
            BootEnvironment {
                is_default: defaults.contains(&dataset.name),
                name: dataset.name,
                encryption,
            }
        })
        .collect();
    Ok(Snapshot { boot_environments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::zfs::FakeZfs;

    #[test]
    fn filesystems_mounted_at_root_are_boot_environments() {
        let zfs = FakeZfs::new()
            .pool("tank")
            .pool("zroot")
            .filesystem("zroot", &[("mountpoint", "none")])
            .filesystem("zroot/ROOT", &[("mountpoint", "none"), ("canmount", "off")])
            .filesystem(
                "zroot/ROOT/void",
                &[
                    ("mountpoint", "/"),
                    ("canmount", "noauto"),
                    ("keystatus", "available"),
                ],
            )
            .filesystem(
                "zroot/ROOT/old",
                &[("mountpoint", "/"), ("canmount", "off")],
            )
            .filesystem("zroot/home", &[("mountpoint", "/home")])
            .filesystem(
                "tank/ROOT/arch",
                &[
                    ("mountpoint", "/"),
                    ("canmount", "noauto"),
                    ("keystatus", "unavailable"),
                ],
            )
            .bootfs("zroot", "zroot/ROOT/void");

        let snapshot = read(&zfs).unwrap();

        assert_eq!(
            snapshot.boot_environments,
            [
                BootEnvironment {
                    name: "tank/ROOT/arch".parse().unwrap(),
                    is_default: false,
                    encryption: Encryption::Locked,
                },
                BootEnvironment {
                    name: "zroot/ROOT/void".parse().unwrap(),
                    is_default: true,
                    encryption: Encryption::Unlocked,
                },
            ]
        );
        assert_eq!(
            zfs.calls(),
            ["pools", "filesystems - mountpoint,canmount,keystatus"]
        );
    }

    #[test]
    fn no_pools_means_no_boot_environments() {
        let snapshot = read(&FakeZfs::new()).unwrap();

        assert_eq!(snapshot, Snapshot::default());
    }

    #[test]
    fn a_failure_of_zfs_is_an_error_that_says_what_failed() {
        let error = SnapshotError::from(ZfsError::Missing("zroot".to_owned()));

        assert_eq!(error.to_string(), "zroot does not exist");
    }

    #[test]
    fn the_example_has_a_default_and_a_locked_environment() {
        let example = Snapshot::example();

        assert_eq!(example.boot_environments.len(), 3);
        assert!(example.boot_environments.iter().any(|e| e.is_default));
        assert!(
            example
                .boot_environments
                .iter()
                .any(|e| e.encryption == Encryption::Locked)
        );
    }
}
