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
