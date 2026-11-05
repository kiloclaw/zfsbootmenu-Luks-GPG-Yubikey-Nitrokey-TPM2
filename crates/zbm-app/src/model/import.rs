//! How [`Action::Import`](super::Action::Import) imports a pool
//! ([`ImportOptions`]): read-only or read/write, under a policy, and
//! rewound to its checkpoint or not (ADR 0016, point 3).

use zbm_types::ImportPolicy;

/// Whether a pool is imported to be written.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Access {
    /// Read-only, as start-up imports every pool.
    #[default]
    ReadOnly,
    /// Read/write.
    ReadWrite,
}

/// How a pool is imported.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportOptions {
    /// Read-only or read/write.
    pub access: Access,
    /// How a pool that is not imported is: as this host
    /// ([`ImportPolicy::Strict`]), as the host that owns it, whose host ID
    /// is then adopted ([`ImportPolicy::Hostid`]), or forced
    /// ([`ImportPolicy::Force`]). An imported pool is imported again with
    /// `zbm.import_policy`, as a change imports it read/write.
    pub policy: ImportPolicy,
    /// Whether the pool is rewound to its checkpoint, which loses every
    /// change made since.
    pub rewind: bool,
}

impl From<Access> for zbm_zfs::import::Access {
    fn from(access: Access) -> zbm_zfs::import::Access {
        match access {
            Access::ReadOnly => zbm_zfs::import::Access::ReadOnly,
            Access::ReadWrite => zbm_zfs::import::Access::ReadWrite,
        }
    }
}

impl ImportOptions {
    /// The rewind of `zpool import`.
    pub(crate) fn rewinds(self) -> zbm_zfs::import::Rewind {
        if self.rewind {
            zbm_zfs::import::Rewind::ToCheckpoint
        } else {
            zbm_zfs::import::Rewind::No
        }
    }

    /// The import of `pool` with these options, under `policy`.
    pub(crate) fn import(
        self,
        pool: &zbm_types::PoolName,
        policy: ImportPolicy,
    ) -> zbm_zfs::import::Import<'_> {
        zbm_zfs::import::Import::pool(pool)
            .policy(policy)
            .access(self.access.into())
            .rewind(self.rewinds())
    }

    /// Whether the locked menu keeps the import (ADR 0010): one that
    /// writes the pool, rewinds it, forces it, or adopts the host ID of
    /// its last host, which the default boot would then be given.
    pub(crate) fn is_kept_by_lock(self) -> bool {
        self.access == Access::ReadWrite || self.policy != ImportPolicy::Strict || self.rewind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_options_make_the_command_line_of_zpool_import() {
        let tank: zbm_types::PoolName = "tank".parse().unwrap();
        let rewind = ImportOptions {
            access: Access::ReadWrite,
            policy: ImportPolicy::Strict,
            rewind: true,
        };
        assert_eq!(
            rewind.import(&tank, ImportPolicy::Force).arguments(),
            [
                "import",
                "-N",
                "-f",
                "-o",
                "readonly=off",
                "--rewind-to-checkpoint",
                "tank"
            ]
        );
        let read_only = ImportOptions::default();
        assert_eq!(
            read_only.import(&tank, ImportPolicy::Hostid).arguments(),
            ["import", "-N", "-o", "readonly=on", "tank"]
        );
        assert_eq!(read_only.rewinds(), zbm_zfs::import::Rewind::No);
    }

    #[test]
    fn the_options_cross_the_wire_as_they_are() {
        let options = ImportOptions {
            access: Access::ReadWrite,
            policy: ImportPolicy::Force,
            rewind: true,
        };
        let action = super::super::Action::Import("tank".parse().unwrap(), options);
        let text = serde_json::to_string(&action).unwrap();
        assert_eq!(
            text,
            r#"{"Import":["tank",{"access":"ReadWrite","policy":"force","rewind":true}]}"#
        );
        assert_eq!(
            serde_json::from_str::<super::super::Action>(&text).unwrap(),
            action
        );
    }
}
