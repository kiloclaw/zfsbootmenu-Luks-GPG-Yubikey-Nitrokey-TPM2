//! The key methods of `zfsbootmenu-luks-keys.sh`: LUKS2 tokens through
//! cryptsetup's plugins or ZFSBootMenu's own code ([`tpm2`], [`fido2`],
//! [`clevis`]), GnuPG-encrypted key files ([`gpg`]) and YubiKey
//! challenge-response ([`yubikey`]).
//!
//! Every method that opens a volume returns whether the volume is open
//! afterwards.

pub mod clevis;
pub mod fido2;
pub mod gpg;
pub mod tpm2;
pub mod yubikey;

use std::path::Path;

use zbm_core::exec::{Input, Stream};
use zbm_core::secret::Secret;
use zbm_core::shell::command_substitution;

use crate::session::Session;
use crate::tokens::{json_bool, list_tokens};

/// The volume a key method opens.
#[derive(Clone, Copy, Debug)]
pub struct Target<'v> {
    /// The mapping name.
    pub name: &'v str,
    /// The LUKS device.
    pub device: &'v str,
    /// More arguments for `cryptsetup open`.
    pub args: &'v [String],
}

/// What a key method's own implementation of a token got.
pub enum Outcome {
    /// The LUKS passphrase.
    Key(Secret),
    /// The token did not release the key.
    Failed,
    /// The token needs something ZFSBootMenu's own implementation lacks.
    Unsupported,
}

/// Where cryptsetup's token plugins may be.
const PLUGIN_DIRS: [&str; 4] = [
    "/usr/lib/cryptsetup",
    "/usr/lib64/cryptsetup",
    "/lib/cryptsetup",
    "/lib64/cryptsetup",
];

/// Removes all final newlines, as `$(...)` does.
pub(crate) fn strip_newlines(secret: &mut Secret) {
    while secret.expose().last() == Some(&b'\n') {
        secret.pop();
    }
}

/// A secret followed by a newline, as `printf '%s\n'` or a here-string
/// writes it.
pub(crate) fn with_newline(secret: &Secret) -> Secret {
    let mut line = secret.clone();
    line.push(b'\n');
    line
}

impl Session<'_> {
    /// Whether cryptsetup's token plugin for `kind` is installed
    /// (`luks_have_token_plugin`); never when the settings force ZFSBootMenu's
    /// own implementations.
    pub fn have_token_plugin(&self, kind: &str) -> bool {
        if !self.settings().token_plugins {
            return false;
        }
        let files = self.system().files;
        let library = format!("libcryptsetup-token-{kind}.so");
        // /usr/lib/cryptsetup /usr/lib64/cryptsetup /usr/lib/*/cryptsetup and
        // the same below /lib, in this order.
        let mut dirs: Vec<String> = Vec::new();
        for root in ["/usr", ""] {
            dirs.push(format!("{root}/lib/cryptsetup"));
            dirs.push(format!("{root}/lib64/cryptsetup"));
            let lib = format!("{root}/lib");
            let mut names = files.read_dir(Path::new(&lib)).unwrap_or_default();
            names.retain(|name| !name.as_encoded_bytes().starts_with(b"."));
            dirs.extend(
                names
                    .iter()
                    .map(|name| format!("{lib}/{}/cryptsetup", name.to_string_lossy())),
            );
        }
        debug_assert!(PLUGIN_DIRS.iter().all(|dir| dirs.iter().any(|d| d == dir)));
        dirs.iter()
            .any(|dir| files.exists(&Path::new(dir).join(&library)))
    }

    /// Opens a volume through cryptsetup's token plugin
    /// (`luks_open_token_plugin`); cryptsetup reads a PIN from standard
    /// input, followed by a newline.
    pub fn open_token_plugin(&self, target: Target<'_>, id: &str, pin: Option<&Secret>) -> bool {
        let stdin = pin.map_or(Input::Null, |pin| Input::Secret(with_newline(pin)));
        let command = self
            .command("cryptsetup")
            .args(["open", "--type", "luks", "--token-only", "--token-id", id])
            .args(target.args)
            .args([target.device, target.name])
            .stdin(stdin)
            .stdout(Stream::Capture)
            .merge_stderr();
        let (status, output) = match self.system().runner.run(&command) {
            Ok(output) => (output.status.shell_code(), output.stdout_text()),
            Err(error) => (127, error.to_string()),
        };
        if status != 0 {
            zbm_trace::debug!(
                "token {id} on {} failed ({status}): {}",
                target.device,
                command_substitution(&output)
            );
        }
        status == 0
    }

    /// The JSON of a token (`cryptsetup token export`), or `None` if it
    /// cannot be read.
    pub fn token_json(&self, header: &str, id: &str) -> Option<String> {
        let command = self
            .command("cryptsetup")
            .args(["token", "export", "--token-id", id, header])
            .stderr(Stream::Null);
        let output = self.system().runner.run(&command).ok()?;
        output
            .status
            .success()
            .then(|| command_substitution(&output.stdout_text()).to_owned())
    }

    /// Tries the LUKS2 tokens of a volume whose header is `header`
    /// (`luks_try_tokens`): first those that need no interaction (TPM2
    /// without a PIN, clevis), then those that do (TPM2 with a PIN, FIDO2,
    /// and any other type that has a plugin in the image).
    pub fn try_tokens(&mut self, target: Target<'_>, header: &str) -> bool {
        let tokens = list_tokens(self.system(), header);
        if tokens.is_empty() {
            return false;
        }
        for interactive in [false, true] {
            for token in &tokens {
                let Some(json) = self.token_json(header, &token.id) else {
                    continue;
                };
                let id = token.id.as_str();
                let with_pin = json_bool(&json, "tpm2-pin").as_deref() == Some("true");
                let opened = match (interactive, token.kind.as_str()) {
                    (false, "systemd-tpm2") if !with_pin => self.try_tpm2_token(target, id, &json),
                    (false, "clevis") => self.try_clevis_token(target, id, &json),
                    (true, "systemd-tpm2") if with_pin => self.try_tpm2_token(target, id, &json),
                    (true, "systemd-fido2") => self.try_fido2_token(target, id, &json),
                    (true, "systemd-tpm2" | "clevis") | (false, _) => false,
                    (true, kind) => {
                        if self.have_token_plugin(kind) {
                            self.open_token_plugin(target, id, None)
                        } else {
                            zbm_trace::debug!(
                                "no support for {kind} token {id} on {}",
                                target.device
                            );
                            false
                        }
                    }
                };
                if opened {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Luks, Settings};
    use crate::testing::{NoPrompts, log};
    use zbm_core::exec::{Output, Status};
    use zbm_testkit::fakes::FakeSystem;

    pub(crate) fn with_session<T>(
        fake: &FakeSystem,
        settings: Settings,
        body: impl FnOnce(&mut Session<'_>) -> T,
    ) -> T {
        let _log = log(fake);
        let luks = Luks {
            system: fake.system(),
            settings: &settings,
            prompter: &NoPrompts,
        };
        body(&mut Session::new(luks))
    }

    #[test]
    fn plugins_are_found_in_the_library_directories() {
        let fake = FakeSystem::new();
        fake.files.dir("/usr/lib/.hidden/cryptsetup").file(
            "/usr/lib/.hidden/cryptsetup/libcryptsetup-token-systemd-fido2.so",
            "",
        );
        fake.files.file(
            "/usr/lib/x86_64-linux-gnu/cryptsetup/libcryptsetup-token-systemd-tpm2.so",
            "",
        );
        fake.files
            .file("/lib64/cryptsetup/libcryptsetup-token-ssh.so", "");
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(session.have_token_plugin("systemd-tpm2"));
            assert!(session.have_token_plugin("ssh"));
            assert!(!session.have_token_plugin("systemd-fido2"));
        });
        let mut settings = Settings::new("/zfsbootmenu");
        settings.token_plugins = false;
        with_session(&fake, settings, |session| {
            assert!(!session.have_token_plugin("systemd-tpm2"));
        });
    }

    #[test]
    fn plugins_read_the_pin_on_standard_input() {
        let fake = FakeSystem::new();
        fake.runner.on(|command| {
            let pin_ok = matches!(
                command.get_stdin(),
                Input::Secret(pin) if pin.expose() == b"1234\n"
            );
            Some(Ok(Output::new(
                Status::exited(if pin_ok { 0 } else { 1 }),
                "No usable token is available.\n",
            )))
        });
        let args = ["--readonly".to_owned()];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            assert!(session.open_token_plugin(target, "0", Some(&Secret::copy_from(b"1234"))));
            assert!(!session.open_token_plugin(target, "1", None));
        });
        assert_eq!(
            fake.runner.lines(),
            [
                "cryptsetup open --type luks --token-only --token-id 0 --readonly /dev/sda v",
                "cryptsetup open --type luks --token-only --token-id 1 --readonly /dev/sda v",
            ]
        );
        assert!(
            fake.kmsg()
                .contains("token 1 on /dev/sda failed (1): No usable token is available.")
        );
        fake.assert_no_leak(b"1234");
    }

    #[test]
    fn tokens_without_interaction_come_first() {
        let fake = FakeSystem::new();
        let dump = r#"{
  "tokens":{
    "0":{
      "type":"systemd-fido2"
    },
    "1":{
      "type":"systemd-tpm2"
    },
    "2":{
      "type":"systemd-tpm2"
    },
    "3":{
      "type":"other"
    }
  }
}
"#;
        fake.runner
            .respond("cryptsetup luksDump --dump-json-metadata /dev/sda", 0, dump);
        fake.runner.respond(
            "cryptsetup token export --token-id 0 /dev/sda",
            0,
            r#"{"type":"systemd-fido2","keyslots":["1"]}"#,
        );
        fake.runner.respond(
            "cryptsetup token export --token-id 1 /dev/sda",
            0,
            r#"{"type":"systemd-tpm2","tpm2-pin":true}"#,
        );
        fake.runner.respond(
            "cryptsetup token export --token-id 2 /dev/sda",
            0,
            r#"{"type":"systemd-tpm2","tpm2-pin":false}"#,
        );
        fake.runner
            .respond("cryptsetup token export --token-id 3 /dev/sda", 1, "");
        // No TPM and no FIDO2 key: every method gives up at once.
        fake.files
            .file("/zfsbootmenu/cmdline.d/zbm.luks_token_wait", "0");
        let mut settings = Settings::new("/zfsbootmenu");
        settings.tpm2_tcti = "device:/dev/none".to_owned();
        let args = [];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, settings, |session| {
            assert!(!session.try_tokens(target, "/dev/sda"));
        });
        let infos: Vec<String> = fake
            .kmsg()
            .lines()
            .filter_map(|line| line.strip_prefix("<6>ZFSBootMenu: "))
            .map(str::to_owned)
            .collect();
        assert_eq!(
            infos,
            [
                "no TPM2 available for token 2 on /dev/sda",
                "no FIDO2 security key for token 0 on /dev/sda",
                "no TPM2 available for token 1 on /dev/sda",
            ]
        );
    }
}
