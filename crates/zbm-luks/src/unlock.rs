//! Unlocking volumes: opening one volume with the first key method that
//! works, waiting for a missing device, finding every LUKS device and telling
//! whether a mapping might hold ZFS; unlocking them all is
//! [`Unlocker::unlock_all`].

use std::collections::BTreeSet;
use std::path::Path;

use zbm_tools::devices::Devices;
use zbm_types::{DevicePath, MappingName};

use crate::config::Options;
use crate::device::{
    canonical, describe_device, is_block_device, is_open, mapping_name, resolve_device,
};
use crate::error::LuksError;
use crate::mappings::Origin;
use crate::unlocker::{Key, Unlocker};
use zbm_platform::interaction::{Answer, PromptRequest, Subject};
use zbm_platform::interaction::{Asked, SecretKind, SecretRequest, Told, VolumeRef};

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
        is_open(self.context().files(), device, &self.settings().open_check)
    }

    /// The name to open `device` as, with the LUKS header `header`: `name`,
    /// or the default name, when no mapping has it yet.
    fn free_name(
        &self,
        name: Option<&MappingName>,
        header: &str,
        device: &str,
    ) -> Result<MappingName, LuksError> {
        let name = mapping_name(self.context(), name, header)?;
        if self
            .context()
            .files()
            .exists(Path::new(&self.mapper(name.as_str())))
        {
            zbm_trace::error!(
                "cannot unlock {device}: mapping {name} already exists";
                %device, %name
            );
            return Err(LuksError::MappingExists {
                name: name.to_string(),
            });
        }
        Ok(name)
    }

    /// Records a mapping this run opened with `method`.
    fn opened(&self, name: &str, device: &str, origin: Origin, method: Method) -> Opened {
        self.mappings().record(name, device, origin);
        Opened::Unlocked(method)
    }

    /// Unlocks one volume, trying in order: a key script, a key file
    /// (decrypting a GnuPG key file), LUKS2 tokens, passphrases that unlocked
    /// other volumes, YubiKey challenge-response, and the passphrase prompt.
    /// `name` is the mapping name (`None` for the default), `keyspec`
    /// the key file specification (may be empty), `options` the crypttab
    /// options and `origin` how the volume was found.
    ///
    /// # Errors
    ///
    /// When the volume is not open afterwards: [`LuksError::NotLuks`],
    /// [`LuksError::Name`], [`LuksError::MappingExists`], or why the
    /// passphrase prompt ended ([`LuksError::Skipped`],
    /// [`LuksError::NoConsole`]).
    pub fn open_device(
        &mut self,
        name: Option<&MappingName>,
        device: &DevicePath,
        keyspec: &str,
        options_text: &str,
        origin: Origin,
    ) -> Result<Opened, LuksError> {
        let device = device.as_str();
        let (options, messages) = Options::parse(options_text, self.settings().default_tries);
        log_all(&messages);

        if self.is_device_open(device) {
            zbm_trace::info!("LUKS device {device} is already in use, skipping"; %device);
            return Ok(Opened::AlreadyOpen);
        }

        let header = options.header.clone().unwrap_or_else(|| device.to_owned());
        if !self.cryptsetup().is_luks(&header) {
            zbm_trace::warn!(
                "{header} is not a LUKS volume (header), skipping {device}";
                %header, %device
            );
            return Err(LuksError::NotLuks { header });
        }

        let name = self.free_name(name, &header, device)?;
        let name = name.as_str();
        let volume = Volume {
            name,
            device,
            header: &header,
            keyspec,
            options: &options,
            options_text,
        };
        for method in key_methods() {
            match method.try_open(self, &volume) {
                Tried::Opened => return Ok(self.opened(name, device, origin, method.method())),
                Tried::Elsewhere => return Ok(Opened::Elsewhere),
                Tried::NotOpened => {}
            }
        }
        // Ask the user.
        self.ask_passphrase(name, device, &options, origin)
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
        let description = describe_device(self.context(), device);
        let volume = VolumeRef { name, device };
        let mut prompt = PassphrasePrompt::new(options.tries);
        let mut next = prompt.start();
        loop {
            next = match next {
                Next::Ask { attempt } => {
                    let asked = self.passphrase(volume, &description, attempt, options.tries);
                    let passphrase = match asked {
                        Asked::Secret(passphrase) => passphrase,
                        Asked::Elsewhere => {
                            zbm_trace::info!("{device} was unlocked elsewhere"; %device);
                            return Ok(Opened::Elsewhere);
                        }
                        Asked::Ended => {
                            zbm_trace::warn!("no console available to unlock {device}"; %device);
                            return Err(LuksError::NoConsole {
                                device: device.to_owned(),
                            });
                        }
                    };
                    if passphrase.is_empty() {
                        zbm_trace::info!("skipping LUKS volume {device} at user request"; %device);
                        return Err(self.skipped(device));
                    }
                    self.tell(Told::Working, &format!("Unlocking {device} ...\n"));
                    if self
                        .cryptsetup_open(name, device, Key::Stdin(&passphrase), &options.args)
                        .is_ok()
                    {
                        self.passphrases.push(passphrase);
                        zbm_trace::info!("unlocked {device} as {name}"; %device, %name);
                        return Ok(self.opened(name, device, origin, Method::Passphrase));
                    }
                    drop(passphrase);
                    if self.is_device_open(device) {
                        zbm_trace::info!("{device} was unlocked elsewhere"; %device);
                        return Ok(Opened::Elsewhere);
                    }
                    zbm_trace::warn!(
                        "failed to unlock {device} (attempt {attempt})";
                        %device, %attempt
                    );
                    prompt.failed()
                }
                Next::AskRetry => prompt.retry(self.ask_retry(volume, &description)),
                Next::GiveUp => {
                    zbm_trace::warn!("giving up on LUKS volume {device}"; %device);
                    return Err(self.skipped(device));
                }
            };
        }
    }

    /// Asks for the passphrase of `volume`, described as `description`,
    /// attempt `attempt` of `tries` (0: without limit).
    fn passphrase(
        &self,
        volume: VolumeRef<'_>,
        description: &str,
        attempt: u64,
        tries: u64,
    ) -> Asked {
        let device = volume.device;
        let elsewhere = || self.is_device_open(device);
        let request = SecretRequest {
            volume,
            kind: SecretKind::Passphrase,
            attempt: u32::try_from(attempt).unwrap_or(u32::MAX),
            tries: (tries > 0).then(|| u32::try_from(tries).unwrap_or(u32::MAX)),
            opened_elsewhere: Some(&elsewhere),
            description: Some(description),
        };
        self.ask(&request)
    }

    /// Asks whether to try `volume`, at `description`, again after a failed
    /// round.
    fn ask_retry(&self, volume: VolumeRef<'_>, description: &str) -> Answer {
        let request = PromptRequest {
            subject: Subject::Retry {
                name: volume.name.to_owned(),
                device: volume.device.to_owned(),
            },
            delay: None,
            message: format!("Unable to unlock LUKS volume {description}"),
            on_return: "try again".to_owned(),
            on_escape: "skip this volume".to_owned(),
        };
        self.timed_prompt(&request)
    }

    /// Records that the user skipped `device`, and the error that says so.
    fn skipped(&self, device: &str) -> LuksError {
        self.mappings().mark_skipped(device);
        LuksError::Skipped {
            device: device.to_owned(),
        }
    }

    /// Waits for the device of a specification; a missing device is skipped
    /// at once with `nofail`, and otherwise when the user says so.
    pub fn wait_device(&self, spec: &str, nofail: bool) -> Option<DevicePath> {
        let context = *self.context();
        loop {
            if let Some(device) = resolve_device(&context, spec) {
                return Some(device);
            }
            self.settle();
            if let Some(device) = resolve_device(&context, spec) {
                return Some(device);
            }
            if nofail {
                zbm_trace::info!("optional LUKS device {spec} not found, skipping"; %spec);
                return None;
            }
            let request = PromptRequest {
                subject: Subject::Missing(spec.to_owned()),
                delay: Some(self.settings().retry_delay),
                message: format!("LUKS device {spec} is missing"),
                on_return: "retry now".to_owned(),
                on_escape: "skip this device".to_owned(),
            };
            if self.timed_prompt(&request) == Answer::Escape {
                zbm_trace::error!("LUKS device {spec} not found"; %spec);
                return None;
            }
        }
    }

    /// Every LUKS block device on the machine.
    pub fn discover_devices(&self) -> Vec<DevicePath> {
        let context = *self.context();
        let output = Devices::new(context.runner(), context.files()).luks_devices();
        let devices: BTreeSet<DevicePath> = output
            .lines()
            .map(str::trim)
            .filter(|device| is_block_device(context.files(), device))
            .filter_map(|device| canonical(context.files(), device).parse().ok())
            .collect();
        devices.into_iter().collect()
    }

    /// Whether an open mapping might hold a ZFS vdev: unknown contents (or a
    /// partitioned device) count as maybe.
    pub fn may_hold_zfs(&self, name: &MappingName) -> bool {
        let kind = Devices::new(self.context().runner(), self.context().files())
            .content_type(&self.mapper(name.as_str()));
        kind.is_empty() || kind == "zfs_member"
    }

    /// `cryptsetup close <name>`.
    ///
    /// # Errors
    ///
    /// [`LuksError::Close`] when cryptsetup fails, for example because the
    /// mapping is in use.
    pub fn cryptsetup_close(&self, name: &MappingName) -> Result<(), LuksError> {
        self.cryptsetup()
            .close_named(name.as_str())
            .map_err(|_| LuksError::Close {
                name: name.to_string(),
            })
    }
}

mod all;
pub mod methods;
pub mod prompt;

use methods::{Tried, Volume, key_methods};
use prompt::{Next, PassphrasePrompt};

/// Logs the messages of a configuration step.
fn log_all(messages: &[zbm_trace::Message]) {
    for message in messages {
        message.emit();
    }
}

#[cfg(test)]
mod tests;
