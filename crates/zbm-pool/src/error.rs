//! What can go wrong importing pools, and making one writable.

/// An error of pool import.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PoolError {
    /// `zpool import` failed.
    #[error("zpool import {target} failed (status {status})")]
    Import {
        /// The pool, or `-a` for all.
        target: String,
        /// Its exit status (126 when it could not start).
        status: i32,
    },
    /// A rewind to a checkpoint was asked for all pools at once.
    #[error("rewind only works on a specific pool")]
    RewindAll,
    /// `/etc/hostid` could not be written.
    #[error("unable to write /etc/hostid")]
    WriteHostid,
    /// `zpool export` failed.
    #[error("zpool export {pool} failed (status {status})")]
    Export {
        /// The pool.
        pool: String,
        /// Its exit status (126 when it could not start).
        status: i32,
    },
}

/// Why a pool stays read-only before a change ([`crate::writable`]).
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum WritableError {
    /// Its export or import failed.
    #[error(transparent)]
    Pool(#[from] PoolError),
    /// The user did not allow a read-write import of a pool that a
    /// hibernated system may still use.
    #[error("a read-write import of {pool} was not allowed while a system may be resumed")]
    ResumeRefused {
        /// The pool.
        pool: String,
    },
    /// The pool may not be imported read/write: its
    /// `org.zfsbootmenu:readonly` property forbids it, or it is degraded.
    #[error("{pool} may not be imported read/write: {reason}")]
    ReadWriteProhibited {
        /// The pool.
        pool: String,
        /// Why, for people.
        reason: String,
    },
    /// The pool's properties could not be read.
    #[error("unable to read the properties of {pool}")]
    PoolProperty {
        /// The pool.
        pool: String,
    },
}

zbm_types::error_codes! {
    PoolError, POOL_CODES;
    Import = 301 => "a pool could not be imported":
        "zpool import failed. The pool may have been used by another host (see zbm.import_policy \
         and spl_hostid), miss a device, or need a newer ZFS. zpool import in the recovery shell \
         shows why.",
    RewindAll = 302 => "a rewind to a checkpoint needs one pool":
        "A rewind to a pool checkpoint was asked for every pool at once. Name the pool to rewind.",
    WriteHostid = 303 => "/etc/hostid could not be written":
        "The host ID from spl_hostid could not be written to /etc/hostid, so pools are imported \
         with the host ID of the image. Pools last used by another host may then refuse to import.",
    Export = 304 => "a pool could not be exported for a read-write import":
        "zpool export failed before the pool was imported read-write for a change, so it stays \
         imported read-only and the change is not made. A dataset may still be busy.",
}

// The codes of the read-write import kept the numbers they had in zbm-boot
// (ADR 0015).
zbm_types::error_codes! {
    WritableError, WRITABLE_CODES, delegate [Pool];
    ResumeRefused = 606 => "a read-write import was refused for a hibernated system":
        "A hibernated system on the pool may still be resumed, and a read-write import could \
         corrupt it, so the import was declined at the prompt. Resume that system first, or allow \
         the import, which loses its hibernated state.",
    ReadWriteProhibited = 607 => "a pool may not be imported read-write":
        "The pool's org.zfsbootmenu:readonly property is on, or the pool is degraded, so it stays \
         read-only.",
    PoolProperty = 608 => "a pool's properties could not be read":
        "zpool get failed for the pool; it may have been exported meanwhile.",
}

#[cfg(test)]
mod tests {
    use zbm_types::code::{Coded as _, ErrorCode};

    use super::*;

    #[test]
    fn errors_name_what_failed() {
        let import = PoolError::Import {
            target: "-a".to_owned(),
            status: 1,
        };
        assert_eq!(import.to_string(), "zpool import -a failed (status 1)");
        assert_eq!(
            PoolError::RewindAll.to_string(),
            "rewind only works on a specific pool"
        );
        assert_eq!(
            PoolError::WriteHostid.to_string(),
            "unable to write /etc/hostid"
        );
        let export = WritableError::from(PoolError::Export {
            pool: "tank".to_owned(),
            status: 1,
        });
        assert_eq!(export.to_string(), "zpool export tank failed (status 1)");
        assert_eq!(export.code(), ErrorCode::new(304));
        let refused = WritableError::ResumeRefused {
            pool: "tank".to_owned(),
        };
        assert_eq!(refused.code(), ErrorCode::new(606));
    }
}
