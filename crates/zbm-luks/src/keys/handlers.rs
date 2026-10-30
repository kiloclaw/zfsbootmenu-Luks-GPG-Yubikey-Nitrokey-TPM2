//! The registry of token handlers: which code tries a LUKS2 token of each
//! type, and when.
//!
//! [`token_handlers`] lists a [`TokenHandler`] for each token type with code
//! of its own (TPM2, clevis, FIDO2). Tokens are tried in two passes: first
//! those that need no interaction ([`Pass::Silent`]: TPM2 without a PIN,
//! clevis), then those that do ([`Pass::Interactive`]: TPM2 with a PIN,
//! FIDO2). A type without a handler is tried in the interactive pass through
//! cryptsetup's token plugin, when the image has one. Supporting a new token
//! type is one new handler.

use zbm_crypt::Token;

use super::Target;
use crate::error::LuksError;
use crate::keys::OpenMapping;
use crate::unlocker::Unlocker;

/// The pass in which a token is tried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pass {
    /// Before any token that needs a person.
    Silent,
    /// When no silent token opened the volume.
    Interactive,
}

impl Pass {
    /// The passes, in order.
    pub const ALL: [Pass; 2] = [Pass::Silent, Pass::Interactive];
}

/// The code that tries the tokens of one type.
pub trait TokenHandler {
    /// The token type, such as `systemd-tpm2`.
    fn kind(&self) -> &'static str;

    /// The pass in which `token` is tried.
    fn pass(&self, token: &Token) -> Pass;

    /// Tries to open `target` with `token`.
    ///
    /// # Errors
    ///
    /// Why the token did not open the volume; the handler logs it.
    fn try_open(
        &self,
        session: &mut Unlocker<'_>,
        target: Target<'_>,
        token: &Token,
    ) -> Result<OpenMapping, LuksError>;
}

/// The handlers, one per token type with code of its own.
pub fn token_handlers() -> Vec<Box<dyn TokenHandler>> {
    vec![Box::new(Tpm2), Box::new(Clevis), Box::new(Fido2)]
}

/// systemd-cryptenroll's TPM2 tokens, with or without a PIN.
pub struct Tpm2;

impl TokenHandler for Tpm2 {
    fn kind(&self) -> &'static str {
        "systemd-tpm2"
    }

    fn pass(&self, token: &Token) -> Pass {
        if token.flag("tpm2-pin") == Some(true) {
            Pass::Interactive
        } else {
            Pass::Silent
        }
    }

    fn try_open(
        &self,
        session: &mut Unlocker<'_>,
        target: Target<'_>,
        token: &Token,
    ) -> Result<OpenMapping, LuksError> {
        session.try_tpm2_token(target, &token.id, token)
    }
}

/// clevis bindings, which need no interaction.
pub struct Clevis;

impl TokenHandler for Clevis {
    fn kind(&self) -> &'static str {
        "clevis"
    }

    fn pass(&self, _token: &Token) -> Pass {
        Pass::Silent
    }

    fn try_open(
        &self,
        session: &mut Unlocker<'_>,
        target: Target<'_>,
        token: &Token,
    ) -> Result<OpenMapping, LuksError> {
        session.try_clevis_token(target, &token.id, token)
    }
}

/// systemd-cryptenroll's FIDO2 tokens, which need a security key.
pub struct Fido2;

impl TokenHandler for Fido2 {
    fn kind(&self) -> &'static str {
        "systemd-fido2"
    }

    fn pass(&self, _token: &Token) -> Pass {
        Pass::Interactive
    }

    fn try_open(
        &self,
        session: &mut Unlocker<'_>,
        target: Target<'_>,
        token: &Token,
    ) -> Result<OpenMapping, LuksError> {
        session.try_fido2_token(target, &token.id, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::token;

    #[test]
    fn each_type_has_one_handler_and_a_pass() {
        let handlers = token_handlers();
        let kinds: Vec<&str> = handlers.iter().map(|handler| handler.kind()).collect();
        assert_eq!(kinds, ["systemd-tpm2", "clevis", "systemd-fido2"]);
        let pin = token(r#"{"type":"systemd-tpm2","tpm2-pin":true}"#);
        let no_pin = token(r#"{"type":"systemd-tpm2","tpm2-pin":false}"#);
        assert_eq!(Tpm2.pass(&pin), Pass::Interactive);
        assert_eq!(Tpm2.pass(&no_pin), Pass::Silent);
        assert_eq!(Clevis.pass(&no_pin), Pass::Silent);
        assert_eq!(Fido2.pass(&no_pin), Pass::Interactive);
    }
}
