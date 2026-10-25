//! systemd-fido2 tokens: finding a FIDO2 device and opening a volume with
//! its token, through cryptsetup's plugin or `fido2-assert`.
//!
//! Without cryptsetup's plugin, `fido2-assert` computes the FIDO2
//! hmac-secret of the token's credential and salt; as in systemd, the LUKS
//! passphrase is the base64-encoded hmac-secret, which `fido2-assert` prints
//! on its last line. It is captured as a secret and reaches `cryptsetup` on
//! standard input.

use std::path::Path;

use zbm_platform::files::FileKind;

use super::{Outcome, Target};
use crate::ask::{Awaited, SecretKind, SecretRequest, Told, VolumeRef, WaitRequest};
use crate::crypto::base64_encode;
use crate::error::{LuksError, TokenFailure};
use crate::unlocker::{Key, Unlocker, Wait};
use zbm_crypt::Token;
use zbm_crypt::fido2::{Assertion, Fido2};

pub use zbm_crypt::fido2::{last_line, listed_devices};

/// How many times the PIN of a security key is asked for.
const FIDO2_TRIES: u32 = 3;

impl Unlocker<'_> {
    /// Whether a FIDO2 (HID) device is connected.
    pub fn have_fido2_device(&self) -> bool {
        let files = self.system().files;
        let names = files.read_dir(Path::new("/dev")).unwrap_or_default();
        names.iter().any(|name| {
            name.as_encoded_bytes().starts_with(b"hidraw")
                && files
                    .metadata(&Path::new("/dev").join(name))
                    .is_ok_and(|metadata| metadata.kind == FileKind::CharDevice)
        })
    }

    /// Computes the LUKS passphrase of a systemd-fido2 token with
    /// `fido2-assert`.
    pub fn fido2_native(&self, token: &Token) -> Outcome {
        let system = self.system();
        if !system.command_exists("fido2-assert") || !system.command_exists("fido2-token") {
            return Outcome::Unsupported;
        }
        let (Some(credential), Some(salt)) =
            (token.text("fido2-credential"), token.text("fido2-salt"))
        else {
            return Outcome::Unsupported;
        };
        let rp = token.text("fido2-rp").unwrap_or("io.systemd.cryptsetup");
        let up = token.flag("fido2-up-required").unwrap_or(true);
        let uv = token.flag("fido2-uv-required").unwrap_or(false);
        // Tokens enrolled by systemd 248 lack the PIN flag: try without, then
        // with.
        let pin_modes: &[bool] = match token.flag("fido2-clientPin-required") {
            Some(true) => &[true],
            Some(false) => &[false],
            None => &[false, true],
        };

        let fido2 = Fido2::new(system.runner);
        let devices = fido2.devices();
        if devices.is_empty() {
            return Outcome::Failed;
        }

        for device in &devices {
            for pin in pin_modes {
                let assertion = Assertion {
                    rp,
                    credential,
                    salt,
                    presence: up,
                    verification: uv,
                    pin: *pin,
                };
                if up {
                    let touch = format!("Touch the security key {device} when it blinks.\n");
                    self.tell(Told::Touch, Some("yellow"), &touch);
                }
                if let Some(hmac) = self.fido2_assert(device, assertion) {
                    return Outcome::Key(hmac);
                }
                zbm_trace::debug!("fido2-assert on {device} (pin={pin}) did not produce a secret");
            }
        }
        Outcome::Failed
    }

    /// The hmac-secret of the security key `device` for `assertion`, with
    /// a new challenge. `fido2-assert` asks for a PIN on the terminal
    /// itself, so it then gets the console.
    fn fido2_assert(
        &self,
        device: &str,
        assertion: Assertion<'_>,
    ) -> Option<zbm_platform::secret::Secret> {
        let system = self.system();
        let random = system
            .files
            .read_prefix(Path::new("/dev/urandom"), 32)
            .unwrap_or_default();
        let challenge = base64_encode(&random);
        if !assertion.pin {
            return Fido2::new(system.runner).hmac_secret(device, assertion, &challenge);
        }
        let mut hmac = None;
        self.with_console(&mut |system| {
            hmac = Fido2::new(system.runner).hmac_secret(device, assertion, &challenge);
        });
        hmac
    }

    /// Opens a volume with a systemd-fido2 token, waiting for the security
    /// key first.
    ///
    /// # Errors
    ///
    /// [`LuksError::Token`] when no security key comes, the user leaves the
    /// PIN empty, the key does not release the secret, or the image has no
    /// support for the token; [`LuksError::Open`] when the secret does not
    /// open the volume.
    pub fn try_fido2_token(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<(), LuksError> {
        let device = target.device;
        let failed = |failure| target.token_error("systemd-fido2", id, failure);
        let message = self.paint(
            "cyan",
            &format!(
                "Insert the FIDO2 security key (YubiKey, Nitrokey, ...) for {}",
                self.paint("yellow", device)
            ),
        );
        let request = WaitRequest {
            volume: VolumeRef {
                name: target.name,
                device,
            },
            awaited: Awaited::SecurityKey,
            seconds: self.token_wait(),
            present: &|| self.have_fido2_device(),
        };
        if self.await_device(&request, &message) != Wait::Present {
            zbm_trace::info!("no FIDO2 security key for token {id} on {device}");
            return Err(failed(TokenFailure::NoDevice));
        }
        let heading = format!(
            "Unlock {} with a FIDO2 security key\n",
            self.paint("yellow", device)
        );
        let plain = format!("Unlock {device} with a FIDO2 security key");
        self.tell_with(Told::Working, &plain, || {
            self.say("\n");
            self.say(&self.paint("cyan", &heading));
        });
        if self.have_token_plugin("systemd-fido2") {
            self.fido2_plugin_open(target, id, token).map_err(failed)?;
        } else {
            // Without the plugin, fido2-assert asks for the PIN itself.
            match self.fido2_native(token) {
                Outcome::Key(key) => {
                    self.cryptsetup_open(target.name, device, Key::Stdin(&key), target.args)?;
                }
                Outcome::Failed => return Err(failed(TokenFailure::NotReleased)),
                Outcome::Unsupported => {
                    zbm_trace::warn!(
                        "FIDO2 token {id} on {device} needs fido2-tools or the systemd-fido2 \
                         plugin"
                    );
                    return Err(failed(TokenFailure::Unsupported));
                }
            }
        }
        zbm_trace::info!("unlocked {device} with FIDO2 security key");
        Ok(())
    }

    /// Opens the volume with the token through cryptsetup's plugin, asking
    /// for the PIN up to three times when the token needs one.
    fn fido2_plugin_open(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<(), TokenFailure> {
        let touch = token.flag("fido2-up-required") != Some(false);
        let touch_message = || {
            if touch {
                let text = "Touch the security key when it blinks.\n";
                self.tell(Told::Touch, Some("yellow"), text);
            }
        };
        if token.flag("fido2-clientPin-required") != Some(true) {
            touch_message();
            return self
                .open_token_plugin(target, id, None)
                .map_err(|_| TokenFailure::NotReleased);
        }
        for attempt in 1..=FIDO2_TRIES {
            let request = SecretRequest {
                volume: VolumeRef {
                    name: target.name,
                    device: target.device,
                },
                kind: SecretKind::Fido2Pin,
                attempt,
                tries: Some(FIDO2_TRIES),
                opened_elsewhere: None,
            };
            let asked = self.ask(&request, || {
                self.read_secret("Security key PIN (leave empty to use another method): ")
                    .into()
            });
            let pin = asked
                .secret()
                .filter(|pin| !pin.is_empty())
                .ok_or(TokenFailure::NoPin)?;
            touch_message();
            if self.open_token_plugin(target, id, Some(&pin)).is_ok() {
                return Ok(());
            }
            let refused = "The security key did not release the key.\n";
            self.tell(Told::Refused, Some("red"), refused);
        }
        Err(TokenFailure::TooManyTries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::handlers::TokenHandler;
    use crate::keys::tests::{with_prompter, with_session};
    use crate::unlocker::Settings;
    use zbm_platform::exec::Input;
    use zbm_platform::exec::{Output, Status};
    use zbm_platform::secret::Secret;
    use zbm_testkit::fakes::FakeSystem;
    use zbm_testkit::repo_root;

    #[test]
    fn devices_and_secrets_are_read_as_the_legacy_pipelines_read_them() {
        assert_eq!(
            listed_devices(
                "/dev/hidraw0: vendor=0x1050, product=0x0407 (Yubico YubiKey)\n\
                 not/dev/x: y\n/dev/hidraw2\n/dev/hidraw3: z\n"
            ),
            ["/dev/hidraw0", "/dev/hidraw3"]
        );
        assert_eq!(last_line(b"cdh\nrp\nAUTH\nSIG\nhmac\n").expose(), b"hmac");
        assert_eq!(last_line(b"a\nb").expose(), b"b");
        assert_eq!(last_line(b"a\n\n").expose(), b"");
        assert_eq!(last_line(b"").expose(), b"");
    }

    fn fido2_machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/bin");
        for tool in ["fido2-assert", "fido2-token"] {
            fake.files.file_with_mode(format!("/bin/{tool}"), "", 0o755);
        }
        fake.files
            .device("/dev/hidraw99")
            .file("/dev/urandom", [7_u8; 64]);
        fake.runner.respond(
            "fido2-token -L",
            0,
            "/dev/hidraw99: vendor=0x1050, product=0x0407 (Yubico YubiKey OTP+FIDO+CCID)\n",
        );
        fake.runner.on(|command| {
            if command.program() != "fido2-assert" {
                return None;
            }
            let pin = command.get_args().iter().any(|arg| arg == "pin=true");
            Some(Ok(if pin {
                Output::with_secret(
                    Status::exited(0),
                    Secret::copy_from(b"cdh\nrp\nAUTHDATA\nSIG\naG1hYw==\n"),
                )
            } else {
                Output::new(Status::exited(1), "")
            }))
        });
        fake
    }

    #[test]
    fn tokens_without_the_pin_flag_are_tried_both_ways() {
        let fake = fido2_machine();
        let json = std::fs::read_to_string(
            repo_root().join("crates/zbm-luks/tests/fixtures/token-fido2.json"),
        )
        .unwrap()
        .replace(r#""fido2-clientPin-required":false,"#, "");
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        // The assertion with a PIN gets the console: fido2-assert asks there.
        let prompts = crate::testing::Prompts::default();
        let settings = Settings::new("/zfsbootmenu");
        with_prompter(&fake, &settings, &prompts, |session| {
            let token = Token::from_export("0", &json).unwrap();
            assert_eq!(session.try_fido2_token(target, "0", &token), Ok(()));
        });
        assert_eq!(prompts.consoles(), 1);
        let lines = fake.runner.lines();
        assert_eq!(
            lines[1..],
            [
                "fido2-assert -G -h -t up=true -t uv=false /dev/hidraw99",
                "fido2-assert -G -h -t up=true -t uv=false -t pin=true /dev/hidraw99",
                "cryptsetup open --type luks --key-file=- /dev/sda v",
            ]
        );
        let input = match fake.runner.calls()[1].get_stdin() {
            Input::Bytes(bytes) => String::from_utf8(bytes.clone()).unwrap(),
            other => panic!("unexpected input {other:?}"),
        };
        assert_eq!(
            input,
            "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=\nio.systemd.cryptsetup\n\
             Y3JlZGVudGlhbA==\nc2FsdA==\n"
        );
        assert!(matches!(
            fake.runner.calls()[3].get_stdin(),
            Input::Secret(key) if key.expose() == b"aG1hYw=="
        ));
        assert!(
            fake.console
                .output_text()
                .contains("Touch the security key /dev/hidraw99")
        );
        assert!(
            fake.kmsg()
                .contains("unlocked /dev/sda with FIDO2 security key")
        );
        fake.assert_no_leak(b"aG1hYw==");
    }

    #[test]
    fn missing_keys_are_waited_for() {
        let fake = FakeSystem::new();
        fake.files
            .file("/zfsbootmenu/cmdline.d/zbm.luks_token_wait", "1");
        fake.console.timeout().timeout();
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            let token = crate::keys::tests::token("{}");
            assert!(session.try_fido2_token(target, "3", &token).is_err());
            // The registry's handler does the same.
            let handler = crate::keys::handlers::Fido2;
            assert!(handler.try_open(session, target, &token).is_err());
        });
        assert!(
            fake.console
                .output_text()
                .contains("Insert the FIDO2 security key (YubiKey, Nitrokey, ...) for")
        );
        assert!(
            fake.kmsg()
                .contains("no FIDO2 security key for token 3 on /dev/sda")
        );
    }

    /// A machine with a FIDO2 security key and the systemd-fido2 plugin,
    /// whose `cryptsetup` accepts the PIN `1234`, or no PIN.
    fn plugin_machine() -> FakeSystem {
        let fake = FakeSystem::new();
        fake.files.device("/dev/hidraw99").file(
            "/usr/lib/cryptsetup/libcryptsetup-token-systemd-fido2.so",
            "",
        );
        fake.runner.on(|command| {
            let right = matches!(
                command.get_stdin(),
                Input::Secret(pin) if pin.expose().starts_with(b"1234")
            );
            let pinless = matches!(command.get_stdin(), Input::Null);
            let status = if right || pinless { 0 } else { 2 };
            Some(Ok(Output::new(Status::exited(status), "")))
        });
        fake
    }

    fn fido2_token(pin: bool) -> Token {
        let json = std::fs::read_to_string(
            repo_root().join("crates/zbm-luks/tests/fixtures/token-fido2.json"),
        )
        .unwrap()
        .replace(
            r#""fido2-clientPin-required":false"#,
            &format!(r#""fido2-clientPin-required":{pin}"#),
        );
        Token::from_export("0", &json).unwrap()
    }

    #[test]
    fn the_plugin_asks_for_the_pin_when_the_token_needs_one() {
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        let fake = plugin_machine();
        fake.console.secret(b"bad").secret(b"1234");
        let settings = Settings::new("/zfsbootmenu");
        with_session(&fake, &settings, |session| {
            assert_eq!(
                session.try_fido2_token(target, "0", &fido2_token(true)),
                Ok(())
            );
            assert_eq!(
                session.try_fido2_token(target, "0", &fido2_token(false)),
                Ok(())
            );
        });
        let output = fake.console.output_text();
        assert!(output.contains("The security key did not release the key."));
        assert!(output.contains("Touch the security key when it blinks."));

        // No PIN, or three wrong ones, give up.
        for secrets in [&[&b""[..]][..], &[b"a", b"b", b"c"]] {
            let fake = plugin_machine();
            for secret in secrets {
                fake.console.secret(secret);
            }
            let result = with_session(&fake, &settings, |session| {
                session.try_fido2_token(target, "0", &fido2_token(true))
            });
            assert!(result.is_err());
        }
    }
}
