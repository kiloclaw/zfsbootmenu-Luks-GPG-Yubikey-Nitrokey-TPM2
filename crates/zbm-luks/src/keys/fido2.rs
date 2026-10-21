//! systemd-fido2 tokens: `luks_have_fido2_device`, `luks_fido2_native` and
//! `luks_try_fido2_token`.
//!
//! Without cryptsetup's plugin, `fido2-assert` computes the FIDO2
//! hmac-secret of the token's credential and salt; as in systemd, the LUKS
//! passphrase is the base64-encoded hmac-secret, which `fido2-assert` prints
//! on its last line. It is captured as a secret and reaches `cryptsetup` on
//! standard input.

use std::path::Path;

use zbm_core::exec::{Input, Stream};
use zbm_core::secret::Secret;
use zbm_system::files::FileKind;

use super::{Outcome, Target};
use crate::crypto::base64_encode;
use crate::session::{Key, Session};
use crate::tokens::{json_bool, json_str};

/// The devices `fido2-token -L` lists, as
/// `sed -n 's/^\(\/dev\/[^:]*\):.*/\1/p'` finds them.
pub fn listed_devices(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let (device, _) = line.split_once(':')?;
            device.starts_with("/dev/").then(|| device.to_owned())
        })
        .collect()
}

/// The last line of `fido2-assert`'s output, as `tail -n 1` and `$(...)` leave
/// it.
pub fn last_line(output: &[u8]) -> Secret {
    let body = output.strip_suffix(b"\n").unwrap_or(output);
    let start = body
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |at| at + 1);
    let mut line = Secret::copy_from(&body[start..]);
    super::strip_newlines(&mut line);
    line
}

impl Session<'_> {
    /// Whether a FIDO2 (HID) device is connected (`luks_have_fido2_device`).
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
    /// `fido2-assert` (`luks_fido2_native`).
    pub fn fido2_native(&self, json: &str) -> Outcome {
        let system = self.system();
        if !system.command_exists("fido2-assert") || !system.command_exists("fido2-token") {
            return Outcome::Unsupported;
        }
        let (Some(credential), Some(salt)) = (
            json_str(json, "fido2-credential"),
            json_str(json, "fido2-salt"),
        ) else {
            return Outcome::Unsupported;
        };
        let rp = json_str(json, "fido2-rp").unwrap_or_else(|| "io.systemd.cryptsetup".to_owned());
        let up = json_bool(json, "fido2-up-required").unwrap_or_else(|| "true".to_owned());
        let uv = json_bool(json, "fido2-uv-required").unwrap_or_else(|| "false".to_owned());
        // Tokens enrolled by systemd 248 lack the PIN flag: try without, then
        // with.
        let pin_modes: &[&str] = match json_bool(json, "fido2-clientPin-required").as_deref() {
            Some("true") => &["true"],
            Some("false") => &["false"],
            _ => &["false", "true"],
        };

        let list = self.command("fido2-token").arg("-L").stderr(Stream::Null);
        let listing = system
            .runner
            .run(&list)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        let devices = listed_devices(&listing);
        if devices.is_empty() {
            return Outcome::Failed;
        }

        for device in &devices {
            for pin in pin_modes {
                let mut args = vec![
                    "-G".to_owned(),
                    "-h".to_owned(),
                    "-t".to_owned(),
                    format!("up={up}"),
                    "-t".to_owned(),
                    format!("uv={uv}"),
                ];
                if *pin == "true" {
                    args.extend(["-t".to_owned(), "pin=true".to_owned()]);
                }
                if up == "true" {
                    self.say(&self.paint(
                        "yellow",
                        &format!("Touch the security key {device} when it blinks.\n"),
                    ));
                }
                let random = system
                    .files
                    .read_prefix(Path::new("/dev/urandom"), 32)
                    .unwrap_or_default();
                let challenge = base64_encode(&random);
                let input = format!(
                    "{}\n{rp}\n{credential}\n{salt}\n",
                    String::from_utf8_lossy(challenge.expose())
                );
                let assert = self
                    .command("fido2-assert")
                    .args(&args)
                    .arg(device)
                    .stdin(Input::Bytes(input.into_bytes()))
                    .stdout(Stream::CaptureSecret)
                    .stderr(Stream::Null);
                if let Ok(output) = system.runner.run(&assert) {
                    let secret = output.secret.unwrap_or_default();
                    let hmac = last_line(secret.expose());
                    if !hmac.is_empty() {
                        return Outcome::Key(hmac);
                    }
                }
                self.log().debug(format!(
                    "fido2-assert on {device} (pin={pin}) did not produce a secret"
                ));
            }
        }
        Outcome::Failed
    }

    /// Opens a volume with a systemd-fido2 token (`luks_try_fido2_token`),
    /// waiting for the security key first.
    pub fn try_fido2_token(&mut self, target: Target<'_>, id: &str, json: &str) -> bool {
        let device = target.device;
        let message = self.paint(
            "cyan",
            &format!(
                "Insert the FIDO2 security key (YubiKey, Nitrokey, ...) for {}",
                self.paint("yellow", device)
            ),
        );
        if !self.wait_for(self.token_wait(), &message, || self.have_fido2_device()) {
            self.log()
                .info(format!("no FIDO2 security key for token {id} on {device}"));
            return false;
        }
        let touch = json_bool(json, "fido2-up-required").as_deref() != Some("false");
        let heading = format!(
            "Unlock {} with a FIDO2 security key\n",
            self.paint("yellow", device)
        );

        if self.have_token_plugin("systemd-fido2") {
            self.say("\n");
            self.say(&self.paint("cyan", &heading));
            if json_bool(json, "fido2-clientPin-required").as_deref() == Some("true") {
                for _ in 0..3 {
                    let pin = match self
                        .read_secret("Security key PIN (leave empty to use another method): ")
                    {
                        Some(pin) if !pin.is_empty() => pin,
                        _ => return false,
                    };
                    if touch {
                        self.say(&self.paint("yellow", "Touch the security key when it blinks.\n"));
                    }
                    if self.open_token_plugin(target, id, Some(&pin)) {
                        self.log()
                            .info(format!("unlocked {device} with FIDO2 security key"));
                        return true;
                    }
                    self.say(&self.paint("red", "The security key did not release the key.\n"));
                }
                return false;
            }
            if touch {
                self.say(&self.paint("yellow", "Touch the security key when it blinks.\n"));
            }
            if self.open_token_plugin(target, id, None) {
                self.log()
                    .info(format!("unlocked {device} with FIDO2 security key"));
                return true;
            }
            return false;
        }

        // Without the plugin, fido2-assert asks for the PIN itself.
        self.say("\n");
        self.say(&self.paint("cyan", &heading));
        let opened = match self.fido2_native(json) {
            Outcome::Key(key) => {
                self.cryptsetup_open(target.name, device, Key::Stdin(&key), target.args)
            }
            Outcome::Failed => false,
            Outcome::Unsupported => {
                self.log().warning(format!(
                    "FIDO2 token {id} on {device} needs fido2-tools or the systemd-fido2 plugin"
                ));
                false
            }
        };
        if opened {
            self.log()
                .info(format!("unlocked {device} with FIDO2 security key"));
        }
        opened
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::session::Settings;
    use zbm_core::exec::{Output, Status};
    use zbm_testutil::fakes::FakeSystem;
    use zbm_testutil::repo_root;

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
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(session.try_fido2_token(target, "0", &json));
        });
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
        fake.console.timeout();
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(!session.try_fido2_token(target, "3", "{}"));
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
}
