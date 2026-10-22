//! Why a LUKS operation did not succeed.

/// Why a key method did not open a volume with a LUKS2 token.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum TokenFailure {
    /// The machine has no TPM2.
    #[error("no TPM2 is available")]
    NoTpm,
    /// The TPM2 refuses PINs after too many wrong ones.
    #[error("the TPM2 is in dictionary-attack lockout")]
    Lockout,
    /// No security key was inserted in time, or the user skipped waiting.
    #[error("no security key was found")]
    NoDevice,
    /// The user left the PIN empty to use another method.
    #[error("no PIN was given")]
    NoPin,
    /// The PIN was wrong as often as allowed.
    #[error("the PIN was wrong too often")]
    TooManyTries,
    /// The token's device did not release the key.
    #[error("the key was not released")]
    NotReleased,
    /// The token needs a cryptsetup plugin, or tools, that the image lacks.
    #[error("the image has no support for it")]
    Unsupported,
}

/// Why a LUKS operation of this crate did not succeed.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum LuksError {
    /// The header is not a LUKS header.
    #[error("{header} is not a LUKS volume")]
    NotLuks {
        /// The device or detached header.
        header: String,
    },
    /// A mapping of that name exists already.
    #[error("the mapping {name} already exists")]
    MappingExists {
        /// The mapping name.
        name: String,
    },
    /// `cryptsetup open` failed.
    #[error("cryptsetup could not open {device} as {name} (status {status})")]
    Open {
        /// The LUKS device.
        device: String,
        /// The mapping name.
        name: String,
        /// Its exit status (127 when it could not start).
        status: i32,
    },
    /// `cryptsetup close` failed.
    #[error("cryptsetup could not close {name}")]
    Close {
        /// The mapping name.
        name: String,
    },
    /// A key script cannot be run.
    #[error("the key script {script} is not executable")]
    KeyScriptNotExecutable {
        /// The key script.
        script: String,
    },
    /// A LUKS2 token did not open the volume.
    #[error("{kind} token {id} did not open {device}: {failure}")]
    Token {
        /// The token type, such as `systemd-tpm2`.
        kind: String,
        /// The token ID.
        id: String,
        /// The LUKS device.
        device: String,
        /// Why.
        #[source]
        failure: TokenFailure,
    },
    /// None of the volume's LUKS2 tokens opened it.
    #[error("no LUKS2 token opened {device}")]
    NoToken {
        /// The LUKS device.
        device: String,
    },
    /// YubiKey challenge-response did not open the volume.
    #[error("YubiKey challenge-response did not open {device}")]
    YubiKey {
        /// The LUKS device.
        device: String,
    },
    /// The user skipped the volume.
    #[error("{device} was skipped at the user's request")]
    Skipped {
        /// The LUKS device.
        device: String,
    },
    /// No console was there to ask for a passphrase.
    #[error("no console is available to unlock {device}")]
    NoConsole {
        /// The LUKS device.
        device: String,
    },
    /// Some mappings could not be closed.
    #[error("unable to close {}", names.join(", "))]
    NotClosed {
        /// The mappings that stay open.
        names: Vec<String>,
    },
    /// Volumes needed unlocking and none was unlocked.
    #[error("no LUKS volume was unlocked ({failed} failed)")]
    NoneUnlocked {
        /// How many volumes failed.
        failed: i64,
    },
    /// A retry of the import loop unlocked nothing new.
    #[error("no new LUKS volume was unlocked")]
    NothingNew,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn errors_say_what_failed_and_why() {
        let token = LuksError::Token {
            kind: "systemd-tpm2".to_owned(),
            id: "0".to_owned(),
            device: "/dev/sda2".to_owned(),
            failure: TokenFailure::Lockout,
        };
        let not_closed = LuksError::NotClosed {
            names: vec!["a".to_owned(), "b".to_owned()],
        };

        assert_eq!(
            token.to_string(),
            "systemd-tpm2 token 0 did not open /dev/sda2: the TPM2 is in dictionary-attack lockout"
        );
        assert_eq!(
            token.source().map(ToString::to_string).as_deref(),
            Some("the TPM2 is in dictionary-attack lockout")
        );
        assert_eq!(not_closed.to_string(), "unable to close a, b");
    }
}
