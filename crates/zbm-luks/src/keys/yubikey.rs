//! YubiKey HMAC-SHA1 challenge-response in the style of yubikey-luks:
//! finding a YubiKey and opening a volume with its response.
//!
//! The typed passphrase (or its SHA-256, with `ykchalresp-hash`) is the
//! challenge, and the response (after the challenge, with
//! `ykchalresp-concat`) is the LUKS passphrase.
//!
//! The challenge goes to the key over its OTP interface (`hidraw`,
//! `zbm_crypt::yubikey::otp`, ROADMAP.md SB11); `ykchalresp` is the
//! fallback, where the kernel has no `hidraw` or a device cannot be
//! reached, its challenge on standard input and its response captured as
//! a secret.

pub mod check;

use std::path::PathBuf;

use zbm_platform::secret::Secret;

use zbm_crypt::yubikey::otp::{self, Otp, OtpError};

use super::Target;
use crate::config::YubiKey;
use crate::crypto::{hex, sha256};
use crate::error::LuksError;
use crate::keys::OpenMapping;
use crate::unlocker::{Key, Unlocker};
use zbm_platform::interaction::Wait;
use zbm_platform::interaction::{Awaited, SecretKind, SecretRequest, Told, VolumeRef, WaitRequest};

impl Unlocker<'_> {
    /// Whether a YubiKey is connected: an OTP interface among the `hidraw`
    /// devices, or one `ykinfo` finds; where the kernel lists no `hidraw`
    /// devices and the image has no `ykinfo`, `ykchalresp` itself reports
    /// a missing key.
    pub fn ykchalresp_present(&self) -> bool {
        let context = self.context();
        if !otp::devices(context.files()).is_empty() {
            return true;
        }
        if context.command_exists("ykinfo") {
            return zbm_crypt::yubikey::YubiKey::new(context.runner()).is_present();
        }
        !otp::has_hidraw(context.files())
    }

    /// Whether a YubiKey can be asked at all: the kernel lists `hidraw`
    /// devices, or the image has `ykchalresp`.
    fn can_ask_yubikey(&self) -> bool {
        otp::has_hidraw(self.context().files()) || self.context().command_exists("ykchalresp")
    }

    /// The response of the YubiKey's `slot` to `challenge`: from the first
    /// key that gives one over its OTP interface, or from `ykchalresp`
    /// when none can be reached that way.
    fn yubikey_response(&self, slot: u8, challenge: &Secret) -> Option<Secret> {
        let context = self.context();
        let devices: Vec<PathBuf> = otp::devices(context.files());
        let protocol = Otp::new(context.hid(), context.clock());
        let mut reached = false;
        for device in &devices {
            match protocol.response(device, slot, challenge) {
                Ok(response) => {
                    let serial = protocol
                        .serial(device)
                        .map_or_else(|_| "-".to_owned(), |serial| serial.to_string());
                    zbm_trace::info!(
                        "YubiKey {serial} on {device} responded from slot {slot}";
                        %serial, device = %device.display(), %slot
                    );
                    return Some(response);
                }
                Err(error) => {
                    reached |= !matches!(error, OtpError::Io(_));
                    zbm_trace::warn!("{device}: {error}"; device = %device.display(), %error);
                }
            }
        }
        // A key that was reached and gave nothing would give nothing to
        // `ykchalresp` either, and would want another touch.
        if reached || !context.command_exists("ykchalresp") {
            return None;
        }
        zbm_crypt::yubikey::YubiKey::new(context.runner()).response(slot, challenge)
    }

    /// Opens a volume with YubiKey challenge-response, with `tries`
    /// attempts (0: without limit).
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
    ) -> Result<OpenMapping, LuksError> {
        let device = target.device();
        let failed = || LuksError::YubiKey {
            device: device.to_string(),
        };
        if !self.can_ask_yubikey() {
            zbm_trace::warn!(
                "neither hidraw nor ykchalresp is available to unlock {device}";
                %device
            );
            return Err(failed());
        }
        let mut attempt = 0;
        while attempt < tries || tries == 0 {
            attempt += 1;
            let key = match self.ykchalresp_key(target, yubikey, (attempt, tries)) {
                Attempt::Key(key) => key,
                Attempt::NoResponse => continue,
                Attempt::Stop => return Err(failed()),
            };
            if let Ok(open) = self.cryptsetup_open(target, Key::Stdin(&key)) {
                // Other volumes enrolled with the same slot reuse the response.
                self.passphrases.push(key);
                zbm_trace::info!("unlocked {device} with YubiKey challenge-response"; %device);
                return Ok(open);
            }
            let wrong = "No key available with this passphrase.\n";
            self.tell(Told::Refused, wrong);
        }
        Err(failed())
    }

    /// One attempt's LUKS passphrase from the YubiKey, the attempt `attempt`
    /// of `tries` (0: without limit).
    fn ykchalresp_key(
        &self,
        target: Target<'_>,
        yubikey: &YubiKey,
        (attempt, tries): (u64, u64),
    ) -> Attempt {
        let volume = target.volume();
        let Some(pass) = self.ykchalresp_passphrase(volume, yubikey, (attempt, tries)) else {
            return Attempt::Stop;
        };
        let challenge = if yubikey.hash {
            hex(&sha256(pass.expose()))
        } else {
            pass
        };
        self.tell(Told::Touch, "Touch the YubiKey if it blinks.\n");
        let response = self.yubikey_response(yubikey.slot, &challenge);
        let Some(response) = response else {
            self.tell(Told::Refused, "The YubiKey did not respond.\n");
            return Attempt::NoResponse;
        };
        Attempt::Key(if yubikey.concat {
            let mut key: Secret = challenge;
            key.push_bytes(response.expose());
            key
        } else {
            response
        })
    }
}

impl Unlocker<'_> {
    /// Waits for the YubiKey of `volume`, then asks for the passphrase it
    /// turns into the key, the attempt `attempt` of `tries` (0: without
    /// limit); `None` when no YubiKey comes or the passphrase is left
    /// empty.
    fn ykchalresp_passphrase(
        &self,
        volume: VolumeRef<'_>,
        yubikey: &YubiKey,
        (attempt, tries): (u64, u64),
    ) -> Option<Secret> {
        let device = volume.device;
        let request = WaitRequest {
            volume,
            awaited: Awaited::YubiKey,
            seconds: self.token_wait(),
            present: &|| self.ykchalresp_present(),
        };
        if self.await_device(&request) != Wait::Present {
            zbm_trace::info!("no YubiKey present for {device}"; %device);
            return None;
        }
        let request = SecretRequest {
            volume,
            kind: SecretKind::YubiKeyPassphrase(yubikey.slot.to_string()),
            attempt: u32::try_from(attempt).unwrap_or(u32::MAX),
            tries: (tries > 0).then(|| u32::try_from(tries).unwrap_or(u32::MAX)),
            opened_elsewhere: None,
            description: None,
        };
        let asked = self.ask(&request);
        asked.secret().filter(|pass| !pass.is_empty())
    }
}

/// What one attempt of YubiKey challenge-response gave.
enum Attempt {
    /// The LUKS passphrase.
    Key(Secret),
    /// The YubiKey did not respond; the next attempt may do better.
    NoResponse,
    /// No YubiKey came, or the user left the passphrase empty: no more
    /// attempts.
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::unlocker::Settings;
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
        let target = crate::testing::target("v", "/dev/sda", &args);
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &yubikey, 3).is_ok());
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
        let target = crate::testing::target("v", "/dev/sda", &args);
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &yubikey, 1).is_ok());
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
        let target = crate::testing::target("v", "/dev/sda", &args);
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &yubikey, 0).is_err());
        });
        let fake = FakeSystem::new();
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &yubikey, 3).is_err());
        });
        assert!(
            fake.kmsg()
                .contains("neither hidraw nor ykchalresp is available to unlock /dev/sda")
        );
    }

    /// The hexadecimal of the native fake key's response to `ykpass`: 20
    /// bytes, `0f 1e 2d` and zeros.
    fn native_response() -> Vec<u8> {
        let mut text = b"0f1e2d".to_vec();
        text.extend_from_slice(&[b'0'; 34]);
        text
    }

    /// A machine with `key` plugged in, whose volume opens with
    /// [`native_response`].
    fn native_machine(key: zbm_testkit::fakes::FakeYubiKey) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.plug_yubikey("hidraw0", key);
        fake.runner.on(|command| {
            let given = match command.get_stdin() {
                Input::Secret(secret) => secret.expose().to_vec(),
                _ => Vec::new(),
            };
            let opens = given == native_response();
            (command.program() == "cryptsetup")
                .then(|| Ok(Output::new(Status::exited(if opens { 0 } else { 2 }), "")))
        });
        fake
    }

    /// A native fake key whose slot 2 answers `ykpass` with `0f 1e 2d`.
    fn native_key() -> zbm_testkit::fakes::FakeYubiKey {
        zbm_testkit::fakes::FakeYubiKey::new(|slot, challenge| {
            let mut response = vec![0x0f, 0x1e, 0x2d];
            response.resize(20, 0);
            (slot == 2 && challenge == b"ykpass").then_some(response)
        })
    }

    const SLOT_2: YubiKey = YubiKey {
        slot: 2,
        hash: false,
        concat: false,
    };

    #[test]
    fn the_key_answers_over_hidraw_without_ykchalresp() {
        let fake = native_machine(native_key().with_serial(4242));
        fake.console.secret(b"ykpass");
        let args = [];
        let target = crate::testing::target("v", "/dev/sda", &args);
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.ykchalresp_present());
            assert!(session.try_ykchalresp(target, &SLOT_2, 1).is_ok());
        });
        assert!(
            fake.runner
                .lines()
                .iter()
                .all(|line| !line.starts_with("ykchalresp") && !line.starts_with("ykinfo"))
        );
        assert_eq!(
            fake.hid.challenges("/dev/hidraw0"),
            [(2, b"ykpass".to_vec())]
        );
        assert!(
            fake.kmsg()
                .contains("YubiKey 4242 on /dev/hidraw0 responded from slot 2"),
            "{}",
            fake.kmsg()
        );
        fake.assert_no_leak(b"ykpass");
        fake.assert_no_leak(&native_response());
    }

    #[test]
    fn a_key_that_was_reached_and_gave_nothing_is_not_asked_again_by_ykchalresp() {
        let fake = native_machine(native_key().waiting_for_touch(usize::MAX));
        fake.environment.set("PATH", "/bin");
        fake.files.file_with_mode("/bin/ykchalresp", "", 0o755);
        fake.console.secret(b"ykpass");
        let args = [];
        let target = crate::testing::target("v", "/dev/sda", &args);
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &SLOT_2, 1).is_err());
        });
        assert!(
            fake.console
                .output_text()
                .contains("The YubiKey did not respond.")
        );
        assert!(
            fake.runner
                .lines()
                .iter()
                .all(|line| !line.starts_with("ykchalresp"))
        );
        assert!(
            fake.kmsg().contains("the YubiKey was not touched"),
            "{}",
            fake.kmsg()
        );
    }

    #[test]
    fn a_key_that_cannot_be_reached_over_hidraw_is_asked_through_ykchalresp() {
        let fake = yubikey_machine(b"ykpass".to_vec(), b"0f1e2d".to_vec());
        fake.plug_yubikey("hidraw0", native_key().gone());
        fake.console.secret(b"ykpass");
        let args = [];
        let target = crate::testing::target("v", "/dev/sda", &args);
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_ykchalresp(target, &SLOT_2, 1).is_ok());
        });
        assert!(
            fake.runner
                .lines()
                .contains(&"ykchalresp -2 -i-".to_owned())
        );
        let log = fake.kmsg();
        assert!(
            log.contains("/dev/hidraw0: the YubiKey could not be reached"),
            "{log}"
        );
    }

    #[test]
    fn a_key_is_present_when_hidraw_or_ykinfo_shows_one() {
        let settings = Settings::new("/zfsbootmenu");
        // Neither: `ykchalresp` will say.
        let bare = FakeSystem::new();
        with_session(&bare, &settings, |session| {
            assert!(session.ykchalresp_present());
        });
        // `hidraw` without a YubiKey.
        let empty = FakeSystem::new();
        empty.files.dir("/sys/class/hidraw");
        with_session(&empty, &settings, |session| {
            assert!(!session.ykchalresp_present());
        });
        // `ykinfo` finds one.
        empty.environment.set("PATH", "/bin");
        empty.files.file_with_mode("/bin/ykinfo", "", 0o755);
        with_session(&empty, &settings, |session| {
            assert!(session.ykchalresp_present());
        });
        assert_eq!(empty.runner.lines(), ["ykinfo -s"]);
    }
}
