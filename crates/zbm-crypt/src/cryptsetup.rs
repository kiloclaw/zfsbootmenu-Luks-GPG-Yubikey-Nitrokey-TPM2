//! `cryptsetup`: opening and closing volumes, and reading their tokens.

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use tracing::{debug, info_span};
use zbm_platform::{Command, Input, Output, Runner, Secret, Stream};
use zbm_types::{DevicePath, MappingName};

use crate::CryptError;
use crate::mapping::{Closed, Failed, Mapping, Open};
use crate::token::{self, Token};

/// How [`Cryptsetup::open`] opens a volume, beyond the key.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OpenOptions {
    /// Pass discard (TRIM) requests through (`--allow-discards`).
    pub allow_discards: bool,
    /// Open the mapping read-only (`--readonly`).
    pub read_only: bool,
}

/// Where `cryptsetup open` gets the key.
#[derive(Clone, Copy, Debug)]
pub enum KeySource<'k> {
    /// A key file on a mounted file system.
    File(&'k str),
    /// A secret, given on standard input.
    Secret(&'k Secret),
}

/// `cryptsetup`, run through a [`Runner`].
#[derive(Clone, Copy)]
pub struct Cryptsetup<'a> {
    /// Where the program runs.
    runner: &'a dyn Runner,
    /// Whether device-mapper skips udev synchronization
    /// (`DM_DISABLE_UDEV=1`), for images without its udev rules.
    without_udev: bool,
}

impl<'a> Cryptsetup<'a> {
    /// `cryptsetup`, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Cryptsetup<'a> {
        Cryptsetup {
            runner,
            without_udev: false,
        }
    }

    /// The same, with device-mapper's udev synchronization off when
    /// `without_udev` is set (`DM_DISABLE_UDEV=1`).
    #[must_use]
    pub fn without_udev(mut self, without_udev: bool) -> Cryptsetup<'a> {
        self.without_udev = without_udev;
        self
    }

    /// A `cryptsetup` command with `args`, standard input from `/dev/null`.
    fn command(&self, args: &[&str]) -> Command {
        let command = Command::new("cryptsetup")
            .args(args.iter().copied())
            .stdin(Input::Null);
        if self.without_udev {
            command.env("DM_DISABLE_UDEV", "1")
        } else {
            command
        }
    }

    /// What `cryptsetup <args>` prints on standard output, if it succeeds;
    /// its errors are discarded.
    fn quietly(&self, args: &[&str]) -> Option<String> {
        let command = self
            .command(args)
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let output = self.runner.run(&command).ok()?;
        output.status.is_success().then(|| output.stdout_text())
    }

    /// The LUKS2 tokens of a volume or detached header: from `cryptsetup
    /// luksDump --dump-json-metadata`, with their fields, or, for cryptsetup
    /// before 2.4, from the `luksDump` text, without them. A volume that
    /// cannot be read has no tokens.
    pub fn list_tokens(&self, header: &str) -> Vec<Token> {
        let metadata = self.quietly(&["luksDump", "--dump-json-metadata", header]);
        if let Some(Ok(tokens)) = metadata.as_deref().map(token::from_metadata) {
            return tokens;
        }
        let text = self.quietly(&["luksDump", header]).unwrap_or_default();
        token::from_dump_text(&text)
    }

    /// The LUKS2 tokens of a volume or detached header with their fields,
    /// which `cryptsetup token export` reads for the tokens that
    /// [`Cryptsetup::list_tokens`] found without them.
    pub fn tokens(&self, header: &str) -> Vec<Token> {
        self.list_tokens(header)
            .into_iter()
            .map(|listed| {
                if listed.has_fields() {
                    listed
                } else {
                    self.token(header, &listed.id).unwrap_or(listed)
                }
            })
            .collect()
    }

    /// One token with its fields (`cryptsetup token export`), or `None` when
    /// it cannot be read.
    pub fn token(&self, header: &str, id: &str) -> Option<Token> {
        let text = self.quietly(&["token", "export", "--token-id", id, header])?;
        Token::from_export(id, &text).ok()
    }

    /// Whether `header` is a LUKS volume or detached header
    /// (`cryptsetup isLuks`).
    pub fn is_luks(&self, header: &str) -> bool {
        let command = self
            .command(&["isLuks", header])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.runner
            .run(&command)
            .is_ok_and(|output| output.status.is_success())
    }

    /// The UUID of the LUKS volume `device` (`cryptsetup luksUUID`), or
    /// `None` when it cannot be read.
    pub fn luks_uuid(&self, device: &str) -> Option<String> {
        self.quietly(&["luksUUID", device])
            .map(|text| text.trim_end_matches('\n').to_owned())
    }

    /// Opens `device` as `name` with `key`, and `options` before the device:
    /// `cryptsetup open --type luks --key-file=<file or -> [options] DEVICE
    /// NAME`, for volumes described by text, as in a crypttab.
    ///
    /// # Errors
    ///
    /// [`CryptError::Failed`] with cryptsetup's exit status and output, and
    /// [`CryptError::Start`] when it cannot be started.
    pub fn open_described(
        &self,
        device: &str,
        name: &str,
        key: KeySource<'_>,
        options: &[String],
    ) -> Result<(), CryptError> {
        let (key_file, stdin) = match key {
            KeySource::File(path) => (path, Input::Null),
            KeySource::Secret(secret) => ("-", Input::Secret(secret.clone())),
        };
        let key_option = format!("--key-file={key_file}");
        let command = self
            .command(&["open", "--type", "luks", &key_option])
            .args(options.iter().map(String::as_str))
            .args([device, name])
            .stdin(stdin)
            .stdout(Stream::Capture)
            .merge_stderr();
        self.finish(&command)
    }

    /// Closes the mapping `name` (`cryptsetup close`), quietly.
    ///
    /// # Errors
    ///
    /// [`CryptError::Failed`] when cryptsetup fails, for example because the
    /// mapping is in use, and [`CryptError::Start`] when it cannot be
    /// started.
    pub fn close_named(&self, name: &str) -> Result<(), CryptError> {
        let command = self
            .command(&["close", name])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.finish(&command)
    }

    /// Runs `command`; a failure is [`CryptError::Failed`] with its status
    /// and captured output.
    fn finish(&self, command: &Command) -> Result<(), CryptError> {
        let output = self.runner.run(command).map_err(CryptError::Start)?;
        if output.status.is_success() {
            return Ok(());
        }
        Err(CryptError::Failed {
            status: output.status.shell_code(),
            output: output.stdout_text().trim_end_matches('\n').to_owned(),
        })
    }

    /// Opens `device` as `name` with the token `id` through cryptsetup's
    /// token plugin (`--token-only`), with `options` before the device; a
    /// PIN goes to standard input, followed by a newline.
    ///
    /// # Errors
    ///
    /// [`CryptError::Failed`] with cryptsetup's exit status and output, and
    /// [`CryptError::Start`] when it cannot be started.
    pub fn open_with_token(
        &self,
        device: &str,
        name: &str,
        id: &str,
        options: &[String],
        pin: Option<&Secret>,
    ) -> Result<(), CryptError> {
        let stdin = pin.map_or(Input::Null, |pin| {
            let mut line = pin.clone();
            line.push(b'\n');
            Input::Secret(line)
        });
        let command = self
            .command(&["open", "--type", "luks", "--token-only", "--token-id", id])
            .args(options.iter().map(String::as_str))
            .args([device, name])
            .stdin(stdin)
            .stdout(Stream::Capture)
            .merge_stderr();
        self.finish(&command)
    }

    /// Opens `mapping` with `key`:
    /// `cryptsetup open --type luks --key-file=- [options] DEVICE NAME`, with
    /// the key on standard input.
    ///
    /// # Errors
    ///
    /// [`Failed`] with the closed mapping and a [`CryptError`]:
    /// [`CryptError::WrongKey`] when no key slot accepts the key, and the
    /// other variants as their documentation says.
    pub fn open(
        &self,
        mapping: Mapping<Closed>,
        key: &Secret,
        options: OpenOptions,
    ) -> Result<Mapping<Open>, Failed<Closed>> {
        let _span =
            info_span!("cryptsetup_open", device = %mapping.device(), name = %mapping.name())
                .entered();
        let mut command = self.command(&["open", "--type", "luks", "--key-file=-"]);
        if options.allow_discards {
            command = command.arg("--allow-discards");
        }
        if options.read_only {
            command = command.arg("--readonly");
        }
        let command = command
            .args([mapping.device().as_str(), mapping.name().as_str()])
            .stdin(Input::Secret(key.clone()));
        match self.run(command, mapping.device(), mapping.name()) {
            Ok(()) => Ok(mapping.into_state()),
            Err(error) => Err(Failed { mapping, error }),
        }
    }

    /// Closes `mapping`: `cryptsetup close NAME`.
    ///
    /// # Errors
    ///
    /// [`Failed`] with the mapping, still open, and a [`CryptError`]:
    /// [`CryptError::Busy`] when something still uses it.
    pub fn close(&self, mapping: Mapping<Open>) -> Result<Mapping<Closed>, Failed<Open>> {
        let _span = info_span!("cryptsetup_close", name = %mapping.name()).entered();
        let command = self.command(&["close", mapping.name().as_str()]);
        match self.run(command, mapping.device(), mapping.name()) {
            Ok(()) => Ok(mapping.into_state()),
            Err(error) => Err(Failed { mapping, error }),
        }
    }

    /// Runs `command` with its output captured, and turns a failure into the
    /// [`CryptError`] that its exit status means for the mapping `name` of
    /// `device`.
    fn run(
        &self,
        command: Command,
        device: &DevicePath,
        name: &MappingName,
    ) -> Result<(), CryptError> {
        let command = command.stdout(Stream::Capture).merge_stderr();
        let output = self.runner.run(&command).map_err(CryptError::Start)?;
        if output.status.is_success() {
            return Ok(());
        }
        let error = classify(&output, device, name);
        debug!(%error, "cryptsetup failed");
        Err(error)
    }
}

/// The error that a failed `cryptsetup` run means, from its exit status
/// (`cryptsetup(8)`, "RETURN CODES").
fn classify(output: &Output, device: &DevicePath, name: &MappingName) -> CryptError {
    match output.status.shell_code() {
        2 => CryptError::WrongKey {
            device: device.clone(),
        },
        4 => CryptError::NoDevice {
            device: device.clone(),
        },
        5 => CryptError::Busy { name: name.clone() },
        127 => CryptError::NotInstalled,
        status => CryptError::Failed {
            status,
            output: output.stdout_text().trim().to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_testkit::FakeRunner;

    fn volume() -> Mapping<Closed> {
        Mapping::new("/dev/sda2".parse().unwrap(), "cryptroot".parse().unwrap())
    }

    fn key() -> Secret {
        Secret::copy_from(b"correct horse")
    }

    #[test]
    fn open_passes_the_key_on_standard_input_only() {
        let runner = FakeRunner::new();

        let open = Cryptsetup::new(&runner)
            .open(volume(), &key(), OpenOptions::default())
            .unwrap();

        assert_eq!(open.path(), "/dev/mapper/cryptroot");
        let calls = runner.calls();
        assert_eq!(
            calls[0].display(),
            "cryptsetup open --type luks --key-file=- /dev/sda2 cryptroot"
        );
        assert_eq!(calls[0].get_stdin(), &Input::Secret(key()));
        assert!(!calls[0].display().contains("correct horse"));
    }

    #[test]
    fn options_become_flags_before_the_device() {
        let runner = FakeRunner::new();
        let options = OpenOptions {
            allow_discards: true,
            read_only: true,
        };

        Cryptsetup::new(&runner)
            .open(volume(), &key(), options)
            .unwrap();

        assert_eq!(
            runner.lines(),
            [concat!(
                "cryptsetup open --type luks --key-file=- ",
                "--allow-discards --readonly /dev/sda2 cryptroot"
            )]
        );
    }

    #[test]
    fn exit_statuses_become_errors_that_say_what_happened() {
        let cases = [
            (2, "no key slot of /dev/sda2 accepts the key"),
            (4, "/dev/sda2 does not exist or is not a LUKS volume"),
            (5, "the mapping cryptroot exists already or is busy"),
            (127, "cryptsetup is not installed"),
            (1, "cryptsetup failed with status 1: bad option"),
        ];

        for (status, message) in cases {
            let runner = FakeRunner::new();
            runner.respond_program("cryptsetup", status, "bad option\n");

            let failed = Cryptsetup::new(&runner)
                .open(volume(), &key(), OpenOptions::default())
                .unwrap_err();

            assert_eq!(failed.error.to_string(), message, "status {status}");
        }
    }

    #[test]
    fn a_failed_open_hands_the_closed_mapping_back() {
        let runner = FakeRunner::new();
        runner.respond_program("cryptsetup", 2, "");

        let failed = Cryptsetup::new(&runner)
            .open(volume(), &key(), OpenOptions::default())
            .unwrap_err();

        assert_eq!(failed.mapping, volume());
    }

    #[test]
    fn close_runs_cryptsetup_close_and_returns_the_closed_mapping() {
        let runner = FakeRunner::new();
        let cryptsetup = Cryptsetup::new(&runner);
        let open = cryptsetup
            .open(volume(), &key(), OpenOptions::default())
            .unwrap();

        let closed = cryptsetup.close(open).unwrap();

        assert_eq!(closed, volume());
        assert_eq!(runner.lines()[1], "cryptsetup close cryptroot");
    }

    #[test]
    fn a_busy_mapping_stays_open() {
        let runner = FakeRunner::new();
        let cryptsetup = Cryptsetup::new(&runner);
        let open = cryptsetup
            .open(volume(), &key(), OpenOptions::default())
            .unwrap();
        runner.respond("cryptsetup close cryptroot", 5, "");

        let failed = cryptsetup.close(open).unwrap_err();

        assert_eq!(failed.mapping.path(), "/dev/mapper/cryptroot");
        assert!(matches!(failed.error, CryptError::Busy { .. }));
    }

    #[test]
    fn described_volumes_open_with_a_key_file_or_a_secret() {
        let runner = FakeRunner::new();
        runner.respond(
            "cryptsetup open --type luks --key-file=- --readonly /dev/sdb2 data",
            2,
            "No key available with this passphrase.\n\n",
        );
        let cryptsetup = Cryptsetup::new(&runner).without_udev(true);
        let options = ["--readonly".to_owned()];

        cryptsetup
            .open_described("/dev/sda2", "root", KeySource::File("/key"), &[])
            .unwrap();
        let error = cryptsetup
            .open_described("/dev/sdb2", "data", KeySource::Secret(&key()), &options)
            .unwrap_err();

        assert!(matches!(
            error,
            CryptError::Failed { status: 2, ref output }
                if output == "No key available with this passphrase."
        ));
        let calls = runner.calls();
        assert_eq!(
            calls[0].display(),
            "cryptsetup open --type luks --key-file=/key /dev/sda2 root"
        );
        assert_eq!(*calls[0].get_stdin(), Input::Null);
        assert_eq!(*calls[1].get_stdin(), Input::Secret(key()));
        assert_eq!(
            calls[1].get_env(),
            [("DM_DISABLE_UDEV".into(), Some("1".into()))]
        );
    }

    #[test]
    fn headers_names_and_mappings_are_probed_quietly() {
        let runner = FakeRunner::new();
        runner.respond("cryptsetup isLuks /dev/sdc", 1, "");
        runner.respond("cryptsetup luksUUID /dev/sda2", 0, "0b5c-41\n");
        runner.respond("cryptsetup luksUUID /dev/sdc", 1, "");
        runner.respond("cryptsetup close busy", 5, "");
        let cryptsetup = Cryptsetup::new(&runner);

        assert!(cryptsetup.is_luks("/dev/sda2"));
        assert!(!cryptsetup.is_luks("/dev/sdc"));
        assert_eq!(
            cryptsetup.luks_uuid("/dev/sda2").as_deref(),
            Some("0b5c-41")
        );
        assert_eq!(cryptsetup.luks_uuid("/dev/sdc"), None);
        assert!(cryptsetup.close_named("root").is_ok());
        let busy = cryptsetup.close_named("busy");
        assert!(matches!(busy, Err(CryptError::Failed { status: 5, .. })));
    }

    #[test]
    fn tokens_from_json_metadata_need_no_export() {
        let runner = FakeRunner::new();
        runner.respond(
            "cryptsetup luksDump --dump-json-metadata /dev/sda2",
            0,
            r#"{"tokens":{"1":{"type":"systemd-tpm2","keyslots":["0"],"tpm2-pcrs":[7]}}}"#,
        );

        let tokens = Cryptsetup::new(&runner).tokens("/dev/sda2");

        assert_eq!(tokens.len(), 1);
        assert!(tokens[0].has_fields());
        assert_eq!(
            runner.lines(),
            ["cryptsetup luksDump --dump-json-metadata /dev/sda2"]
        );
    }

    #[test]
    fn tokens_from_old_dumps_are_exported_for_their_fields() {
        let runner = FakeRunner::new();
        runner.respond("cryptsetup luksDump --dump-json-metadata /dev/sdb", 1, "");
        runner.respond(
            "cryptsetup luksDump /dev/sdb",
            0,
            "LUKS header information\nTokens:\n  0: clevis\n  1: systemd-fido2\nDigests:\n",
        );
        runner.respond(
            "cryptsetup token export --token-id 0 /dev/sdb",
            0,
            r#"{"type":"clevis","keyslots":["1"],"jwe":{}}"#,
        );

        let tokens = Cryptsetup::new(&runner).tokens("/dev/sdb");

        assert_eq!(tokens.len(), 2);
        assert!(tokens[0].has_fields());
        assert!(!tokens[1].has_fields());
    }

    #[test]
    fn a_missing_cryptsetup_reads_nothing_and_opens_nothing() {
        let runner = FakeRunner::new();
        runner.on(|_| Some(Err(std::io::Error::other("no cryptsetup"))));
        let cryptsetup = Cryptsetup::new(&runner);

        assert!(cryptsetup.tokens("/dev/sda2").is_empty());
        assert!(!cryptsetup.is_luks("/dev/sda2"));
        let failed = cryptsetup
            .open(volume(), &key(), OpenOptions::default())
            .unwrap_err();
        assert!(matches!(failed.error, CryptError::Start(_)));
        let closed = cryptsetup.close_named("root");
        assert!(matches!(closed, Err(CryptError::Start(_))));
    }
}
