//! `cryptsetup open` and `cryptsetup close`.

use tracing::{debug, info_span};
use zbm_platform::{Command, Input, Output, Runner, Secret, Stream};

use crate::CryptError;
use crate::mapping::{Closed, Failed, Mapping, Open};

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
}

impl<'a> Cryptsetup<'a> {
    /// `cryptsetup`, run through `runner`.
    pub fn new(runner: &'a dyn Runner) -> Cryptsetup<'a> {
        Cryptsetup { runner }
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
        let mut command =
            Command::new("cryptsetup").args(["open", "--type", "luks", "--key-file=-"]);
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
        let command = Command::new("cryptsetup").args(["close", mapping.name().as_str()]);
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
        if output.status.success() {
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
    use zbm_testutil::FakeRunner;

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
