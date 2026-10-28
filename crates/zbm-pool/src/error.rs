//! What can go wrong importing pools.

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
}

#[cfg(test)]
mod tests {
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
    }
}
