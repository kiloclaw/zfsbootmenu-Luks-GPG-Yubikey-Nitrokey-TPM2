//! Why a step of the start-up program or of booting did not succeed.

/// Why a step of this crate did not succeed. The step has logged the details
/// to the kernel log, as the legacy runtime does; the error says what failed
/// for the code that decides what to do next.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BootError {
    /// No pool was named.
    #[error("the pool is undefined")]
    NoPool,
    /// No boot environment was named.
    #[error("the boot environment is undefined")]
    NoFilesystem,
    /// Pool import failed.
    #[error(transparent)]
    Pool(#[from] zbm_pool::PoolError),
    /// `zpool export` failed.
    #[error("zpool export {pool} failed (status {status})")]
    Export {
        /// The pool.
        pool: String,
        /// Its exit status (126 when it could not start).
        status: i32,
    },
    /// The key of an encryption root could not be loaded.
    #[error("unable to load the key of {root}")]
    KeyNotLoaded {
        /// The encryption root.
        root: String,
    },
    /// A boot environment could not be mounted.
    #[error("unable to mount {fs}")]
    Mount {
        /// The boot environment.
        fs: String,
    },
    /// A boot environment has no kernel that can be booted.
    #[error("no kernel to boot was found on {fs}")]
    NoKernel {
        /// The boot environment.
        fs: String,
    },
    /// Some mount points could not be unmounted.
    #[error("unable to unmount {}", mounts.join(", "))]
    Unmount {
        /// The mount points that stay mounted.
        mounts: Vec<String>,
    },
    /// The user did not allow a read-write import of a pool that a
    /// hibernated system may still use.
    #[error("a read-write import of {pool} was not allowed while a system may be resumed")]
    ResumeRefused {
        /// The pool.
        pool: String,
    },
    /// `cryptsetup` is not in the image.
    #[error("cryptsetup is not available")]
    NoCryptsetup,
    /// `zluks unlock` failed: nothing was unlocked that was needed.
    #[error("zluks unlock failed (status {status})")]
    Zluks {
        /// Its exit status (127 when it could not start).
        status: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_what_failed() {
        let cases = [
            (
                BootError::Pool(zbm_pool::PoolError::Import {
                    target: "-a".to_owned(),
                    status: 1,
                }),
                "zpool import -a failed (status 1)",
            ),
            (
                BootError::Unmount {
                    mounts: vec!["/a".to_owned(), "/a/b".to_owned()],
                },
                "unable to unmount /a, /a/b",
            ),
        ];

        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
