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
pub mod handlers;
pub mod tpm2;
pub mod yubikey;

use std::path::Path;

use zbm_crypt::{CryptError, Cryptsetup, Token};
use zbm_platform::secret::Secret;

use crate::error::{LuksError, TokenFailure};
use crate::unlocker::Unlocker;
use handlers::Pass;

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

impl Target<'_> {
    /// The error of the `kind` token `id` on this volume failing for
    /// `failure`.
    pub fn token_error(&self, kind: &str, id: &str, failure: TokenFailure) -> LuksError {
        LuksError::Token {
            kind: kind.to_owned(),
            id: id.to_owned(),
            device: self.device.to_owned(),
            failure,
        }
    }
}

impl Unlocker<'_> {
    /// Whether cryptsetup's token plugin for `kind` is installed; never when
    /// the settings force ZFSBootMenu's own implementations.
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

    /// `cryptsetup`, with device-mapper's udev synchronization off when
    /// [`Unlocker::available`] turned it off.
    pub fn cryptsetup(&self) -> Cryptsetup<'_> {
        Cryptsetup::new(self.system().runner).without_udev(self.dm_disable_udev)
    }

    /// Opens a volume through cryptsetup's token plugin; cryptsetup reads a
    /// PIN from standard input, followed by a newline.
    ///
    /// # Errors
    ///
    /// [`LuksError::Open`] with cryptsetup's exit status.
    pub fn open_token_plugin(
        &self,
        target: Target<'_>,
        id: &str,
        pin: Option<&Secret>,
    ) -> Result<(), LuksError> {
        let result =
            self.cryptsetup()
                .open_with_token(target.device, target.name, id, target.args, pin);
        let (status, output) = match result {
            Ok(()) => return Ok(()),
            Err(CryptError::Failed { status, output }) => (status, output),
            Err(error) => (127, error.to_string()),
        };
        zbm_trace::debug!(
            "token {id} on {} failed ({status}): {output}",
            target.device
        );
        Err(LuksError::Open {
            device: target.device.to_owned(),
            name: target.name.to_owned(),
            status,
        })
    }

    /// Tries the LUKS2 tokens of a volume whose header is `header`, with the
    /// handlers of [`handlers::token_handlers`]: first those that need no
    /// interaction (TPM2 without a PIN, clevis), then those that do (TPM2
    /// with a PIN, FIDO2, and any other type that has a plugin in the
    /// image).
    ///
    /// # Errors
    ///
    /// [`LuksError::NoToken`] when no token opened the volume; each token's
    /// own failure is logged by its method.
    pub fn try_tokens(&mut self, target: Target<'_>, header: &str) -> Result<(), LuksError> {
        let none_opened = || LuksError::NoToken {
            device: target.device.to_owned(),
        };
        let tokens: Vec<Token> = self
            .cryptsetup()
            .tokens(header)
            .into_iter()
            .filter(Token::has_fields)
            .collect();
        if tokens.is_empty() {
            return Err(none_opened());
        }
        let handlers = handlers::token_handlers();
        for pass in Pass::ALL {
            for token in &tokens {
                let handler = handlers.iter().find(|handler| handler.kind() == token.kind);
                let result = match handler {
                    Some(handler) if handler.pass(token) == pass => {
                        handler.try_open(self, target, token)
                    }
                    None if pass == Pass::Interactive && self.have_token_plugin(&token.kind) => {
                        self.open_token_plugin(target, &token.id, None)
                    }
                    None if pass == Pass::Interactive => {
                        zbm_trace::debug!(
                            "no support for {} token {} on {}",
                            token.kind,
                            token.id,
                            target.device
                        );
                        continue;
                    }
                    // Tried in the other pass, or not at all.
                    Some(_) | None => continue,
                };
                if result.is_ok() {
                    return Ok(());
                }
            }
        }
        Err(none_opened())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::testing::{NoPrompts, log};
    use crate::unlocker::{Luks, Settings};
    use zbm_platform::exec::{Input, Output, Status};
    use zbm_testkit::fakes::FakeSystem;

    /// A token from its JSON, typed `test` when the JSON has no type.
    pub(crate) fn token(json: &str) -> Token {
        let typed = if json.contains("\"type\"") {
            json.to_owned()
        } else {
            let separator = if json.trim() == "{}" { "" } else { "," };
            json.replacen('{', &format!("{{\"type\":\"test\"{separator}"), 1)
        };
        Token::from_export("0", &typed).unwrap()
    }

    pub(crate) fn with_session<T>(
        fake: &FakeSystem,
        settings: &Settings,
        body: impl FnOnce(&mut Unlocker<'_>) -> T,
    ) -> T {
        let _log = log(fake);
        let luks = Luks {
            system: fake.system(),
            settings,
            prompter: &NoPrompts,
        };
        body(&mut Unlocker::new(luks))
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
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert!(session.have_token_plugin("systemd-tpm2"));
            assert!(session.have_token_plugin("ssh"));
            assert!(!session.have_token_plugin("systemd-fido2"));
        });
        let mut settings = Settings::new("/zfsbootmenu");
        settings.token_plugins = false;
        with_session(&fake, &settings, |session| {
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
                Status::exited(u8::from(!pin_ok)),
                "No usable token is available.\n",
            )))
        });
        let args = ["--readonly".to_owned()];
        let target = Target {
            name: "v",
            device: "/dev/sda",
            args: &args,
        };
        with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            assert_eq!(
                session.open_token_plugin(target, "0", Some(&Secret::copy_from(b"1234"))),
                Ok(())
            );
            assert!(session.open_token_plugin(target, "1", None).is_err());
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
        with_session(&fake, &settings, |session| {
            assert!(session.try_tokens(target, "/dev/sda").is_err());
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
