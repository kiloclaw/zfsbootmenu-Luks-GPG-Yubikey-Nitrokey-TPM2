//! What can go wrong in the start-up program.

/// An error of the start-up program.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
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
    /// The ZFS kernel modules could not be loaded; the recovery shell
    /// opened, and start-up went on after it.
    #[error("the kernel modules stage failed")]
    ZfsModules {
        /// Why `modprobe zfs` failed.
        #[source]
        source: zbm_platform::error::Error,
    },
    /// The runtime met something it cannot handle yet, logged it and opened
    /// the recovery shell.
    #[error("not supported yet: {}", items.join("; "))]
    Unsupported {
        /// What it met.
        items: Vec<String>,
    },
}

zbm_types::error_codes! {
    StartupError, STARTUP_CODES;
    HookRoot = 701 => "zbm.hookroot is malformed":
        "zbm.hookroot must be filesystem//path, such as zroot/hooks//zfsbootmenu. The image's own \
         hooks run without it.",
    HookMount = 702 => "the file system of zbm.hookroot could not be mounted":
        "The hooks of zbm.hookroot could not be read, so only the image's own hooks run.",
    Unsupported = 703 => "something the runtime does not support yet":
        "The Rust runtime met a setting or layout it cannot handle yet, and opened the recovery \
         shell. The image's legacy runtime may handle it.",
    ZfsModules = 704 => "the ZFS kernel modules could not be loaded":
        "modprobe zfs failed: the image has no ZFS modules for the running kernel, or they do not \
         load on it. The recovery shell opened; no pool can be imported until the modules load. \
         Rebuild the image for this kernel.",
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_what_failed() {
        let cases = [
            (
                StartupError::HookRoot {
                    hook_root: "x".to_owned(),
                },
                "the hook root 'x' is malformed",
            ),
            (
                StartupError::HookMount {
                    filesystem: "tank/hooks".to_owned(),
                },
                "unable to mount the hook file system tank/hooks",
            ),
            (
                StartupError::Unsupported {
                    items: vec!["x".to_owned(), "y".to_owned()],
                },
                "not supported yet: x; y",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.to_string(), text);
        }
    }
}
