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

use zbm_config::args::{Args, CmdlineDir};
use zbm_config::spec::OptionSpec;
use zbm_config::spec::cmdline::luks::{LUKS_TOKEN_WAIT, LUKS_TOKENS};
use zbm_crypt::{Closed, Cryptsetup, LuksVolumes, Mapping};
use zbm_platform::secret::Secret;
use zbm_platform::{Context, Environment, Runner};
use zbm_tools::{Shown, udev};
pub use zbm_types::Answer;
use zbm_types::RetryDelay;

use crate::config::{CRYPTTAB, DEFAULT_TRIES};
use crate::device::OpenCheck;
use crate::error::LuksError;
use zbm_platform::interaction::{
    Asked, Interaction, PromptRequest, SecretRequest, Tell, Told, Wait, WaitRequest,
};

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
    pub retry_delay: RetryDelay,
    /// How to tell whether a volume is open.
    pub open_check: OpenCheck,
    /// Whether text is coloured; the unit suites turn it off, as they expect
    /// plain text.
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
            retry_delay: RetryDelay::DEFAULT,
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
        settings.retry_delay =
            RetryDelay::or_default(&environment.var("zbm_retry_delay").unwrap_or_default());
        settings
    }
}

/// What the LUKS code works with.
#[derive(Clone, Copy)]
pub struct Luks<'a> {
    /// The ports to the system.
    pub context: Context<'a>,
    /// The settings.
    pub settings: &'a Settings,
    /// The person at the console.
    pub interaction: &'a dyn Interaction,
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
    /// What answers for the LUKS volumes instead of `cryptsetup`, as a
    /// test's.
    volumes: Option<&'a dyn LuksVolumes>,
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
            volumes: None,
            luks,
            passphrases: Vec::new(),
            tpm2_pins: Vec::new(),
            gpg_plain: BTreeMap::new(),
            keydev_absent: BTreeSet::new(),
            tpm2_probed: false,
            dm_disable_udev: false,
        }
    }

    /// The run, with `volumes` answering for the LUKS volumes instead of
    /// `cryptsetup`.
    #[must_use]
    pub fn with_volumes(mut self, volumes: &'a dyn LuksVolumes) -> Unlocker<'a> {
        self.volumes = Some(volumes);
        self
    }

    /// The LUKS port: `cryptsetup` through the context's runner, with
    /// device-mapper's udev synchronization off when
    /// [`Unlocker::available`] turned it off.
    pub fn volumes(&self) -> Box<dyn LuksVolumes + '_> {
        match self.volumes {
            Some(volumes) => Box::new(volumes),
            None => Box::new(
                Cryptsetup::new(self.context().runner()).without_udev(self.dm_disable_udev),
            ),
        }
    }

    /// The ports to the system.
    pub fn context(&self) -> &Context<'a> {
        &self.luks.context
    }

    /// The settings.
    pub fn settings(&self) -> &'a Settings {
        self.luks.settings
    }

    /// Drops every secret of the run, which wipes it, and forgets the key
    /// devices that were missing.
    pub fn forget_secrets(&mut self) {
        self.gpg_plain.clear();
        self.passphrases.clear();
        self.tpm2_pins.clear();
        self.keydev_absent.clear();
    }

    /// Whether `cryptsetup` can be used. Without device-mapper's `udev` rules
    /// nothing acknowledges libdevmapper's `udev` cookie and `cryptsetup`
    /// waits forever, so it is told to manage the device nodes itself.
    pub fn available(&mut self) -> bool {
        let context = *self.context();
        if !context.command_exists("cryptsetup") {
            return false;
        }
        let rules = [
            "/usr/lib/udev/rules.d/95-dm-notify.rules",
            "/lib/udev/rules.d/95-dm-notify.rules",
            "/etc/udev/rules.d/95-dm-notify.rules",
        ];
        let set = context.environment().var_os("DM_DISABLE_UDEV").is_some();
        if !set
            && !rules
                .iter()
                .any(|rule| context.files().exists(Path::new(rule)))
        {
            zbm_trace::debug!(
                "95-dm-notify.rules missing, disabling udev synchronization for device-mapper"
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
        CmdlineDir::new(self.context().files(), &self.settings().base)
    }

    /// The value of a ZFSBootMenu command line option.
    pub fn option(&self, spec: &OptionSpec) -> Option<String> {
        self.options().lookup(spec, &mut Vec::new())
    }

    /// `udevadm settle`.
    pub fn settle(&self) {
        udev::settle(self.context().runner(), Shown::Hidden);
    }

    /// Shows a timed prompt.
    pub fn timed_prompt(&self, request: &PromptRequest) -> Answer {
        self.luks.interaction.ask(request)
    }

    /// Asks for a secret.
    pub fn ask(&self, request: &SecretRequest<'_>) -> Asked {
        self.luks.interaction.secret(request)
    }

    /// Runs `run` with the runner whose programs reach the console
    /// ([`Interaction::lease_console`]): a front end that draws on it steps aside.
    pub fn with_console(&self, run: &mut dyn FnMut(&dyn Runner)) {
        let own = self.context().runner();
        self.luks
            .interaction
            .lease_console(&mut |runner| run(runner.unwrap_or(own)));
    }

    /// Waits for a device. One that appeared meanwhile is used once udev
    /// settled, so that its links and partitions are there, whichever front
    /// end waited.
    pub fn await_device(&self, request: &WaitRequest<'_>) -> Wait {
        let was_present = (request.present)();
        let waited = self.luks.interaction.wait(request);
        if waited == Wait::Present && !was_present {
            self.settle();
        }
        waited
    }

    /// Says `text`, a line, with or without its newline.
    pub fn tell(&self, told: Told, text: &str) {
        self.luks
            .interaction
            .notify(Tell::new(told, text.trim_end_matches('\n')));
    }

    /// Says what comes next is about `text`, with `device` in it
    /// highlighted.
    pub fn heading(&self, text: &str, device: &str) {
        self.luks.interaction.notify(Tell {
            told: Told::Heading,
            text: text.trim_end_matches('\n'),
            highlight: Some(device),
        });
    }

    /// Seconds to wait for a security key, smart card or key device
    /// (`zbm.luks_token_wait`): up to six digits, 10 otherwise.
    pub fn token_wait(&self) -> u64 {
        self.option(&LUKS_TOKEN_WAIT)
            .filter(|wait| (1..=6).contains(&wait.len()))
            .filter(|wait| wait.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|wait| wait.parse().ok())
            .unwrap_or(10)
    }

    /// Whether LUKS2 tokens may be tried: on unless `zbm.luks_tokens` turns
    /// it off.
    pub fn tokens_enabled(&self) -> bool {
        self.options().enabled(&LUKS_TOKENS, &mut Vec::new())
    }

    /// `cryptsetup open --type luks --key-file=<key> <args> <device> <name>`;
    /// logs the output when it fails.
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
            Key::File(path) => zbm_crypt::Key::File(Path::new(path)),
            Key::Stdin(secret) => zbm_crypt::Key::Secret(secret),
        };
        let (status, output) = match closed_mapping(name, device) {
            Some(mapping) => match self.volumes().open(mapping, key, args) {
                Ok(_) => return Ok(()),
                Err(failed) => (failed.error.status(), failed.output),
            },
            None => (1, "not a device or mapping name".to_owned()),
        };
        zbm_trace::debug!(
            "cryptsetup open {device} {name} failed ({status}): {output}";
            %device, %name, %status, %output
        );
        Err(LuksError::Open {
            device: device.to_owned(),
            name: name.to_owned(),
            status,
        })
    }
}

/// The closed mapping of `device` as `name`, when both are what they say.
pub(crate) fn closed_mapping(name: &str, device: &str) -> Option<Mapping<Closed>> {
    Some(Mapping::new(device.parse().ok()?, name.parse().ok()?))
}

#[cfg(test)]
mod tests;
