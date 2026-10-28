//! clevis tokens: opening a volume with the key a clevis token recovers.
//!
//! The token's JWE is decrypted as `clevis luks pass` does, but from the token
//! JSON: clevis finds the token of a key slot by parsing `cryptsetup luksDump`
//! text, which lacks every token after the first with systemd's token plugins
//! (cryptsetup 2.8, systemd 261), and it cannot read a detached header. The
//! key `clevis decrypt` prints is captured as a secret and reaches
//! `cryptsetup` on standard input.

use super::Target;
use crate::error::LuksError;
use crate::error::TokenFailure;
use crate::unlocker::{Key, Unlocker};
use zbm_crypt::Token;
use zbm_crypt::clevis::Clevis;

impl Unlocker<'_> {
    /// Opens a volume with a clevis token.
    ///
    /// # Errors
    ///
    /// [`LuksError::Token`] when the image lacks clevis, the token names no
    /// key slot or clevis does not recover the key; [`LuksError::Open`] when
    /// the key it recovered does not open the volume.
    pub fn try_clevis_token(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<(), LuksError> {
        let device = target.device;
        let context = *self.context();
        let failed = |failure| target.token_error("clevis", id, failure);
        if !context.command_exists("clevis") || !context.command_exists("jose") {
            zbm_trace::info!("clevis token {id} on {device} needs clevis and jose in the image");
            return Err(failed(TokenFailure::Unsupported));
        }
        let Some(slot) = token.keyslots().into_iter().next() else {
            return Err(failed(TokenFailure::Unsupported));
        };

        // The clevis policy may use the TPM.
        self.have_tpm2();

        let key = Clevis::new(context.runner(), &self.settings().tpm2_tcti).decrypt(token);

        let Some(key) = key else {
            zbm_trace::debug!("clevis could not recover the key for slot {slot} on {device}");
            return Err(failed(TokenFailure::NotReleased));
        };
        self.cryptsetup_open(target.name, device, Key::Stdin(&key), target.args)?;
        zbm_trace::info!("unlocked {device} with clevis");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::unlocker::Settings;
    use zbm_platform::exec::Input;
    use zbm_platform::exec::{Output, Status};
    use zbm_platform::secret::Secret;
    use zbm_testkit::fakes::FakeSystem;

    fn clevis_machine(key: &'static [u8]) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/bin");
        for tool in ["clevis", "jose"] {
            fake.files.file_with_mode(format!("/bin/{tool}"), "", 0o755);
        }
        fake.runner
            .respond("jose fmt -j- -Og jwe -o-", 0, "{\"jwe\":1}\n");
        fake.runner.respond("jose jwe fmt -i- -c", 0, "a.b.c.d.e");
        fake.runner.on(move |command| {
            (command.program() == "clevis").then(|| {
                let input_ok =
                    matches!(command.get_stdin(), Input::Bytes(bytes) if bytes == b"a.b.c.d.e");
                Ok(Output::with_secret(
                    Status::exited(u8::from(!input_ok)),
                    Secret::copy_from(key),
                ))
            })
        });
        fake
    }

    #[test]
    fn the_jwe_is_decrypted_from_the_token() {
        let fake = clevis_machine(b"clevis-key");
        let mut settings = Settings::new("/zfsbootmenu");
        settings.tpm2_tcti = "swtpm:port=1".to_owned();
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, &settings, |session| {
            let json = r#"{"type":"clevis","keyslots":["3","4"],"jwe":{"protected":"x"}}"#;
            let token = Token::from_export("2", json).unwrap();
            assert_eq!(session.try_clevis_token(target, "2", &token), Ok(()));
            assert!(
                session
                    .try_clevis_token(
                        target,
                        "2",
                        &crate::keys::tests::token(r#"{"keyslots":[]}"#)
                    )
                    .is_err()
            );
        });
        assert_eq!(
            fake.runner.lines()[..4],
            [
                "jose fmt -j- -Og jwe -o-",
                "jose jwe fmt -i- -c",
                "clevis decrypt",
                "cryptsetup open --type luks --key-file=- /dev/sda v",
            ]
        );
        assert!(fake.kmsg().contains("unlocked /dev/sda with clevis"));
        fake.assert_no_leak(b"clevis-key");
    }

    #[test]
    fn failures_are_logged_and_tools_are_required() {
        let fake = clevis_machine(b"");
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(
                session
                    .try_clevis_token(
                        target,
                        "2",
                        &crate::keys::tests::token(r#"{"keyslots":["3"]}"#)
                    )
                    .is_err()
            );
        });
        assert!(
            fake.kmsg()
                .contains("clevis could not recover the key for slot 3 on /dev/sda")
        );
        let fake = FakeSystem::new();
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(
                session
                    .try_clevis_token(target, "2", &crate::keys::tests::token("{}"))
                    .is_err()
            );
        });
        assert!(
            fake.kmsg()
                .contains("clevis token 2 on /dev/sda needs clevis and jose in the image")
        );
    }
}
