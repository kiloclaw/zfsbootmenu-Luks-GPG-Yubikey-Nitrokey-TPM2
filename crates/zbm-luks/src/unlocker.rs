//! The [`Unlocker`]: one unlocking run, with its settings, the user
//! interface, the secrets it keeps for its duration, and `cryptsetup open`.
//!
//! It opens a volume with the key methods of [`crate::unlock::methods`], in
//! order, and then the passphrase prompt of [`crate::unlock::prompt`]; it
//! keeps its records in a [`crate::mappings::MappingStore`]. Secrets that
//! unlock several volumes (passphrases, TPM2 PINs, decrypted GnuPG key files)
//! stay in the unlocker until it ends or [`Unlocker::forget_secrets`] drops
//! them, which wipes them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use zbm_config::args::{Args, CmdlineDir};
use zbm_config::spec::OptionSpec;
use zbm_config::spec::cmdline::luks::{LUKS_TOKEN_WAIT, LUKS_TOKENS};
use zbm_core::color::colorize;
use zbm_crypt::{CryptError, Cryptsetup, KeySource};
use zbm_platform::exec::Input;
use zbm_platform::secret::Secret;
use zbm_platform::{Environment, System};
use zbm_tools::{Shown, terminal, udev};
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
            settings.default_tries = tries
                .trim()
                .parse::<i64>()
                .ok()
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

/// How [`Unlocker::wait_for`] ended.
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
#[derive(Clone, Copy)]
pub enum Key<'a> {
    /// A key file, read by `cryptsetup`.
    File(&'a str),
    /// Key material on standard input (`--key-file=-`).
    Stdin(&'a Secret),
}

/// An unlocking run, with the secrets it keeps until it ends.
pub struct Unlocker<'a> {
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

impl<'a> Unlocker<'a> {
    /// A run without secrets.
    pub fn new(luks: Luks<'a>) -> Unlocker<'a> {
        Unlocker {
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

    /// The environment the legacy code exported for the programs it runs:
    /// `DM_DISABLE_UDEV=1` when [`Unlocker::available`] exported it.
    pub fn exported(&self) -> &'static [(&'static str, &'static str)] {
        if self.dm_disable_udev {
            &[("DM_DISABLE_UDEV", "1")]
        } else {
            &[]
        }
    }

    /// The options the start-up stored from ZFSBootMenu's command line.
    pub fn options(&self) -> CmdlineDir<'a> {
        CmdlineDir::new(self.system().files, &self.settings().base)
    }

    /// The value of a ZFSBootMenu command line option (`get_zbm_arg`).
    pub fn option(&self, spec: &OptionSpec) -> Option<String> {
        self.options().lookup(spec, &mut Vec::new())
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
        let output = terminal::tput(self.system().runner, args, Input::Null);
        let _ = self.system().console.write(&output);
    }

    /// `udevadm settle`.
    pub fn settle(&self) {
        udev::settle(self.system().runner, Shown::Hidden);
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
        self.option(&LUKS_TOKEN_WAIT)
            .filter(|wait| (1..=6).contains(&wait.len()))
            .filter(|wait| wait.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|wait| wait.parse().ok())
            .unwrap_or(10)
    }

    /// Whether LUKS2 tokens may be tried (`zbm.luks_tokens`, on by default),
    /// as `get_zbm_bool 1 zbm.luks_tokens` decides.
    pub fn tokens_enabled(&self) -> bool {
        self.options().enabled(&LUKS_TOKENS, &mut Vec::new())
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
mod tests;
