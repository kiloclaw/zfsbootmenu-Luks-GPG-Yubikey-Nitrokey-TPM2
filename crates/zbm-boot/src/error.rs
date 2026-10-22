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
    /// `zpool export` failed.
    #[error("zpool export {pool} failed (status {status})")]
    Export {
        /// The pool.
        pool: String,
        /// Its exit status (126 when it could not start).
        status: i32,
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
    /// `zbm.hookroot` is empty or not `filesystem//path`.
    #[error("the hook root '{hook_root}' is malformed")]
    HookRoot {
        /// The value of `zbm.hookroot`.
        hook_root: String,
    },
    /// The file system of `zbm.hookroot` could not be mounted.
    #[error("unable to mount the hook file system {filesystem}")]
    HookMount {
        /// The file system.
        filesystem: String,
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
    /// The start-up program met things the Rust runtime cannot do yet, and
    /// opened the recovery shell.
    #[error("not supported yet: {}", items.join("; "))]
    Unsupported {
        /// What it met.
        items: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_what_failed() {
        let cases = [
            (
                BootError::Import {
                    target: "-a".to_owned(),
                    status: 1,
                },
                "zpool import -a failed (status 1)",
            ),
            (
                BootError::Unmount {
                    mounts: vec!["/a".to_owned(), "/a/b".to_owned()],
                },
                "unable to unmount /a, /a/b",
            ),
            (
                BootError::Unsupported {
                    items: vec!["x".to_owned(), "y".to_owned()],
                },
                "not supported yet: x; y",
            ),
            (BootError::RewindAll, "rewind only works on a specific pool"),
        ];

        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
