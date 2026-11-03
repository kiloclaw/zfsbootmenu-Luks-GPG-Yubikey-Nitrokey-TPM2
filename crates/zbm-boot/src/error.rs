//! Why a step of the start-up program or of booting did not succeed.

/// Why a step of this crate did not succeed. The step has logged the details
/// to the kernel log, as the legacy runtime does; the error says what failed
/// for the code that decides what to do next.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BootError {
    /// A boot environment's key, mount, kernels or unmount failed.
    #[error(transparent)]
    BootEnv(#[from] zbm_bootenv::BootEnvError),
    /// `zpool export` failed.
    #[error("zpool export {pool} failed (status {status})")]
    Export {
        /// The pool.
        pool: String,
        /// Its exit status (126 when it could not start).
        status: i32,
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
    BootError, BOOT_CODES, delegate [BootEnv];
    Export = 601 => "a pool could not be exported":
        "zpool export failed before the kernel was booted, so the booted system imports a pool \
         that was not exported. A dataset may still be busy; the booted system may need zpool \
         import -f.",
    // 602 to 605 are the boot environment's, in zbm-bootenv
    // (BOOTENV_CODES).
    // 606, 607 and 608 are the read-write import's, in zbm-pool
    // (WRITABLE_CODES).
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
        let export = BootError::Export {
            pool: "tank".to_owned(),
            status: 1,
        };
        assert_eq!(export.to_string(), "zpool export tank failed (status 1)");
        let unmount = BootError::from(zbm_bootenv::BootEnvError::Unmount {
            mounts: vec!["/a".to_owned()],
        });
        assert_eq!(unmount.to_string(), "unable to unmount /a");
    }
}
