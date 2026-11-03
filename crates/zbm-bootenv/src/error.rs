//! Why loading a key, mounting a boot environment, finding its kernels or
//! unmounting did not succeed.

/// Why a step of a boot environment did not succeed. The step has logged
/// the details to the kernel log, as the legacy runtime does; the error says
/// what failed for the code that decides what to do next.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum BootEnvError {
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
}

// The codes kept the numbers they had in zbm-boot (ADR 0015).
zbm_types::error_codes! {
    BootEnvError, BOOTENV_CODES;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_what_failed() {
        let unmount = BootEnvError::Unmount {
            mounts: vec!["/a".to_owned(), "/a/b".to_owned()],
        };
        assert_eq!(unmount.to_string(), "unable to unmount /a, /a/b");
    }
}
