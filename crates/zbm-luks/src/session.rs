//! An unlocking run: its settings, the user interface, the secrets it keeps
//! for its duration, and `cryptsetup open`.
//!
//! The legacy functions share global variables; here a [`Session`] holds them.
//! Secrets that unlock several volumes (passphrases, TPM2 PINs, decrypted
//! GnuPG key files) stay in the session until it ends or
//! [`Session::forget_secrets`] drops them, which wipes them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_core::color::colorize;
use zbm_crypt::{CryptError, Cryptsetup, KeySource};
use zbm_platform::exec::{Command, Input, Stream};
use zbm_platform::secret::Secret;
use zbm_platform::{Environment, System};
use zbm_state::State;
pub use zbm_types::Answer;

use crate::config::{CRYPTTAB, DEFAULT_TRIES};
use crate::device::OpenCheck;
use crate::error::LuksError;

/// Where the image keeps public keys and settings for GnuPG.
pub const GNUPG_DIR: &str = "/etc/zfsbootmenu/gnupg";

/// The TPM of the machine.
pub const TPM2_TCTI: &str = "device:/dev/tpmrm0";

/// The settings of the LUKS code, with the names of the legacy variables that
/// set them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settings {
    /// The state directory (`BASE`), whose `cmdline.d` holds the options.
    pub base: PathBuf,
    /// The crypttab (`ZBM_CRYPTTAB`).
    pub crypttab: PathBuf,
    /// The mappings ZFSBootMenu opened (`LUKS_STATE`).
    pub mappings: PathBuf,
    /// The volumes the user skipped (`LUKS_SKIPPED`).
    pub skipped: PathBuf,
    /// Public keys and settings for GnuPG (`LUKS_GNUPG_DIR`).
    pub gnupg_dir: PathBuf,
    /// Where the TPM is (`LUKS_TPM2_TCTI`).
    pub tpm2_tcti: String,
    /// Whether cryptsetup's token plugins may be used (unless
    /// `LUKS_NO_TOKEN_PLUGINS` is set).
    pub token_plugins: bool,
    /// Passphrase attempts per round (`LUKS_DEFAULT_TRIES`).
    pub default_tries: u64,
    /// Whether this run is a retry of the import loop (`LUKS_RETRY`), which
    /// only looks at volumes that appeared since and never waits.
    pub retry: bool,
    /// The delay of the prompt for a missing device (`zbm_retry_delay`).
    pub retry_delay: String,
    /// How to tell whether a volume is open.
    pub open_check: OpenCheck,
    /// Whether text is coloured (the unit suites replace `colorize` with
    /// plain text).
    pub colors: bool,
    /// The device-mapper directory.
    pub mapper: PathBuf,
}

impl Settings {
    /// The settings of the boot image, whose state directory is `base`.
    pub fn new(base: impl Into<PathBuf>) -> Settings {
        let base = base.into();
        Settings {
            crypttab: PathBuf::from(CRYPTTAB),
            mappings: base.join("luks-mappings"),
            skipped: base.join("luks-skipped"),
            base,
            gnupg_dir: PathBuf::from(GNUPG_DIR),
            tpm2_tcti: TPM2_TCTI.to_owned(),
            token_plugins: true,
            default_tries: DEFAULT_TRIES,
            retry: false,
            retry_delay: String::new(),
            open_check: OpenCheck::Sysfs,
            colors: true,
            mapper: PathBuf::from("/dev/mapper"),
        }
    }

    /// The settings as the legacy variables in `environment` change them, as
    /// `: "${VAR:=default}"` lets them: `BASE`, `ZBM_CRYPTTAB`, `LUKS_STATE`,
    /// `LUKS_SKIPPED`, `LUKS_GNUPG_DIR`, `LUKS_TPM2_TCTI`,
    /// `LUKS_NO_TOKEN_PLUGINS`, `LUKS_DEFAULT_TRIES`, `LUKS_RETRY` and
    /// `zbm_retry_delay`.
    pub fn from_environment(environment: &dyn Environment) -> Settings {
        let var = |name: &str| environment.non_empty(name);
        let base = var("BASE").unwrap_or_else(|| zbm_state::BASE.to_owned());
        let mut settings = Settings::new(&base);
        if let Some(path) = var("ZBM_CRYPTTAB") {
            settings.crypttab = PathBuf::from(path);
        }
        if let Some(path) = var("LUKS_STATE") {
            settings.mappings = PathBuf::from(path);
        }
        if let Some(path) = var("LUKS_SKIPPED") {
            settings.skipped = PathBuf::from(path);
        }
        if let Some(path) = var("LUKS_GNUPG_DIR") {
            settings.gnupg_dir = PathBuf::from(path);
        }
        if let Some(tcti) = var("LUKS_TPM2_TCTI") {
            settings.tpm2_tcti = tcti;
        }
        settings.token_plugins = var("LUKS_NO_TOKEN_PLUGINS").is_none();
        if let Some(tries) = var("LUKS_DEFAULT_TRIES") {
            settings.default_tries = zbm_core::shell::test_integer(&tries)
                .and_then(|tries| u64::try_from(tries).ok())
                .unwrap_or(DEFAULT_TRIES);
        }
        settings.retry = var("LUKS_RETRY").is_some();
        settings.retry_delay = environment.var("zbm_retry_delay").unwrap_or_default();
        settings
    }
}

/// A `timed_prompt` of the LUKS code: a message, what RETURN and ESCAPE do,
/// and the countdown "Retrying in NN seconds".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptRequest {
    /// `-d`: the delay, as the legacy code passes it; `None` for the default.
    pub delay: Option<String>,
    /// `-m`: the message, shown in red.
    pub message: String,
    /// `-r`: what RETURN does.
    pub on_return: String,
    /// `-e`: what ESCAPE does.
    pub on_escape: String,
}

impl PromptRequest {
    /// The arguments of the legacy call, with the colours left out, as the
    /// unit suites record them.
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = Vec::new();
        if let Some(delay) = &self.delay {
            arguments.extend(["-d".to_owned(), delay.clone()]);
        }
        arguments.extend([
            "-m".to_owned(),
            self.message.clone(),
            "-r".to_owned(),
            self.on_return.clone(),
            "-e".to_owned(),
            self.on_escape.clone(),
            "-p".to_owned(),
            "Retrying in %0.2d seconds".to_owned(),
        ]);
        arguments
    }
}

/// Shows timed prompts. The start-up program draws them as `timed_prompt`
/// does; the unit suites record and answer them.
pub trait Prompter {
    /// Shows the prompt and returns what the user did: RETURN or letting the
    /// time pass is [`Answer::Continue`], ESCAPE is [`Answer::Escape`].
    fn timed_prompt(&self, request: &PromptRequest) -> Answer;
}

/// How [`Session::wait_for`] ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Wait {
    /// What was waited for is there.
    Present,
    /// The user pressed a key to stop waiting.
    Skipped,
    /// The time ran out, or there was no time to wait.
    TimedOut,
}

/// What the LUKS code works with.
#[derive(Clone, Copy)]
pub struct Luks<'a> {
    /// The system.
    pub system: System<'a>,
    /// The settings.
    pub settings: &'a Settings,
    /// The timed prompts.
    pub prompter: &'a dyn Prompter,
}

/// How `cryptsetup open` gets the key.
pub enum Key<'a> {
    /// A key file, read by `cryptsetup`.
    File(&'a str),
    /// Key material on standard input (`--key-file=-`).
    Stdin(&'a Secret),
}

/// An unlocking run, with the secrets it keeps until it ends.
pub struct Session<'a> {
    /// What the run works with.
    pub luks: Luks<'a>,
    /// Passphrases (and YubiKey responses) that unlocked a volume.
    pub(crate) passphrases: Vec<Secret>,
    /// TPM2 PINs that unsealed a volume's key.
    pub(crate) tpm2_pins: Vec<Secret>,
    /// Decrypted GnuPG key files, by key file specification.
    pub(crate) gpg_plain: BTreeMap<String, Secret>,
    /// Key devices that did not appear in this run.
    pub(crate) keydev_absent: BTreeSet<String>,
    /// Whether the TPM drivers were loaded already.
    pub(crate) tpm2_probed: bool,
    /// Whether `udev`'s device-mapper rules are missing, so `cryptsetup` must
    /// not wait for `udev` (`DM_DISABLE_UDEV=1`).
    pub(crate) dm_disable_udev: bool,
}

impl<'a> Session<'a> {
    /// A run without secrets.
    pub fn new(luks: Luks<'a>) -> Session<'a> {
        Session {
            luks,
            passphrases: Vec::new(),
            tpm2_pins: Vec::new(),
            gpg_plain: BTreeMap::new(),
            keydev_absent: BTreeSet::new(),
            tpm2_probed: false,
            dm_disable_udev: false,
        }
    }

    /// The system.
    pub fn system(&self) -> &System<'a> {
        &self.luks.system
    }

    /// The settings.
    pub fn settings(&self) -> &'a Settings {
        self.luks.settings
    }

    /// Drops every secret of the run, which wipes it, and forgets the key
    /// devices that were missing (`luks_forget_secrets`).
    pub fn forget_secrets(&mut self) {
        self.gpg_plain.clear();
        self.passphrases.clear();
        self.tpm2_pins.clear();
        self.keydev_absent.clear();
    }

    /// Whether `cryptsetup` can be used (`luks_available`). Without
    /// device-mapper's `udev` rules nothing acknowledges libdevmapper's
    /// `udev` cookie and `cryptsetup` waits forever, so it is told to manage
    /// the device nodes itself.
    pub fn available(&mut self) -> bool {
        let system = self.system();
        if !system.command_exists("cryptsetup") {
            return false;
        }
        let rules = [
            "/usr/lib/udev/rules.d/95-dm-notify.rules",
            "/lib/udev/rules.d/95-dm-notify.rules",
            "/etc/udev/rules.d/95-dm-notify.rules",
        ];
        let set = system.environment.var_os("DM_DISABLE_UDEV").is_some();
        if !set
            && !rules
                .iter()
                .any(|rule| system.files.exists(Path::new(rule)))
        {
            zbm_trace::debug!(
                "{}",
                "95-dm-notify.rules missing, disabling udev synchronization for device-mapper",
            );
            self.dm_disable_udev = true;
        }
        true
    }

    /// A command as the legacy code runs it: standard input from `/dev/null`
    /// unless the caller changes it, and with `DM_DISABLE_UDEV` when
    /// [`Session::available`] exported it.
    pub fn command(&self, program: &str) -> Command {
        let command = Command::new(program).stdin(Input::Null);
        if self.dm_disable_udev {
            command.env("DM_DISABLE_UDEV", "1")
        } else {
            command
        }
    }

    /// The value of a ZFSBootMenu command line option (`get_zbm_arg`).
    pub fn zbm_arg(&self, key: &str) -> Option<String> {
        State::new(self.system().files, &self.settings().base).zbm_arg(key)
    }

    /// `colorize`, or plain text where the suites expect it.
    pub fn paint(&self, color: &str, text: &str) -> String {
        if self.settings().colors {
            colorize(color, text)
        } else {
            text.to_owned()
        }
    }

    /// Writes text to the console.
    pub fn say(&self, text: &str) {
        let _ = self.system().console.write(text.as_bytes());
    }

    /// `tput <args>`, whose output goes to the console.
    pub fn tput(&self, args: &[&str]) {
        let command = self.command("tput").args(args).stderr(Stream::Null);
        if let Ok(output) = self.system().runner.run(&command) {
            let _ = self.system().console.write(&output.stdout);
        }
    }

    /// `udevadm settle`.
    pub fn settle(&self) {
        let command = self
            .command("udevadm")
            .arg("settle")
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        let _ = self.system().runner.run(&command);
    }

    /// Shows a timed prompt.
    pub fn timed_prompt(&self, request: &PromptRequest) -> Answer {
        self.luks.prompter.timed_prompt(request)
    }

    /// Asks for a secret with echo off (`luks_read_secret`); `None` when the
    /// input ends. Echo goes off before the prompt appears, so no keystroke
    /// can echo.
    pub fn read_secret(&self, prompt: &str) -> Option<Secret> {
        let console = self.system().console;
        console.set_echo(false);
        self.tput(&["cnorm"]);
        self.say(prompt);
        let secret = console.read_secret().ok().flatten();
        console.set_echo(true);
        self.say("\n");
        secret
    }

    /// Seconds to wait for a security key, smart card or key device
    /// (`luks_token_wait`, `zbm.luks_token_wait`): up to six digits, 10
    /// otherwise.
    pub fn token_wait(&self) -> u64 {
        self.zbm_arg("zbm.luks_token_wait")
            .filter(|wait| (1..=6).contains(&wait.len()))
            .filter(|wait| wait.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|wait| wait.parse().ok())
            .unwrap_or(10)
    }

    /// Whether LUKS2 tokens may be tried (`zbm.luks_tokens`, on by default),
    /// as `get_zbm_bool 1 zbm.luks_tokens` decides.
    pub fn tokens_enabled(&self) -> bool {
        self.zbm_arg("zbm.luks_tokens")
            .is_none_or(|value| !matches!(value.to_lowercase().as_str(), "" | "no" | "off" | "0"))
    }

    /// Waits up to `seconds` for `present` to hold, showing `message`, unless
    /// the user presses a key (`luks_wait_for`).
    pub fn wait_for(&self, seconds: u64, message: &str, mut present: impl FnMut() -> bool) -> Wait {
        if present() {
            return Wait::Present;
        }
        if seconds == 0 {
            return Wait::TimedOut;
        }
        let system = self.system();
        self.say(&format!("{message}\n"));
        self.say(&format!(
            "Waiting {seconds} seconds; press any key to skip.\n"
        ));
        let second = Duration::from_secs(1);
        for _ in 0..seconds {
            let before = system.clock.now();
            match system.console.read_key(Some(second)) {
                Ok(Some(_)) => return Wait::Skipped,
                // The end of the input returns at once, so wait here.
                _ if system.clock.now().saturating_sub(before) < second => {
                    system.clock.sleep(second);
                }
                _ => {}
            }
            if present() {
                self.settle();
                return Wait::Present;
            }
        }
        Wait::TimedOut
    }

    /// `cryptsetup open --type luks --key-file=<key> <args> <device> <name>`
    /// (`luks_cryptsetup_open`); logs the output when it fails.
    ///
    /// # Errors
    ///
    /// [`LuksError::Open`] with cryptsetup's exit status.
    pub fn cryptsetup_open(
        &self,
        name: &str,
        device: &str,
        key: Key<'_>,
        args: &[String],
    ) -> Result<(), LuksError> {
        let key = match key {
            Key::File(path) => KeySource::File(path),
            Key::Stdin(secret) => KeySource::Secret(secret),
        };
        let result = Cryptsetup::new(self.system().runner)
            .without_udev(self.dm_disable_udev)
            .open_described(device, name, key, args);
        let (status, output) = match result {
            Ok(()) => return Ok(()),
            Err(CryptError::Failed { status, output }) => (status, output),
            Err(error) => (127, error.to_string()),
        };
        zbm_trace::debug!("cryptsetup open {device} {name} failed ({status}): {output}");
        Err(LuksError::Open {
            device: device.to_owned(),
            name: name.to_owned(),
            status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{NoPrompts, Prompts, log};
    use zbm_platform::clock::Clock;
    use zbm_platform::exec::{Output, Status};
    use zbm_testkit::fakes::{ConsoleInput, FakeSystem};

    #[test]
    fn settings_follow_the_legacy_variables() {
        let fake = FakeSystem::new();
        let settings = Settings::from_environment(&fake.environment);
        assert_eq!(settings, Settings::new("/zfsbootmenu"));
        assert_eq!(settings.mappings, Path::new("/zfsbootmenu/luks-mappings"));
        assert!(settings.token_plugins && !settings.retry);
        for (name, value) in [
            ("BASE", "/run/base"),
            ("ZBM_CRYPTTAB", "/tab"),
            ("LUKS_STATE", "/state"),
            ("LUKS_GNUPG_DIR", "/gpg"),
            ("LUKS_TPM2_TCTI", "swtpm:port=1"),
            ("LUKS_NO_TOKEN_PLUGINS", "1"),
            ("LUKS_DEFAULT_TRIES", "08"),
            ("LUKS_RETRY", "1"),
            ("zbm_retry_delay", "7"),
        ] {
            fake.environment.set(name, value);
        }
        let settings = Settings::from_environment(&fake.environment);
        assert_eq!(settings.base, Path::new("/run/base"));
        assert_eq!(settings.skipped, Path::new("/run/base/luks-skipped"));
        assert_eq!(settings.mappings, Path::new("/state"));
        assert_eq!(settings.crypttab, Path::new("/tab"));
        assert_eq!(settings.gnupg_dir, Path::new("/gpg"));
        assert_eq!(settings.tpm2_tcti, "swtpm:port=1");
        assert!(!settings.token_plugins && settings.retry);
        assert_eq!(settings.default_tries, 8);
        assert_eq!(settings.retry_delay, "7");
    }

    fn with_session(fake: &FakeSystem, body: impl FnOnce(&mut Session<'_>)) {
        let settings = Settings::new("/zfsbootmenu");
        let _log = log(fake);
        let luks = Luks {
            system: fake.system(),
            settings: &settings,
            prompter: &NoPrompts,
        };
        body(&mut Session::new(luks));
    }

    #[test]
    fn token_waits_are_six_digits_at_most() {
        let fake = FakeSystem::new();
        for (value, wait) in [
            (None, 10),
            (Some("0"), 0),
            (Some("08"), 8),
            (Some("010"), 10),
            (Some("999999"), 999_999),
            (Some("1000000"), 10),
            (Some("-5"), 10),
            (Some("+5"), 10),
            (Some("1e3"), 10),
            (Some(""), 10),
        ] {
            if let Some(value) = value {
                fake.files
                    .file("/zfsbootmenu/cmdline.d/zbm.luks_token_wait", value);
            }
            with_session(&fake, |session| {
                assert_eq!(session.token_wait(), wait, "{value:?}");
            });
        }
    }

    #[test]
    fn tokens_are_on_unless_turned_off() {
        let fake = FakeSystem::new();
        with_session(&fake, |session| assert!(session.tokens_enabled()));
        for (value, on) in [
            ("0", false),
            ("off", false),
            ("No", false),
            ("", false),
            ("1", true),
        ] {
            fake.files
                .file("/zfsbootmenu/cmdline.d/zbm.luks_tokens", value);
            with_session(&fake, |session| {
                assert_eq!(session.tokens_enabled(), on, "{value:?}");
            });
        }
    }

    #[test]
    fn waiting_ends_with_the_device_or_a_key() {
        let fake = FakeSystem::new();
        fake.console.timeout().timeout();
        let mut checks = 0;
        with_session(&fake, |session| {
            let waited = session.wait_for(5, "Insert it", || {
                checks += 1;
                checks == 3
            });
            assert_eq!(waited, Wait::Present);
        });
        assert_eq!(checks, 3);
        assert_eq!(fake.clock.now(), Duration::from_secs(2));
        assert_eq!(
            fake.console.output_text(),
            "Insert it\nWaiting 5 seconds; press any key to skip.\n"
        );
        assert_eq!(fake.runner.lines(), ["udevadm settle"]);

        // A key skips the wait; the end of the input waits a second each time.
        let fake = FakeSystem::new();
        fake.console.keys(b"x");
        with_session(&fake, |session| {
            assert_eq!(session.wait_for(5, "m", || false), Wait::Skipped);
        });
        let fake = FakeSystem::new();
        fake.console.push(ConsoleInput::End);
        with_session(&fake, |session| {
            assert_eq!(session.wait_for(3, "m", || false), Wait::TimedOut);
        });
        assert_eq!(fake.clock.now(), Duration::from_secs(3));
        // Without time to wait, only the first check counts.
        let fake = FakeSystem::new();
        with_session(&fake, |session| {
            assert_eq!(session.wait_for(0, "m", || false), Wait::TimedOut);
        });
        assert_eq!(fake.console.output_text(), "");
    }

    #[test]
    fn secrets_are_read_with_echo_off() {
        let fake = FakeSystem::new();
        fake.console.secret(b"1234");
        with_session(&fake, |session| {
            let pin = session.read_secret("TPM2 PIN: ").unwrap();
            assert_eq!(pin.expose(), b"1234");
            assert_eq!(session.read_secret("again: "), None);
        });
        assert_eq!(fake.console.echo_changes(), [false, true, false, true]);
        assert_eq!(fake.console.output_text(), "TPM2 PIN: \nagain: \n");
        fake.assert_no_leak(b"1234");
    }

    #[test]
    fn cryptsetup_gets_keys_on_standard_input() {
        let fake = FakeSystem::new();
        fake.runner.on(|command| {
            let stdin_ok =
                matches!(command.get_stdin(), Input::Secret(secret) if secret.expose() == b"pw");
            Some(Ok(if stdin_ok {
                Output::new(Status::exited(0), "")
            } else {
                Output::new(
                    Status::exited(2),
                    "No key available with this passphrase.\n\n",
                )
            }))
        });
        with_session(&fake, |session| {
            let args = ["--allow-discards".to_owned()];
            let right = Secret::copy_from(b"pw");
            let wrong = Secret::copy_from(b"no");
            let open = |key| session.cryptsetup_open("v", "/dev/sda", key, &[]);
            assert_eq!(
                session.cryptsetup_open("v", "/dev/sda", Key::Stdin(&right), &args),
                Ok(())
            );
            assert_eq!(
                open(Key::Stdin(&wrong)),
                Err(LuksError::Open {
                    device: "/dev/sda".to_owned(),
                    name: "v".to_owned(),
                    status: 2
                })
            );
            assert!(open(Key::File("/k")).is_err());
        });
        assert_eq!(
            fake.runner.lines(),
            [
                "cryptsetup open --type luks --key-file=- --allow-discards /dev/sda v",
                "cryptsetup open --type luks --key-file=- /dev/sda v",
                "cryptsetup open --type luks --key-file=/k /dev/sda v",
            ]
        );
        let missing = FakeSystem::new();
        missing
            .runner
            .on(|_| Some(Err(std::io::Error::other("no cryptsetup"))));
        with_session(&missing, |session| {
            assert!(matches!(
                session.cryptsetup_open("v", "/dev/sda", Key::File("/k"), &[]),
                Err(LuksError::Open { status: 127, .. })
            ));
        });
        assert!(fake.kmsg().contains(
            "|cryptsetup open /dev/sda v failed (2): No key available with this passphrase."
        ));
        assert_eq!(*fake.runner.calls()[2].get_stdin(), Input::Null);
        fake.assert_no_leak(b"pw");
    }

    #[test]
    fn device_mapper_manages_nodes_without_udev_rules() {
        let fake = FakeSystem::new();
        fake.environment.set("PATH", "/sbin");
        with_session(&fake, |session| assert!(!session.available()));
        fake.files.file_with_mode("/sbin/cryptsetup", "", 0o755);
        with_session(&fake, |session| {
            assert!(session.available());
            let command = session.command("cryptsetup");
            assert_eq!(command.get_env().len(), 1);
        });
        fake.files
            .file("/usr/lib/udev/rules.d/95-dm-notify.rules", "");
        with_session(&fake, |session| {
            assert!(session.available());
            assert!(session.command("cryptsetup").get_env().is_empty());
        });
        assert_eq!(
            fake.kmsg()
                .matches("disabling udev synchronization")
                .count(),
            1
        );
    }

    #[test]
    fn prompts_go_to_the_prompter() {
        let fake = FakeSystem::new();
        let settings = Settings::new("/zfsbootmenu");
        let _log = log(&fake);
        let prompts = Prompts::answering(&[Answer::Continue]);
        let session = Session::new(Luks {
            system: fake.system(),
            settings: &settings,
            prompter: &prompts,
        });
        let request = PromptRequest {
            delay: None,
            message: "Unable to unlock LUKS volume /dev/sda".to_owned(),
            on_return: "try again".to_owned(),
            on_escape: "skip this volume".to_owned(),
        };
        assert_eq!(session.timed_prompt(&request), Answer::Continue);
        assert_eq!(session.timed_prompt(&request), Answer::Escape);
        assert_eq!(prompts.messages().len(), 2);
    }

    #[test]
    fn prompts_record_their_legacy_arguments() {
        let request = PromptRequest {
            delay: Some("5".to_owned()),
            message: "LUKS device x is missing".to_owned(),
            on_return: "retry now".to_owned(),
            on_escape: "skip this device".to_owned(),
        };
        assert_eq!(
            request.arguments().join(" "),
            "-d 5 -m LUKS device x is missing -r retry now -e skip this device \
             -p Retrying in %0.2d seconds"
        );
    }
}
