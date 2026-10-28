//! Why a step of the start-up program or of booting did not succeed.

/// Why a step of this crate did not succeed. The step has logged the details
/// to the kernel log, as the legacy runtime does; the error says what failed
/// for the code that decides what to do next.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BootError {
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

zbm_types::error_codes! {
    BootError, BOOT_CODES, delegate [Pool];
    Export = 601 => "a pool could not be exported":
        "zpool export failed before the kernel was booted, so the booted system imports a pool \
         that was not exported. A dataset may still be busy; the booted system may need zpool \
         import -f.",
    KeyNotLoaded = 602 => "an encryption key could not be loaded":
        "The key of an encrypted dataset was not loaded: the passphrase was wrong, or its \
         keylocation could not be read. Its datasets cannot be mounted or booted.",
    Mount = 603 => "a boot environment could not be mounted":
        "The boot environment's dataset could not be mounted to find its kernels. Check that it \
         exists, that its key is loaded, and that its mountpoint is / or legacy.",
    NoKernel = 604 => "no kernel to boot was found":
        "The boot environment has no kernel and initramfs pair in /boot that ZFSBootMenu \
         recognizes. Install a kernel, or set org.zfsbootmenu:kernel.",
    Unmount = 605 => "a mount point could not be unmounted":
        "A file system stayed mounted, which can keep its pool from being exported before the \
         boot.",
    ResumeRefused = 606 => "a read-write import was refused for a hibernated system":
        "A hibernated system on the pool may still be resumed, and a read-write import could \
         corrupt it, so the import was declined at the prompt. Resume that system first, or allow \
         the import, which loses its hibernated state.",
    ReadWriteProhibited = 607 => "a pool may not be imported read-write":
        "The pool's org.zfsbootmenu:readonly property is on, or the pool is degraded, so it stays \
         read-only.",
    PoolProperty = 608 => "a pool's properties could not be read":
        "zpool get failed for the pool; it may have been exported meanwhile.",
    NoCryptsetup = 609 => "cryptsetup is not available":
        "The image has no cryptsetup, so LUKS volumes cannot be unlocked. Rebuild it with LUKS \
         support.",
    Zluks = 610 => "zluks unlock failed":
        "Unlocking LUKS volumes from the recovery path failed; the codes logged before this one \
         say why.",
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
