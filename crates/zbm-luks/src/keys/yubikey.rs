//! YubiKey HMAC-SHA1 challenge-response in the style of yubikey-luks:
//! `luks_try_ykchalresp` and `ykchalresp_present`.
//!
//! The typed passphrase (or its SHA-256, with `ykchalresp-hash`) is the
//! challenge, and the response (after the challenge, with
//! `ykchalresp-concat`) is the LUKS passphrase. The challenge reaches
//! `ykchalresp` on standard input, and its response is captured as a secret.

use zbm_platform::secret::Secret;

use super::Target;
use crate::config::YubiKey;
use crate::crypto::{hex, sha256};
use crate::error::LuksError;
use crate::session::{Key, Session, Wait};

impl Session<'_> {
    /// Whether a YubiKey is connected (`ykchalresp_present`); without `ykinfo`
    /// in the image, `ykchalresp` itself reports a missing key.
    pub fn ykchalresp_present(&self) -> bool {
        if !self.system().command_exists("ykinfo") {
            return true;
        }
        zbm_crypt::yubikey::YubiKey::new(self.system().runner).is_present()
    }

    /// Opens a volume with YubiKey challenge-response
    /// (`luks_try_ykchalresp`), with `tries` attempts (0: without limit).
    ///
    /// # Errors
    ///
    /// [`LuksError::YubiKey`] when `ykchalresp` is missing, no YubiKey comes,
    /// the user leaves the passphrase empty, or no attempt opens the volume.
    pub fn try_ykchalresp(
        &mut self,
        target: Target<'_>,
        yubikey: &YubiKey,
        tries: u64,
    ) -> Result<(), LuksError> {
        let device = target.device;
        let failed = || LuksError::YubiKey {
            device: device.to_owned(),
        };
        if !self.system().command_exists("ykchalresp") {
            zbm_trace::warn!("ykchalresp is not available to unlock {device}");
            return Err(failed());
        }
        let mut attempt = 0;
        while attempt < tries || tries == 0 {
            attempt += 1;
            let message = self.paint(
                "cyan",
                &format!("Insert the YubiKey for {}", self.paint("yellow", device)),
            );
            let waited = self.wait_for(self.token_wait(), &message, || self.ykchalresp_present());
            if waited != Wait::Present {
                zbm_trace::info!("no YubiKey present for {device}");
                return Err(failed());
            }

            self.say("\n");
            let line = format!(
                "Unlock {} with YubiKey challenge-response (slot {})\n",
                self.paint("yellow", device),
                yubikey.slot
            );
            self.say(&self.paint("cyan", &line));
            let pass = match self.read_secret("Passphrase (leave empty to use another method): ") {
                Some(pass) if !pass.is_empty() => pass,
                _ => return Err(failed()),
            };
            let challenge = if yubikey.hash {
                hex(&sha256(pass.expose()))
            } else {
                pass
            };

            self.say(&self.paint("yellow", "Touch the YubiKey if it blinks.\n"));
            let response = zbm_crypt::yubikey::YubiKey::new(self.system().runner)
                .response(yubikey.slot, &challenge);
            let Some(response) = response else {
                self.say(&self.paint("red", "The YubiKey did not respond.\n"));
                continue;
            };
            let key = if yubikey.concat {
                let mut key: Secret = challenge;
                key.push_bytes(response.expose());
                key
            } else {
                response
            };

            if self
                .cryptsetup_open(target.name, device, Key::Stdin(&key), target.args)
                .is_ok()
            {
                // Other volumes enrolled with the same slot reuse the response.
                self.passphrases.push(key);
                zbm_trace::info!("unlocked {device} with YubiKey challenge-response");
                return Ok(());
            }
            self.say(&self.paint("red", "No key available with this passphrase.\n"));
        }
        Err(failed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::session::Settings;
    use zbm_platform::exec::Input;
    use zbm_platform::exec::{Output, Status};
    use zbm_testkit::fakes::FakeSystem;

    /// A machine whose YubiKey answers `challenge` with `response`, and whose
    /// volume opens with `key`.
    fn yubikey_machine(challenge: Vec<u8>, key: Vec<u8>) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/bin");
        fake.files.file_with_mode("/bin/ykchalresp", "", 0o755);
        fake.runner.on(move |command| {
            let given = match command.get_stdin() {
                Input::Secret(secret) => secret.expose().to_vec(),
                _ => Vec::new(),
            };
            if command.program() == "ykchalresp" {
                return Some(Ok(if given == challenge {
                    Output::with_secret(Status::exited(0), Secret::copy_from(b"0f1e2d\n"))
                } else {
                    Output::new(Status::exited(1), "")
                }));
            }
            (command.program() == "cryptsetup").then(|| {
                Ok(Output::new(
                    Status::exited(if given == key { 0 } else { 2 }),
                    "",
                ))
            })
        });
        fake
    }

    #[test]
    fn responses_unlock_and_are_kept() {
        let fake = yubikey_machine(b"ykpass".to_vec(), b"0f1e2d".to_vec());
        fake.console.secret(b"bad").secret(b"ykpass");
        let yubikey = YubiKey {
            slot: 2,
            hash: false,
            concat: false,
        };
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert_eq!(session.try_ykchalresp(target, &yubikey, 3), Ok(()));
            assert_eq!(session.passphrases.len(), 1);
        });
        let output = fake.console.output_text();
        assert!(output.contains("The YubiKey did not respond."));
        assert!(output.contains("with YubiKey challenge-response (slot 2)"));
        assert!(
            fake.runner
                .lines()
                .contains(&"ykchalresp -2 -i-".to_owned())
        );
        fake.assert_no_leak(b"ykpass");
        fake.assert_no_leak(b"0f1e2d");
    }

    #[test]
    fn hashed_challenges_are_concatenated_with_the_response() {
        let digest = hex(&sha256(b"ykpass"));
        let mut key = digest.expose().to_vec();
        key.extend_from_slice(b"0f1e2d");
        let fake = yubikey_machine(digest.expose().to_vec(), key);
        fake.console.secret(b"ykpass");
        let yubikey = YubiKey {
            slot: 1,
            hash: true,
            concat: true,
        };
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert_eq!(session.try_ykchalresp(target, &yubikey, 1), Ok(()));
        });
        assert!(
            fake.runner
                .lines()
                .contains(&"ykchalresp -1 -i-".to_owned())
        );
    }

    #[test]
    fn an_empty_passphrase_gives_up() {
        let fake = yubikey_machine(Vec::new(), Vec::new());
        fake.console.secret(b"");
        let yubikey = YubiKey {
            slot: 2,
            hash: false,
            concat: false,
        };
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &yubikey, 0).is_err());
        });
        let fake = FakeSystem::new();
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &yubikey, 3).is_err());
        });
        assert!(
            fake.kmsg()
                .contains("ykchalresp is not available to unlock /dev/sda")
        );
    }
}
