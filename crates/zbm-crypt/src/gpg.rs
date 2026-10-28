//! GnuPG: decrypting a key file with a private GnuPG home.
//!
//! A [`GnupgHome`] owns a directory that the caller created for the purpose.
//! When it goes out of scope, its agents are stopped (`gpgconf --kill all`)
//! and the directory is removed, on every path, so no agent keeps a card or
//! a cached passphrase after the boot environment starts.
//!
//! Passphrases and card PINs reach `gpg` on standard input (loopback
//! pinentry), and the decrypted key is captured from its output as a secret;
//! nothing decrypted is written to a file. The home's files are copied,
//! made private and removed in process (`zbm_platform::tree`).

#![allow(
    clippy::disallowed_methods,
    reason = "an adapter: these command lines are built here and nowhere else"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_platform::files::Files;
use zbm_platform::tree::{clear_mode_bits, copy_into, remove_tree};

use zbm_platform::{Command, Input, Runner, Secret, Stream};

/// The longest a GnuPG command may run, for a smart card, its reader or
/// `gpg-agent` that stopped answering; it never waits for typing (`--batch`).
const TIMEOUT: Duration = Duration::from_secs(120);

/// The longest `pcscd` may take to start, and `gpgconf` to end the agents.
const DAEMON_TIMEOUT: Duration = Duration::from_secs(30);

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
            let (_, after) = line.rsplit_once(" keyid ")?;
            let id: String = after.chars().take_while(char::is_ascii_hexdigit).collect();
            (!id.is_empty()).then_some(id)
        })
        .collect()
}

/// Where a secret key is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecretKey {
    /// On an OpenPGP card.
    Card,
    /// In the GnuPG home, protected by a passphrase.
    Soft,
}

/// Where the secret key for `id` is, from `gpg --with-colons
/// --list-secret-keys` output, or `None` without a usable key. Field 15 of
/// `sec` and `ssb` records is `+` for a local key, `#` for a stub whose card
/// is not there, and otherwise the card's serial number.
pub fn parse_secret_key(text: &str, id: &str) -> Option<SecretKey> {
    let token = text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        let kind = fields.first().copied().unwrap_or_default();
        let key = fields.get(4).copied().unwrap_or_default();
        ((kind == "sec" || kind == "ssb") && key.eq_ignore_ascii_case(id))
            .then(|| fields.get(14).copied().unwrap_or_default())
    })?;
    match token {
        "+" => Some(SecretKey::Soft),
        "" | "#" => None,
        _ => Some(SecretKey::Card),
    }
}

/// The PC/SC daemon did not start.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
#[error("pcscd did not start")]
pub struct PcscdFailed;

/// Starts the PC/SC daemon (`pcscd`).
///
/// # Errors
///
/// [`PcscdFailed`] when it does not start.
pub fn start_pcscd(runner: &dyn Runner) -> Result<(), PcscdFailed> {
    let command = Command::new("pcscd")
        .timeout(DAEMON_TIMEOUT)
        .stdin(Input::Null)
        .stdout(Stream::Null)
        .stderr(Stream::Null);
    match runner.run(&command) {
        Ok(output) if output.status.is_success() => Ok(()),
        _ => Err(PcscdFailed),
    }
}

/// What `gpg --decrypt` did.
#[derive(Debug)]
pub enum Decrypted {
    /// The key, with `DECRYPTION_OKAY` in gpg's status.
    Key(Secret),
    /// No key, with gpg's status lines or the error that kept it from
    /// running.
    Failed(String),
}

/// A private GnuPG home, cleaned up when dropped.
pub struct GnupgHome<'a> {
    runner: &'a dyn Runner,
    files: &'a dyn Files,
    path: PathBuf,
}

impl<'a> GnupgHome<'a> {
    /// Takes charge of the existing directory `path` as a GnuPG home, with
    /// GnuPG's programs run through `runner` and its files in `files`.
    pub fn new(
        runner: &'a dyn Runner,
        files: &'a dyn Files,
        path: impl Into<PathBuf>,
    ) -> GnupgHome<'a> {
        GnupgHome {
            runner,
            files,
            path: path.into(),
        }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn text(&self) -> String {
        self.path.display().to_string()
    }

    /// Runs `command` quietly; like the legacy code, a failure is left for
    /// the steps that follow to notice.
    fn quietly(&self, command: Command) {
        let command = command
            .stdin(Input::Null)
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        let _ = self.runner.run(&command);
    }

    /// Copies the contents of `source`, such as the image's public keys,
    /// into the home (`cp -a <source>/. <home>/`). Like the legacy code, a
    /// failure is left for the steps that follow to notice.
    pub fn import(&self, source: &Path) {
        if let Err(error) = copy_into(self.files, source, &self.path) {
            tracing::debug!(%error, "copying {} into the GnuPG home", source.display());
        }
    }

    /// Makes everything in the home private to its owner
    /// (`chmod -R go-rwx`), as GnuPG requires.
    pub fn make_private(&self) {
        if let Err(error) = clear_mode_bits(self.files, &self.path, 0o077) {
            tracing::debug!(%error, "making the GnuPG home private");
        }
    }

    /// `gpg` with this home, in batch mode.
    fn gpg(&self) -> Command {
        Command::new("gpg")
            .env("GNUPGHOME", self.text())
            .args(["--batch", "--no-tty"])
            .timeout(TIMEOUT)
            .stdin(Input::Null)
    }

    /// Standard output of a `gpg` run with `args`, if it succeeds.
    fn gpg_output(&self, args: &[&str]) -> Option<String> {
        let command = self
            .gpg()
            .args(args.iter().copied())
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let output = self.runner.run(&command).ok()?;
        output.status.is_success().then(|| output.stdout_text())
    }

    /// The serial number and PIN attempts left of the OpenPGP card that is
    /// plugged in; `gpg --card-status` also creates stubs for the card's
    /// keys whose public keys are known.
    pub fn card_status(&self) -> Option<(String, String)> {
        self.gpg_output(&["--with-colons", "--card-status"])
            .map(|text| parse_card_status(&text))
    }

    /// The long key IDs `file` is encrypted to.
    pub fn recipients(&self, file: &str) -> Vec<String> {
        let command = self
            .gpg()
            .args(["--list-only", "--list-packets", file])
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let text = self
            .runner
            .run(&command)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        parse_recipients(&text)
    }

    /// Where the secret key for `id` is.
    pub fn secret_key(&self, id: &str) -> Option<SecretKey> {
        let command = self
            .gpg()
            .args(["--with-colons", "--list-secret-keys"])
            .stdout(Stream::Capture)
            .stderr(Stream::Null);
        let text = self
            .runner
            .run(&command)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        parse_secret_key(&text, id)
    }

    /// Decrypts `file` with the passphrase or card PIN `secret`.
    pub fn decrypt(&self, file: &str, secret: &Secret) -> Decrypted {
        let mut line = secret.duplicate();
        line.push(b'\n');
        let decrypt = self
            .gpg()
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
            .stdin(Input::secret(line))
            .stdout(Stream::CaptureSecret)
            .stderr(Stream::Capture);
        let (status, key) = match self.runner.run(&decrypt) {
            Ok(output) => (output.stderr_text(), output.into_secret()),
            Err(error) => (error.to_string(), None),
        };
        let okay = status
            .lines()
            .any(|line| line.starts_with("[GNUPG:] DECRYPTION_OKAY"));
        match key.filter(|key| okay && !key.is_empty()) {
            Some(key) => Decrypted::Key(key),
            None => Decrypted::Failed(status),
        }
    }
}

impl Drop for GnupgHome<'_> {
    fn drop(&mut self) {
        let home = self.text();
        self.quietly(
            Command::new("gpgconf")
                .env("GNUPGHOME", &home)
                .args(["--kill", "all"])
                .timeout(DAEMON_TIMEOUT),
        );
        if let Err(error) = remove_tree(self.files, &self.path) {
            tracing::warn!(%error, "removing the GnuPG home {home}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use zbm_platform::{Output, Status};
    use zbm_testkit::FakeRunner;
    use zbm_testkit::fakes::FakeFiles;

    #[test]
    fn a_home_is_set_up_used_and_removed() {
        let files = FakeFiles::new();
        files
            .file_with_mode("/etc/zfsbootmenu/gnupg/pubring.kbx", "keys", 0o644)
            .dir("/run/zbm-gpg.1-0");
        let runner = FakeRunner::new();
        runner.on(|command| {
            (command.program() == "gpg" && command.get_args().iter().any(|arg| arg == "--decrypt"))
                .then(|| {
                    let mut output =
                        Output::with_secret(Status::exited(0), Secret::copy_from(b"key"));
                    output.stderr = b"[GNUPG:] DECRYPTION_OKAY\n".to_vec();
                    Ok(output)
                })
        });
        {
            let home = GnupgHome::new(&runner, &files, "/run/zbm-gpg.1-0");
            home.import(Path::new("/etc/zfsbootmenu/gnupg"));
            home.make_private();
            assert!(matches!(
                home.decrypt("/k.gpg", &Secret::copy_from(b"pass")),
                Decrypted::Key(key) if key.expose() == b"key"
            ));
            assert_eq!(home.path(), Path::new("/run/zbm-gpg.1-0"));
            let copied = Path::new("/run/zbm-gpg.1-0/pubring.kbx");
            assert_eq!(files.text(copied).as_deref(), Some("keys"));
            assert_eq!(files.metadata(copied).unwrap().mode, 0o600);
        }
        assert!(!files.exists(Path::new("/run/zbm-gpg.1-0")));

        assert_eq!(
            runner.lines(),
            [
                "gpg --batch --no-tty --yes --pinentry-mode loopback --passphrase-fd 0 \
                 --status-fd 2 --output - --decrypt /k.gpg",
                "gpgconf --kill all",
            ]
        );
        let calls = runner.calls();
        assert_eq!(
            *calls[0].get_stdin(),
            Input::secret(Secret::copy_from(b"pass\n"))
        );
    }

    #[test]
    fn file_failures_are_left_for_gpg_to_notice() {
        let files = FakeFiles::new();
        files
            .dir("/h")
            .fail("/h", std::io::ErrorKind::PermissionDenied);
        let runner = FakeRunner::new();
        {
            let home = GnupgHome::new(&runner, &files, "/h");
            home.import(Path::new("/missing"));
            home.make_private();
        }

        assert_eq!(runner.lines(), ["gpgconf --kill all"]);
    }

    #[test]
    fn without_decryption_okay_there_is_no_key() {
        let runner = FakeRunner::new();
        runner.respond_program("gpg", 2, "");
        runner.respond_program("pcscd", 1, "");
        let files = FakeFiles::new();
        let home = GnupgHome::new(&runner, &files, "/h");

        assert!(matches!(
            home.decrypt("/k.gpg", &Secret::copy_from(b"x")),
            Decrypted::Failed(_)
        ));
        assert_eq!(home.card_status(), None);
        assert!(home.recipients("/k.gpg").is_empty());
        assert_eq!(home.secret_key("ABCD"), None);
        assert_eq!(start_pcscd(&runner), Err(PcscdFailed));
        let missing = FakeRunner::new();
        missing.on(|_| Some(Err(std::io::Error::other("no gpg"))));
        let files = FakeFiles::new();
        let decrypted =
            GnupgHome::new(&missing, &files, "/h").decrypt("/k.gpg", &Secret::copy_from(b"x"));
        assert!(matches!(decrypted, Decrypted::Failed(status) if status == "no gpg"));
    }

    #[test]
    fn listings_are_read() {
        let runner = FakeRunner::new();
        runner.respond(
            "gpg --batch --no-tty --with-colons --card-status",
            0,
            "serial:12345678:\nchvstatus:1:127:127:127:3:0:3:\n",
        );
        runner.respond(
            "gpg --batch --no-tty --list-only --list-packets /k.gpg",
            0,
            ":pubkey enc packet: version 3, algo 18, keyid 0123456789ABCDEF\n",
        );
        runner.respond(
            "gpg --batch --no-tty --with-colons --list-secret-keys",
            0,
            "ssb:u:255:18:0123456789ABCDEF:1::::::e:::D2760001:::23:\n",
        );
        let files = FakeFiles::new();
        let home = GnupgHome::new(&runner, &files, "/h");

        assert_eq!(
            home.card_status(),
            Some(("12345678".to_owned(), "3".to_owned()))
        );
        assert_eq!(home.recipients("/k.gpg"), ["0123456789ABCDEF"]);
        assert_eq!(home.secret_key("0123456789abcdef"), Some(SecretKey::Card));
        assert_eq!(
            parse_secret_key("sec::::K::::::::::+:", "k"),
            Some(SecretKey::Soft)
        );
        assert_eq!(parse_secret_key("sec::::K::::::::::#:", "K"), None);
        assert_eq!(
            parse_card_status(""),
            ("unknown".to_owned(), "?".to_owned())
        );
    }
}
