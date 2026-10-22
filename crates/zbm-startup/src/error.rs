//! What can go wrong in the start-up program.

/// An error of the start-up program.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
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
    /// The runtime met something it cannot handle yet, logged it and opened
    /// the recovery shell.
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
