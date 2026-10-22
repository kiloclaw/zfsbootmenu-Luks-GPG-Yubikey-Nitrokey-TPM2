//! Unlocking volumes: `luks_open_device`, `luks_wait_device`,
//! `luks_discover_devices` and `luks_may_hold_zfs`; unlocking them all is
//! [`Unlocker::unlock_all`].

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use zbm_platform::console::Timed;
use zbm_tools::devices::Devices;

use crate::config::Options;
use crate::device::{
    canonical, default_name, describe_device, is_block_device, is_open, resolve_device,
};
use crate::error::LuksError;
use crate::mappings::Origin;
use crate::unlocker::{Answer, Key, PromptRequest, Unlocker};

/// The key method that opened a volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// A key script of the crypttab.
    KeyScript,
    /// A key file, decrypted with GnuPG if it was encrypted.
    KeyFile,
    /// A LUKS2 token: TPM2, FIDO2 or clevis.
    Token,
    /// A passphrase (or YubiKey response) that opened another volume.
    CachedPassphrase,
    /// YubiKey challenge-response.
    YubiKey,
    /// The passphrase the user typed.
    Passphrase,
}

/// How [`Unlocker::open_device`] found a volume open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Opened {
    /// It was in use before.
    AlreadyOpen,
    /// Another program opened it meanwhile, such as `zluks` over SSH.
    Elsewhere,
    /// This run opened it with this method, and recorded the mapping.
    Unlocked(Method),
}

/// What [`Unlocker::unlock_all`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnlockSummary {
    /// Volumes opened and kept open.
    pub unlocked: i64,
    /// Volumes that were not opened.
    pub failed: i64,
}

impl Unlocker<'_> {
    /// Logs the messages of a configuration step.
    fn log_all(&self, messages: &[zbm_trace::Message]) {
        for message in messages {
            message.emit();
        }
    }

    /// The path of a mapping.
    fn mapper(&self, name: &str) -> String {
        self.settings()
            .mapper
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    /// Whether `device` is open, as the settings tell.
    fn is_device_open(&self, device: &str) -> bool {
        is_open(self.system().files, device, &self.settings().open_check)
    }

    /// Records a mapping this run opened with `method`.
    fn opened(&self, name: &str, device: &str, origin: Origin, method: Method) -> Opened {
        self.mappings().record(name, device, origin);
        Opened::Unlocked(method)
    }

    /// Unlocks one volume (`luks_open_device`), trying in order: a key script,
    /// a key file (decrypting a GnuPG key file), LUKS2 tokens, passphrases that
    /// unlocked other volumes, YubiKey challenge-response, and the passphrase
    /// prompt. `name` is the mapping name (empty for the default), `keyspec`
    /// the key file specification (may be empty), `options` the crypttab
    /// options and `origin` how the volume was found.
    ///
    /// # Errors
    ///
    /// When the volume is not open afterwards: [`LuksError::NotLuks`],
    /// [`LuksError::MappingExists`], or why the passphrase prompt ended
    /// ([`LuksError::Skipped`], [`LuksError::NoConsole`]).
    pub fn open_device(
        &mut self,
        name: &str,
        device: &str,
        keyspec: &str,
        options_text: &str,
        origin: Origin,
    ) -> Result<Opened, LuksError> {
        let (options, messages) = Options::parse(options_text, self.settings().default_tries);
        self.log_all(&messages);

        if self.is_device_open(device) {
            zbm_trace::info!("LUKS device {device} is already in use, skipping");
            return Ok(Opened::AlreadyOpen);
        }

        let header = options.header.clone().unwrap_or_else(|| device.to_owned());
        if !self.cryptsetup().is_luks(&header) {
            zbm_trace::warn!("{header} is not a LUKS volume (header), skipping {device}");
            return Err(LuksError::NotLuks { header });
        }

        let name = if name.is_empty() {
            default_name(self.system(), &header)
        } else {
            name.to_owned()
        };
        if self.system().files.exists(Path::new(&self.mapper(&name))) {
            zbm_trace::error!("cannot unlock {device}: mapping {name} already exists");
            return Err(LuksError::MappingExists { name });
        }
        let volume = Volume {
            name: &name,
            device,
            header: &header,
            keyspec,
            options: &options,
            options_text,
        };
        for method in key_methods() {
            match method.try_open(self, &volume) {
                Tried::Opened => return Ok(self.opened(&name, device, origin, method.method())),
                Tried::Elsewhere => return Ok(Opened::Elsewhere),
                Tried::NotOpened => {}
            }
        }
        // Ask the user.
        self.ask_passphrase(&name, device, &options, origin)
    }

    /// The passphrase prompt, driven by [`PassphrasePrompt`]. Echo is off
    /// while a passphrase is read, and input is polled, so that an unlock
    /// performed elsewhere (`zluks` over SSH) is noticed while the prompt
    /// waits.
    fn ask_passphrase(
        &mut self,
        name: &str,
        device: &str,
        options: &Options,
        origin: Origin,
    ) -> Result<Opened, LuksError> {
        let description = describe_device(self.system(), device);
        let mut prompt = PassphrasePrompt::new(options.tries);
        let mut next = prompt.start();
        loop {
            next = match next {
                Next::Ask { attempt } => {
                    self.show_passphrase_prompt(&description, attempt);
                    let passphrase = match self.read_passphrase(device)? {
                        Read::Passphrase(passphrase) => passphrase,
                        Read::Elsewhere => return Ok(Opened::Elsewhere),
                    };
                    if passphrase.is_empty() {
                        zbm_trace::info!("skipping LUKS volume {device} at user request");
                        return Err(self.skipped(device));
                    }
                    self.say(&format!("Unlocking {device} ...\n"));
                    if self
                        .cryptsetup_open(name, device, Key::Stdin(&passphrase), &options.args)
                        .is_ok()
                    {
                        self.passphrases.push(passphrase);
                        zbm_trace::info!("unlocked {device} as {name}");
                        return Ok(self.opened(name, device, origin, Method::Passphrase));
                    }
                    drop(passphrase);
                    if self.is_device_open(device) {
                        zbm_trace::info!("{device} was unlocked elsewhere");
                        return Ok(Opened::Elsewhere);
                    }
                    zbm_trace::warn!("failed to unlock {device} (attempt {attempt})");
                    prompt.failed()
                }
                Next::AskRetry => prompt.retry(self.ask_retry(&description)),
                Next::GiveUp => {
                    zbm_trace::warn!("giving up on LUKS volume {device}");
                    return Err(self.skipped(device));
                }
            };
        }
    }

    /// Draws the prompt for attempt `attempt` at `description`.
    fn show_passphrase_prompt(&self, description: &str, attempt: u64) {
        self.system().console.set_echo(false);
        self.tput(&["cnorm"]);
        self.say("\n");
        let line = format!("Unlock LUKS volume {}\n", self.paint("yellow", description));
        self.say(&self.paint("cyan", &line));
        if attempt > 1 {
            self.say(&self.paint("red", "No key available with this passphrase.\n"));
        }
        self.say("Passphrase (leave empty to skip): ");
    }

    /// Reads a passphrase, noticing when `device` is opened elsewhere
    /// meanwhile; echo is on again afterwards.
    ///
    /// # Errors
    ///
    /// [`LuksError::NoConsole`] when the console has no more input.
    fn read_passphrase(&self, device: &str) -> Result<Read, LuksError> {
        let console = self.system().console;
        let read = loop {
            match console.read_secret_within(Duration::from_secs(2)) {
                Ok(Timed::Read(passphrase)) => break Ok(Read::Passphrase(passphrase)),
                Ok(Timed::TimedOut) if self.is_device_open(device) => {
                    zbm_trace::info!("{device} was unlocked elsewhere");
                    break Ok(Read::Elsewhere);
                }
                Ok(Timed::TimedOut) => {}
                Ok(Timed::Ended) | Err(_) => {
                    // Nothing more can be done interactively.
                    break Err(LuksError::NoConsole {
                        device: device.to_owned(),
                    });
                }
            }
        };
        console.set_echo(true);
        self.say("\n");
        if read.is_err() {
            zbm_trace::warn!("no console available to unlock {device}");
        }
        read
    }

    /// Asks whether to try `description` again after a failed round.
    fn ask_retry(&self, description: &str) -> Answer {
        let request = PromptRequest {
            delay: None,
            message: format!("Unable to unlock LUKS volume {description}"),
            on_return: "try again".to_owned(),
            on_escape: "skip this volume".to_owned(),
        };
        let answer = self.timed_prompt(&request);
        self.tput(&["cnorm"]);
        self.tput(&["clear"]);
        answer
    }

    /// Records that the user skipped `device`, and the error that says so.
    fn skipped(&self, device: &str) -> LuksError {
        self.mappings().mark_skipped(device);
        LuksError::Skipped {
            device: device.to_owned(),
        }
    }

    /// Waits for the device of a specification (`luks_wait_device`); a
    /// missing device is skipped at once with `nofail`, and otherwise when the
    /// user says so.
    pub fn wait_device(&self, spec: &str, nofail: bool) -> Option<String> {
        let system = *self.system();
        loop {
            if let Some(device) = resolve_device(&system, spec) {
                return Some(device);
            }
            self.settle();
            if let Some(device) = resolve_device(&system, spec) {
                return Some(device);
            }
            if nofail {
                zbm_trace::info!("optional LUKS device {spec} not found, skipping");
                return None;
            }
            let delay = match self.settings().retry_delay.as_str() {
                "" => "5".to_owned(),
                delay => delay.to_owned(),
            };
            let request = PromptRequest {
                delay: Some(delay),
                message: format!("LUKS device {spec} is missing"),
                on_return: "retry now".to_owned(),
                on_escape: "skip this device".to_owned(),
            };
            if self.timed_prompt(&request) == Answer::Escape {
                self.tput(&["cnorm"]);
                self.tput(&["clear"]);
                zbm_trace::error!("LUKS device {spec} not found");
                return None;
            }
        }
    }

    /// Every LUKS block device on the machine (`luks_discover_devices`).
    pub fn discover_devices(&self) -> Vec<String> {
        let system = self.system();
        let output = Devices::new(system.runner).luks_devices();
        let devices: BTreeSet<String> = output
            .lines()
            .map(|line| line.trim().to_owned())
            .collect::<Vec<_>>()
            .into_iter()
            .filter(|device| is_block_device(system.files, device))
            .map(|device| canonical(system.files, &device))
            .collect();
        devices.into_iter().collect()
    }

    /// Whether an open mapping might hold a ZFS vdev (`luks_may_hold_zfs`):
    /// unknown contents (or a partitioned device) count as maybe.
    pub fn may_hold_zfs(&self, name: &str) -> bool {
        let kind = Devices::new(self.system().runner).content_type(&self.mapper(name));
        kind.is_empty() || kind == "zfs_member"
    }

    /// `cryptsetup close <name>`.
    ///
    /// # Errors
    ///
    /// [`LuksError::Close`] when cryptsetup fails, for example because the
    /// mapping is in use.
    pub fn cryptsetup_close(&self, name: &str) -> Result<(), LuksError> {
        self.cryptsetup()
            .close_named(name)
            .map_err(|_| LuksError::Close {
                name: name.to_owned(),
            })
    }
}

mod all;
pub mod methods;
pub mod prompt;

use methods::{Tried, Volume, key_methods};
use prompt::{Next, PassphrasePrompt};

/// What reading a passphrase got.
enum Read {
    /// A passphrase; empty skips the volume.
    Passphrase(zbm_platform::Secret),
    /// Nothing: another program opened the volume meanwhile.
    Elsewhere,
}
#[cfg(test)]
mod tests;
