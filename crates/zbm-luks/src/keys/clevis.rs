//! clevis tokens: `luks_try_clevis_token`.
//!
//! The token's JWE is decrypted as `clevis luks pass` does, but from the token
//! JSON: clevis finds the token of a key slot by parsing `cryptsetup luksDump`
//! text, which lacks every token after the first with systemd's token plugins
//! (cryptsetup 2.8, systemd 261), and it cannot read a detached header. The
//! key `clevis decrypt` prints is captured as a secret and reaches
//! `cryptsetup` on standard input.

use zbm_core::exec::{Input, Stream};

use super::Target;
use crate::error::LuksError;
use crate::error::TokenFailure;
use crate::session::{Key, Session};
use crate::tokens::json_arr;

impl Session<'_> {
    /// Opens a volume with a clevis token (`luks_try_clevis_token`).
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
        json: &str,
    ) -> Result<(), LuksError> {
        let device = target.device;
        let system = *self.system();
        let failed = |failure| target.token_error("clevis", id, failure);
        if !system.command_exists("clevis") || !system.command_exists("jose") {
            zbm_trace::info!("clevis token {id} on {device} needs clevis and jose in the image");
            return Err(failed(TokenFailure::Unsupported));
        }
        let Some(slots) = json_arr(json, "keyslots").filter(|slots| !slots.is_empty()) else {
            return Err(failed(TokenFailure::Unsupported));
        };
        let slot = slots.split(',').next().unwrap_or_default().to_owned();

        // The clevis policy may use the TPM.
        self.have_tpm2();

        // jose fmt -j- -Og jwe -o- <<< json | jose jwe fmt -i- -c | clevis decrypt
        let run = |command: zbm_core::exec::Command| {
            system
                .runner
                .run(&command)
                .map(|output| output.stdout)
                .unwrap_or_default()
        };
        let jwe = run(self
            .command("jose")
            .args(["fmt", "-j-", "-Og", "jwe", "-o-"])
            .stdin(Input::Bytes(format!("{json}\n").into_bytes()))
            .stdout(Stream::Capture)
            .stderr(Stream::Null));
        let compact = run(self
            .command("jose")
            .args(["jwe", "fmt", "-i-", "-c"])
            .stdin(Input::Bytes(jwe))
            .stdout(Stream::Capture)
            .stderr(Stream::Null));
        let decrypt = self
            .command("clevis")
            .arg("decrypt")
            .env("TPM2TOOLS_TCTI", &self.settings().tpm2_tcti)
            .stdin(Input::Bytes(compact))
            .stdout(Stream::CaptureSecret)
            .stderr(Stream::Null);
        let key = system
            .runner
            .run(&decrypt)
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| output.secret)
            .filter(|key| !key.is_empty());

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
    use crate::session::Settings;
    use zbm_core::exec::{Output, Status};
    use zbm_core::secret::Secret;
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
                    Status::exited(if input_ok { 0 } else { 1 }),
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
        with_session(&fake, settings, |session| {
            let json = r#"{"type":"clevis","keyslots":["3","4"],"jwe":{"protected":"x"}}"#;
            assert_eq!(session.try_clevis_token(target, "2", json), Ok(()));
            assert!(
                session
                    .try_clevis_token(target, "2", r#"{"keyslots":[]}"#)
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
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(
                session
                    .try_clevis_token(target, "2", r#"{"keyslots":["3"]}"#)
                    .is_err()
            );
        });
        assert!(
            fake.kmsg()
                .contains("clevis could not recover the key for slot 3 on /dev/sda")
        );
        let fake = FakeSystem::new();
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_clevis_token(target, "2", "{}").is_err());
        });
        assert!(
            fake.kmsg()
                .contains("clevis token 2 on /dev/sda needs clevis and jose in the image")
        );
    }
}
