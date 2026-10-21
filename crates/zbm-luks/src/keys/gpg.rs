//! GnuPG-encrypted key files: `luks_is_gpg_keyfile`, `luks_gpg_setup`,
//! `luks_gpg_cleanup`, `luks_gpg_card_status`, `luks_gpg_recipients`,
//! `luks_gpg_secret_key`, `luks_gpg_have_key` and `luks_gpg_decrypt`.
//!
//! The key file is decrypted with a private GnuPG home that holds the image's
//! public keys. A key file encrypted to a public key is decrypted by a smart
//! card (YubiKey, Nitrokey, ...) or a secret key in the image, a symmetric one
//! with a passphrase. The passphrase or card PIN reaches `gpg` on standard
//! input (loopback pinentry), and the decrypted key is captured from its
//! output as a secret: unlike the legacy code, nothing decrypted is written to
//! a file.

use std::path::Path;

use zbm_core::exec::{Command, Input, Stream};
use zbm_core::secret::Secret;
use zbm_core::shell::command_substitution;
use zbm_system::files::FileKind;

use super::with_newline;
use crate::session::Session;

/// Whether a key file is GnuPG-encrypted (`luks_is_gpg_keyfile`): the `gpg`
/// option, or the name.
pub fn is_gpg_keyfile(path: &str, options: &str) -> bool {
    options.split(',').any(|option| option == "gpg")
        || [".gpg", ".asc", ".pgp"]
            .iter()
            .any(|suffix| path.ends_with(suffix))
}

/// The serial number and the PIN attempts left of the card in
/// `gpg --with-colons --card-status` output, `unknown` and `?` when missing.
pub fn parse_card_status(text: &str) -> (String, String) {
    let field = |record: &str, index: usize| {
        text.lines()
            .map(|line| line.split(':').collect::<Vec<_>>())
            .find(|fields| fields.first() == Some(&record))
            .map(|fields| fields.get(index).copied().unwrap_or_default().to_owned())
            .filter(|value| !value.is_empty())
    };
    // chvstatus:<once>:<max PIN len>:<max reset len>:<max admin len>:<tries>
    (
        field("serial", 1).unwrap_or_else(|| "unknown".to_owned()),
        field("chvstatus", 5).unwrap_or_else(|| "?".to_owned()),
    )
}

/// The long key IDs a file is encrypted to, from `gpg --list-packets` output
/// (none if it is symmetric).
pub fn parse_recipients(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.starts_with(":pubkey enc packet:"))
        .filter_map(|line| {
            let at = line.rfind(" keyid ")?;
            let id: String = line[at + 7..]
                .chars()
                .take_while(char::is_ascii_hexdigit)
                .collect();
            (!id.is_empty()).then_some(id)
        })
        .collect()
}

/// Where the secret key for `id` is, from `gpg --with-colons
/// --list-secret-keys` output: `card` or `soft` (a key stored locally), or
/// `None` without a usable key. Field 15 of `sec` and `ssb` records is `+`
/// for a local key, `#` for a stub whose card is not there, and otherwise the
/// card's serial number.
pub fn parse_secret_key(text: &str, id: &str) -> Option<&'static str> {
    let token = text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        let kind = fields.first().copied().unwrap_or_default();
        let key = fields.get(4).copied().unwrap_or_default();
        ((kind == "sec" || kind == "ssb") && key.eq_ignore_ascii_case(id))
            .then(|| fields.get(14).copied().unwrap_or_default())
    })?;
    match token {
        "+" => Some("soft"),
        "" | "#" => None,
        _ => Some("card"),
    }
}

impl Session<'_> {
    /// `gpg` with the private home.
    fn gpg(&self, home: &str) -> Command {
        self.command("gpg")
            .env("GNUPGHOME", home)
            .args(["--batch", "--no-tty"])
    }

    /// Creates a private GnuPG home with the image's public keys
    /// (`luks_gpg_setup`). The home is in `/run`, with a short path, because
    /// the agent's sockets must fit in a `sockaddr_un`.
    pub fn gpg_setup(&self) -> Option<String> {
        let system = self.system();
        if !system.command_exists("gpg") {
            return None;
        }
        let _ = system.files.create_dir_all(Path::new("/run"));
        let home = self.temporary_dir("/run/zbm-gpg")?;
        let _ = system.files.set_mode(Path::new(&home), 0o700);
        let quiet = |command: Command| {
            let command = command.stdout(Stream::Null).stderr(Stream::Null);
            system
                .runner
                .run(&command)
                .is_ok_and(|output| output.status.success())
        };
        let source = &self.settings().gnupg_dir;
        if system.files.is_dir(source) {
            let from = format!("{}/.", source.display());
            quiet(self.command("cp").args(["-a", &from, &format!("{home}/")]));
        }

        // Start pcscd if the image has it, and prefer it over GnuPG's
        // internal CCID driver once it runs.
        let socket = Path::new("/run/pcscd/pcscd.comm");
        let is_socket = |path: &Path| {
            system
                .files
                .metadata(path)
                .is_ok_and(|metadata| metadata.kind == FileKind::Socket)
        };
        if system.command_exists("pcscd") && !is_socket(socket) {
            let _ = system.files.create_dir_all(Path::new("/run/pcscd"));
            if quiet(self.command("pcscd")) {
                system.clock.sleep(std::time::Duration::from_secs(1));
            }
        }
        if is_socket(socket) {
            let conf = format!("{home}/scdaemon.conf");
            let _ = system
                .files
                .append(Path::new(&conf), b"disable-ccid\npcsc-shared\n");
        }
        quiet(self.command("chmod").args(["-R", "go-rwx", &home]));
        Some(home)
    }

    /// Stops the agents of a GnuPG home and removes it (`luks_gpg_cleanup`).
    pub fn gpg_cleanup(&self, home: &str) {
        let quiet = |command: Command| {
            let command = command.stdout(Stream::Null).stderr(Stream::Null);
            let _ = self.system().runner.run(&command);
        };
        quiet(
            self.command("gpgconf")
                .env("GNUPGHOME", home)
                .args(["--kill", "all"]),
        );
        quiet(self.command("rm").args(["-rf", home]));
    }

    /// The serial number and PIN attempts left of the OpenPGP card that is
    /// plugged in (`luks_gpg_card_status`); `gpg --card-status` also creates
    /// stubs for the card's keys whose public keys are known.
    pub fn gpg_card_status(&self, home: &str) -> Option<(String, String)> {
        let command = self
            .gpg(home)
            .args(["--with-colons", "--card-status"])
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let output = self.system().runner.run(&command).ok()?;
        output
            .status
            .success()
            .then(|| parse_card_status(&output.stdout_text()))
    }

    /// The long key IDs `file` is encrypted to (`luks_gpg_recipients`).
    pub fn gpg_recipients(&self, home: &str, file: &str) -> Vec<String> {
        let command = self
            .gpg(home)
            .args(["--list-only", "--list-packets", file])
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let text = self
            .system()
            .runner
            .run(&command)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        parse_recipients(&text)
    }

    /// Where the secret key for `id` is (`luks_gpg_secret_key`).
    pub fn gpg_secret_key(&self, home: &str, id: &str) -> Option<&'static str> {
        let command = self
            .gpg(home)
            .args(["--with-colons", "--list-secret-keys"])
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let text = self
            .system()
            .runner
            .run(&command)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        parse_secret_key(&text, id)
    }

    /// Whether a secret key for any of `recipients` is available, after
    /// reading any card that is plugged in (`luks_gpg_have_key`).
    pub fn gpg_have_key(&self, home: &str, recipients: &[String]) -> bool {
        let _ = self.gpg_card_status(home);
        recipients
            .iter()
            .any(|id| self.gpg_secret_key(home, id).is_some())
    }

    /// Decrypts a GnuPG-encrypted key file for the volume `description`
    /// (`luks_gpg_decrypt`). Card PINs are asked for only once the card is
    /// known to hold a matching key, with the attempts left, so no attempt is
    /// wasted.
    pub fn gpg_decrypt(&self, file: &str, description: &str) -> Option<Secret> {
        let Some(home) = self.gpg_setup() else {
            self.log()
                .warning(format!("GnuPG is not available to decrypt {file}"));
            return None;
        };
        let key = self.gpg_decrypt_in(&home, file, description);
        self.gpg_cleanup(&home);
        key
    }

    fn gpg_decrypt_in(&self, home: &str, file: &str, description: &str) -> Option<Secret> {
        let recipients = self.gpg_recipients(home, file);
        let kind = if recipients.is_empty() {
            "symmetric"
        } else {
            // Encrypted to a public key: find the card (or key) that can
            // decrypt it.
            let message = self.paint(
                "cyan",
                &format!(
                    "Insert the OpenPGP security key (YubiKey, Nitrokey, ...) for {}",
                    self.paint("yellow", description)
                ),
            );
            if !self.wait_for(self.token_wait(), &message, || {
                self.gpg_have_key(home, &recipients)
            }) {
                self.log().info(format!(
                    "no secret key for {file} (encrypted to {} )",
                    recipients.join(" ")
                ));
                return None;
            }
            recipients
                .iter()
                .find_map(|id| self.gpg_secret_key(home, id))
                .unwrap_or("soft")
        };

        for _ in 0..3 {
            self.say("\n");
            let secret = match kind {
                "card" => {
                    let (serial, left) = self
                        .gpg_card_status(home)
                        .unwrap_or_else(|| ("unknown".to_owned(), "?".to_owned()));
                    let line = format!(
                        "Unlock {} with OpenPGP card {serial}\n",
                        self.paint("yellow", description)
                    );
                    self.say(&self.paint("cyan", &line));
                    if left == "0" {
                        self.say(&self.paint(
                            "red",
                            "The card PIN is blocked; unblock it with the admin PIN or reset \
                             code.\n",
                        ));
                        break;
                    }
                    self.read_secret(&format!(
                        "Card PIN ({left} attempts left; leave empty to use another method): "
                    ))
                }
                _ => {
                    let line = format!(
                        "Unlock {} with GnuPG key file {file}\n",
                        self.paint("yellow", description)
                    );
                    self.say(&self.paint("cyan", &line));
                    self.read_secret(if kind == "soft" {
                        "Secret key passphrase (leave empty to use another method): "
                    } else {
                        "Key file passphrase (leave empty to use another method): "
                    })
                }
            };
            let Some(secret) = secret.filter(|secret| !secret.is_empty()) else {
                break;
            };
            if kind == "card" {
                self.say(&self.paint("yellow", "Touch the security key if it blinks.\n"));
            }

            let decrypt = self
                .gpg(home)
                .args([
                    "--yes",
                    "--pinentry-mode",
                    "loopback",
                    "--passphrase-fd",
                    "0",
                    "--status-fd",
                    "2",
                    "--output",
                    "-",
                    "--decrypt",
                    file,
                ])
                .stdin(Input::Secret(with_newline(&secret)))
                .stdout(Stream::CaptureSecret)
                .stderr(Stream::Capture);
            drop(secret);
            let (status, key) = match self.system().runner.run(&decrypt) {
                Ok(output) => (output.stderr_text(), output.secret),
                Err(error) => (error.to_string(), None),
            };
            let okay = status
                .lines()
                .any(|line| line.starts_with("[GNUPG:] DECRYPTION_OKAY"));
            if let Some(key) = key.filter(|key| okay && !key.is_empty()) {
                return Some(key);
            }
            self.log().debug(format!(
                "gpg failed to decrypt {file}: {}",
                command_substitution(&status)
            ));
            self.say(&self.paint("red", "Decryption failed.\n"));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::session::Settings;
    use zbm_core::exec::{Output, Status};
    use zbm_testutil::fakes::FakeSystem;

    #[test]
    fn gpg_key_files_are_known_by_name_or_option() {
        assert!(is_gpg_keyfile("/k.gpg", ""));
        assert!(is_gpg_keyfile("/k.asc", ""));
        assert!(is_gpg_keyfile("/k.pgp", "discard"));
        assert!(is_gpg_keyfile("/k.bin", "discard,gpg"));
        assert!(!is_gpg_keyfile("/k.bin", "gpgx,no-gpg"));
        assert!(!is_gpg_keyfile("/k.gpg.bin", ""));
    }

    #[test]
    fn gpg_output_is_read_as_the_legacy_pipelines_read_it() {
        let card = "Reader:Yubico YubiKey:AID:D2760001240103040006123456780000:openpgp-card\n\
                    serial:12345678:\n\
                    chvstatus:1:127:127:127:3:0:3:\n\
                    serial:other:\n";
        assert_eq!(
            parse_card_status(card),
            ("12345678".to_owned(), "3".to_owned())
        );
        assert_eq!(
            parse_card_status("version:1\n"),
            ("unknown".to_owned(), "?".to_owned())
        );
        let packets = ":pubkey enc packet: version 3, algo 18, keyid 1A2B3C4D5E6F7081\n\
                       \tdata: [263 bits]\n\
                       :pubkey enc packet: version 3, algo 18, keyid aabbccddeeff0011\n\
                       :symkey enc packet: version 4, cipher 9\n\
                       :pubkey enc packet: no id\n";
        assert_eq!(
            parse_recipients(packets),
            ["1A2B3C4D5E6F7081", "aabbccddeeff0011"]
        );
        let keys = "sec:u:255:22:1111111111111111:1:::u:::scESC:::#:::23::0:\n\
                    ssb:u:255:18:2222222222222222:1::::::e:::D27600012401030400:::23:\n\
                    ssb:u:255:18:3333333333333333:1::::::e:::+:::23:\n\
                    ssb:u:255:18:4444444444444444:1::::::e:::#:::23:\n";
        assert_eq!(parse_secret_key(keys, "2222222222222222"), Some("card"));
        assert_eq!(parse_secret_key(keys, "3333333333333333"), Some("soft"));
        assert_eq!(parse_secret_key(keys, "4444444444444444"), None);
        assert_eq!(parse_secret_key(keys, "1111111111111111"), None);
        assert_eq!(parse_secret_key(keys, "5555555555555555"), None);
    }

    fn gpg_machine(packets: &'static str, keys: &'static str) -> FakeSystem {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/bin");
        fake.files.file_with_mode("/bin/gpg", "", 0o755);
        fake.files.dir("/run");
        fake.runner.on(move |command| {
            if command.program() != "gpg" {
                return None;
            }
            let args: Vec<String> = command
                .get_args()
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            let has = |arg: &str| args.iter().any(|a| a == arg);
            Some(Ok(if has("--list-packets") {
                Output::new(Status::exited(0), packets)
            } else if has("--list-secret-keys") {
                Output::new(Status::exited(0), keys)
            } else if has("--card-status") {
                Output::new(Status::exited(2), "")
            } else {
                let right = matches!(
                    command.get_stdin(),
                    Input::Secret(pass) if pass.expose() == b"gpgpass\n"
                );
                let mut output = Output::with_secret(
                    Status::exited(if right { 0 } else { 2 }),
                    Secret::copy_from(if right { b"plain-key" } else { b"" }),
                );
                output.stderr = if right {
                    b"[GNUPG:] BEGIN_DECRYPTION\n[GNUPG:] DECRYPTION_OKAY\n".to_vec()
                } else {
                    b"[GNUPG:] DECRYPTION_FAILED\n".to_vec()
                };
                output
            }))
        });
        fake
    }

    #[test]
    fn symmetric_key_files_ask_for_their_passphrase() {
        let fake = gpg_machine("", "");
        fake.console.secret(b"nope").secret(b"gpgpass");
        let key = with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            session.gpg_decrypt("/k1.gpg", "/dev/sda")
        });
        assert_eq!(key.unwrap().expose(), b"plain-key");
        let output = fake.console.output_text();
        assert_eq!(
            output.matches("Key file passphrase (leave empty").count(),
            2
        );
        assert!(output.contains("Decryption failed."));
        let decrypt = fake
            .runner
            .lines()
            .into_iter()
            .find(|line| line.contains("--decrypt"))
            .unwrap();
        assert_eq!(
            decrypt,
            "gpg --batch --no-tty --yes --pinentry-mode loopback --passphrase-fd 0 \
             --status-fd 2 --output - --decrypt /k1.gpg"
        );
        // The home is gone, with its agents.
        let lines = fake.runner.lines();
        assert!(lines.contains(&"gpgconf --kill all".to_owned()));
        assert!(lines.contains(&"rm -rf /run/zbm-gpg.4242-0".to_owned()));
        fake.assert_no_leak(b"gpgpass");
        fake.assert_no_leak(b"plain-key");
    }

    #[test]
    fn public_key_files_need_their_secret_key() {
        let packets = ":pubkey enc packet: version 3, algo 18, keyid 3333333333333333\n";
        let keys = "ssb:u:255:18:3333333333333333:1::::::e:::+:::23:\n";
        let fake = gpg_machine(packets, keys);
        fake.console.secret(b"gpgpass");
        let key = with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            session.gpg_decrypt("/k2.gpg", "/dev/sda")
        });
        assert!(key.is_some());
        assert!(
            fake.console
                .output_text()
                .contains("Secret key passphrase (leave empty")
        );

        // Without the secret key there is nothing to ask.
        let fake = gpg_machine(packets, "");
        fake.files
            .file("/zfsbootmenu/cmdline.d/zbm.luks_token_wait", "0");
        let key = with_session(&fake, Settings::new("/zfsbootmenu"), |session| {
            session.gpg_decrypt("/k2.gpg", "/dev/sda")
        });
        assert!(key.is_none());
        assert!(!fake.console.output_text().contains("passphrase"));
        assert!(fake.kmsg().contains("no secret key for /k2.gpg"));
    }
}
