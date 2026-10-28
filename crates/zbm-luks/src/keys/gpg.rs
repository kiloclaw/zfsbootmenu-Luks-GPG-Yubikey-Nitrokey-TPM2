//! GnuPG-encrypted key files: recognizing one, setting up a private GnuPG
//! home, finding the card or secret key a key file is encrypted to, and
//! decrypting it.
//!
//! The key file is decrypted with a private GnuPG home that holds the image's
//! public keys. A key file encrypted to a public key is decrypted by a smart
//! card (YubiKey, Nitrokey, ...) or a secret key in the image, a symmetric one
//! with a passphrase. The passphrase or card PIN reaches `gpg` on standard
//! input (loopback pinentry), and the decrypted key is captured from its
//! output as a secret: unlike the legacy code, nothing decrypted is written to
//! a file.

use std::path::Path;

use zbm_crypt::gpg::{Decrypted, GnupgHome, start_pcscd};
use zbm_platform::files::FileKind;
use zbm_platform::secret::Secret;

use crate::unlocker::Unlocker;
use zbm_platform::interaction::Wait;
use zbm_platform::interaction::{Awaited, SecretKind, SecretRequest, Told, VolumeRef, WaitRequest};

/// How many times a card PIN or a key file passphrase is asked for.
const GPG_TRIES: u32 = 3;

/// Whether a key file is GnuPG-encrypted: the `gpg` option, or the name.
pub fn is_gpg_keyfile(path: &str, options: &str) -> bool {
    options.split(',').any(|option| option == "gpg")
        || [".gpg", ".asc", ".pgp"]
            .iter()
            .any(|suffix| path.ends_with(suffix))
}

pub use zbm_crypt::gpg::{SecretKey, parse_card_status, parse_recipients, parse_secret_key};

impl Unlocker<'_> {
    /// Creates a private GnuPG home with the image's public keys; it is
    /// cleaned up when dropped. The home is in `/run`, with a short path,
    /// because the agent's sockets must fit in a `sockaddr_un`.
    pub fn gpg_setup(&self) -> Option<GnupgHome<'_>> {
        let system = self.system();
        if !system.command_exists("gpg") {
            return None;
        }
        let _ = system.files.create_dir_all(Path::new("/run"));
        let home = GnupgHome::new(
            system.runner,
            system.files,
            self.temporary_dir("/run/zbm-gpg")?,
        );
        let _ = system.files.set_mode(home.path(), 0o700);
        let source = &self.settings().gnupg_dir;
        if system.files.is_dir(source) {
            home.import(source);
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
            if start_pcscd(system.runner).is_ok() {
                system.clock.sleep(std::time::Duration::from_secs(1));
            }
        }
        if is_socket(socket) {
            let conf = home.path().join("scdaemon.conf");
            let _ = system.files.append(&conf, b"disable-ccid\npcsc-shared\n");
        }
        home.make_private();
        Some(home)
    }

    /// Whether a secret key for any of `recipients` is available, after
    /// reading any card that is plugged in.
    pub fn gpg_have_key(&self, home: &GnupgHome<'_>, recipients: &[String]) -> bool {
        let _ = home.card_status();
        recipients.iter().any(|id| home.secret_key(id).is_some())
    }

    /// Decrypts a GnuPG-encrypted key file for `volume`. Card PINs are asked
    /// for only once the card is known to hold a matching key, with the
    /// attempts left, so no attempt is wasted.
    pub fn gpg_decrypt(&self, file: &str, volume: VolumeRef<'_>) -> Option<Secret> {
        let Some(home) = self.gpg_setup() else {
            zbm_trace::warn!("GnuPG is not available to decrypt {file}");
            return None;
        };
        self.gpg_decrypt_in(&home, file, volume)
    }

    /// Decrypts `file` with the GnuPG home `home`, asking for the passphrase
    /// or card PIN up to three times.
    fn gpg_decrypt_in(
        &self,
        home: &GnupgHome<'_>,
        file: &str,
        volume: VolumeRef<'_>,
    ) -> Option<Secret> {
        let decryptor = self.gpg_decryptor(home, file, volume)?;
        let card = decryptor == Decryptor::Key(SecretKey::Card);
        for attempt in 1..=GPG_TRIES {
            let secret = if card {
                self.ask_card_pin(home, volume, attempt)
            } else {
                self.ask_gpg_passphrase(decryptor, file, volume, attempt)
            };
            let secret = secret.filter(|secret| !secret.is_empty())?;
            if card {
                let touch = "Touch the security key if it blinks.\n";
                self.tell(Told::Touch, touch);
            }
            match home.decrypt(file, &secret) {
                Decrypted::Key(key) => return Some(key),
                Decrypted::Failed(status) => {
                    zbm_trace::debug!(
                        "gpg failed to decrypt {file}: {}",
                        status.trim_end_matches('\n')
                    );
                }
            }
            self.tell(Told::Refused, "Decryption failed.\n");
        }
        None
    }

    /// What decrypts `file`: a passphrase, or a key the image has (on a
    /// card, after waiting for one, or a secret key); `None` when nothing
    /// can.
    fn gpg_decryptor(
        &self,
        home: &GnupgHome<'_>,
        file: &str,
        volume: VolumeRef<'_>,
    ) -> Option<Decryptor> {
        let recipients = home.recipients(file);
        if recipients.is_empty() {
            return Some(Decryptor::Passphrase);
        }
        // Encrypted to a public key: find the card (or key) that can decrypt
        // it.
        let request = WaitRequest {
            volume,
            awaited: Awaited::OpenPgpKey,
            seconds: self.token_wait(),
            present: &|| self.gpg_have_key(home, &recipients),
        };
        if self.await_device(&request) != Wait::Present {
            zbm_trace::info!(
                "no secret key for {file} (encrypted to {} )",
                recipients.join(" ")
            );
            return None;
        }
        let kind = recipients
            .iter()
            .find_map(|id| home.secret_key(id))
            .unwrap_or(SecretKey::Soft);
        Some(Decryptor::Key(kind))
    }

    /// Asks for the PIN of the OpenPGP card, attempt `attempt`; `None` when
    /// the PIN is blocked, which ends the attempts, as no PIN does.
    fn ask_card_pin(
        &self,
        home: &GnupgHome<'_>,
        volume: VolumeRef<'_>,
        attempt: u32,
    ) -> Option<Secret> {
        let (serial, left) = home
            .card_status()
            .unwrap_or_else(|| ("unknown".to_owned(), "?".to_owned()));
        if left == "0" {
            let device = volume.device;
            self.heading(
                &format!("Unlock {device} with OpenPGP card {serial}"),
                device,
            );
            let blocked = "The card PIN is blocked; unblock it with the admin PIN or reset code.";
            self.tell(Told::Refused, blocked);
            return None;
        }
        let request = SecretRequest {
            volume,
            kind: SecretKind::CardPin { serial, left },
            attempt,
            tries: Some(GPG_TRIES),
            opened_elsewhere: None,
            description: None,
        };
        self.ask(&request).secret()
    }

    /// Asks for the passphrase of the secret key or of the symmetrically
    /// encrypted key file, attempt `attempt`.
    fn ask_gpg_passphrase(
        &self,
        decryptor: Decryptor,
        file: &str,
        volume: VolumeRef<'_>,
        attempt: u32,
    ) -> Option<Secret> {
        let kind = if decryptor == Decryptor::Key(SecretKey::Soft) {
            SecretKind::SecretKeyPassphrase(file.to_owned())
        } else {
            SecretKind::KeyFilePassphrase(file.to_owned())
        };
        let request = SecretRequest {
            volume,
            kind,
            attempt,
            tries: Some(GPG_TRIES),
            opened_elsewhere: None,
            description: None,
        };
        self.ask(&request).secret()
    }
}

/// What decrypts a GnuPG-encrypted key file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Decryptor {
    /// A passphrase: the file is encrypted symmetrically.
    Passphrase,
    /// A secret key the image has, on a card or not.
    Key(SecretKey),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::tests::with_session;
    use crate::unlocker::Settings;
    use zbm_platform::exec::Input;
    use zbm_platform::exec::{Output, Status};
    use zbm_testkit::fakes::FakeSystem;

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
        assert_eq!(
            parse_secret_key(keys, "2222222222222222"),
            Some(SecretKey::Card)
        );
        assert_eq!(
            parse_secret_key(keys, "3333333333333333"),
            Some(SecretKey::Soft)
        );
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
        let key = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            session.gpg_decrypt("/k1.gpg", crate::testing::VOLUME)
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
        assert!(
            !fake
                .files
                .paths()
                .iter()
                .any(|path| path.starts_with("/run/zbm-gpg.4242-0"))
        );
        fake.assert_no_leak(b"gpgpass");
        fake.assert_no_leak(b"plain-key");
    }

    #[test]
    fn public_key_files_need_their_secret_key() {
        let packets = ":pubkey enc packet: version 3, algo 18, keyid 3333333333333333\n";
        let keys = "ssb:u:255:18:3333333333333333:1::::::e:::+:::23:\n";
        let fake = gpg_machine(packets, keys);
        fake.console.secret(b"gpgpass");
        let key = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            session.gpg_decrypt("/k2.gpg", crate::testing::VOLUME)
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
        let key = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            session.gpg_decrypt("/k2.gpg", crate::testing::VOLUME)
        });
        assert!(key.is_none());
        assert!(!fake.console.output_text().contains("passphrase"));
        assert!(fake.kmsg().contains("no secret key for /k2.gpg"));
    }
}
