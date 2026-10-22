//! `cryptsetup`: opening and closing volumes, and reading their tokens.

use tracing::{debug, info_span};
use zbm_platform::{Command, Input, Output, Runner, Secret, Stream};

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
    fn command<'s>(&self, args: impl IntoIterator<Item = &'s str>) -> Command {
        let command = Command::new("cryptsetup").args(args).stdin(Input::Null);
        if self.without_udev {
            command.env("DM_DISABLE_UDEV", "1")
        } else {
            command
        }
    }

    /// What `cryptsetup <args>` prints on standard output, if it succeeds;
    /// its errors are discarded.
    fn quietly<'s>(&self, args: impl IntoIterator<Item = &'s str>) -> Option<String> {
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
        let metadata = self.quietly(["luksDump", "--dump-json-metadata", header]);
        if let Some(Ok(tokens)) = metadata.as_deref().map(token::from_metadata) {
            return tokens;
        }
        let text = self.quietly(["luksDump", header]).unwrap_or_default();
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
        let text = self.quietly(["token", "export", "--token-id", id, header])?;
        Token::from_export(id, &text).ok()
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
            .command(["open", "--type", "luks", "--token-only", "--token-id", id])
            .args(options.iter().map(String::as_str))
            .args([device, name])
            .stdin(stdin)
            .stdout(Stream::Capture)
            .merge_stderr();
        let output = self.runner.run(&command).map_err(CryptError::Start)?;
        if output.status.is_success() {
            return Ok(());
        }
        Err(CryptError::Failed {
            status: output.status.shell_code(),
            output: output.stdout_text().trim().to_owned(),
        })
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
        let mut command = self.command(["open", "--type", "luks", "--key-file=-"]);
        if options.allow_discards {
            command = command.arg("--allow-discards");
        }
        if options.read_only {
            command = command.arg("--readonly");
        }
        let command = command
            .args([mapping.device().as_str(), mapping.name().as_str()])
            .stdin(Input::Secret(key.clone()));
        match self.run(command, &mapping) {
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
        let command = self.command(["close", mapping.name().as_str()]);
        match self.run(command, &mapping) {
            Ok(()) => Ok(mapping.into_state()),
            Err(error) => Err(Failed { mapping, error }),
        }
    }

    /// Runs `command` with its output captured, and turns a failure into the
    /// [`CryptError`] that its exit status means for `mapping`.
    fn run<S>(&self, command: Command, mapping: &Mapping<S>) -> Result<(), CryptError> {
        let command = command.stdout(Stream::Capture).merge_stderr();
        let output = self.runner.run(&command).map_err(CryptError::Start)?;
        if output.status.is_success() {
            return Ok(());
        }
        let error = classify(&output, mapping);
        debug!(%error, "cryptsetup failed");
        Err(error)
    }
}

/// The error that a failed `cryptsetup` run means, from its exit status
/// (`cryptsetup(8)`, "RETURN CODES").
fn classify<S>(output: &Output, mapping: &Mapping<S>) -> CryptError {
    match output.status.shell_code() {
        2 => CryptError::WrongKey {
            device: mapping.device().clone(),
        },
        4 => CryptError::NoDevice {
            device: mapping.device().clone(),
        },
        5 => CryptError::Busy {
            name: mapping.name().clone(),
        },
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
}
