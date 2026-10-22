//! Unlocking volumes: `luks_open_device`, `luks_wait_device`,
//! `luks_discover_devices` and `luks_may_hold_zfs`; unlocking them all is
//! [`Session::unlock_all`].

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use zbm_platform::console::Timed;
use zbm_tools::devices::Devices;
use zbm_tools::programs;

use crate::config::Options;
use crate::device::{
    canonical, default_name, describe_device, is_block_device, is_open, resolve_device,
};
use crate::error::LuksError;
use crate::keys::Target;
use crate::keys::gpg::is_gpg_keyfile;
use crate::session::{Answer, Key, PromptRequest, Session};

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

/// How [`Session::open_device`] found a volume open.
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

/// What [`Session::unlock_all`] did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnlockSummary {
    /// Volumes opened and kept open.
    pub unlocked: i64,
    /// Volumes that were not opened.
    pub failed: i64,
}

impl Session<'_> {
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
    fn opened(&self, name: &str, device: &str, auto: bool, method: Method) -> Opened {
        self.record_mapping(name, device, auto);
        Opened::Unlocked(method)
    }

    /// Unlocks one volume (`luks_open_device`), trying in order: a key script,
    /// a key file (decrypting a GnuPG key file), LUKS2 tokens, passphrases that
    /// unlocked other volumes, YubiKey challenge-response, and the passphrase
    /// prompt. `name` is the mapping name (empty for the default), `keyspec`
    /// the key file specification (may be empty), `options` the crypttab
    /// options and `auto` whether discovery found the volume.
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
        auto: bool,
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
        let target = Target {
            name: &name,
            device,
            args: &options.args,
        };

        // 1. A key script prints the key (Debian's crypttab semantics).
        if let Some(script) = &options.keyscript
            && self
                .open_with_keyscript(target, script, keyspec, options_text)
                .is_ok()
        {
            return Ok(self.opened(&name, device, auto, Method::KeyScript));
        }

        // 2. A key file, possibly on a key device. GnuPG-encrypted key files
        //    are decrypted in memory and kept for the other volumes of the run.
        //    With a key script, the key file field is the script's argument.
        if !keyspec.is_empty() && options.keyscript.is_none() {
            let cached = self
                .gpg_plain
                .get(keyspec)
                .filter(|plain| !plain.is_empty())
                .cloned();
            if let Some(plain) = cached {
                zbm_trace::info!(
                    "unlocking {device} with GnuPG key file {keyspec} (already decrypted)"
                );
                if self
                    .cryptsetup_open(&name, device, Key::Stdin(&plain), &options.args)
                    .is_ok()
                {
                    zbm_trace::info!("unlocked {device} as {name} with key file");
                    return Ok(self.opened(&name, device, auto, Method::KeyFile));
                }
                zbm_trace::warn!("key file {keyspec} failed to unlock {device}");
            } else {
                let found = self.find_keyfile(keyspec);
                if let Some(path) = &found.path {
                    let opened = if options.gpg || is_gpg_keyfile(path, "") {
                        match self.gpg_decrypt(path, device) {
                            Some(plain) => {
                                zbm_trace::info!(
                                    "unlocking {device} with GnuPG key file {keyspec}"
                                );
                                let opened = self
                                    .cryptsetup_open(
                                        &name,
                                        device,
                                        Key::Stdin(&plain),
                                        &options.args,
                                    )
                                    .is_ok();
                                // Kept for other volumes until the secrets are
                                // forgotten.
                                self.gpg_plain.insert(keyspec.to_owned(), plain);
                                opened
                            }
                            None => false,
                        }
                    } else {
                        zbm_trace::info!("unlocking {device} with key file {keyspec}");
                        self.cryptsetup_open(&name, device, Key::File(path), &options.args)
                            .is_ok()
                    };
                    self.release_keyfile(&found);
                    if opened {
                        zbm_trace::info!("unlocked {device} as {name} with key file");
                        return Ok(self.opened(&name, device, auto, Method::KeyFile));
                    }
                    zbm_trace::warn!("key file {keyspec} failed to unlock {device}");
                } else {
                    self.release_keyfile(&found);
                }
            }
        }

        // 3. LUKS2 tokens: TPM2, FIDO2 security keys and clevis.
        if !options.notoken && self.tokens_enabled() {
            if self.try_tokens(target, &header).is_ok() {
                return Ok(self.opened(&name, device, auto, Method::Token));
            }
            // Another tool may have unlocked the device meanwhile.
            if self.is_device_open(device) {
                return Ok(Opened::Elsewhere);
            }
        }

        // 4. Passphrases (and YubiKey responses) that unlocked another volume.
        let cached = self.passphrases.clone();
        for passphrase in &cached {
            if self
                .cryptsetup_open(&name, device, Key::Stdin(passphrase), &options.args)
                .is_ok()
            {
                zbm_trace::info!(
                    "unlocked {device} as {name} with a previously entered passphrase"
                );
                return Ok(self.opened(&name, device, auto, Method::CachedPassphrase));
            }
        }
        drop(cached);

        // 5. YubiKey challenge-response.
        if let Some(yubikey) = &options.yubikey
            && self.try_ykchalresp(target, yubikey, options.tries).is_ok()
        {
            return Ok(self.opened(&name, device, auto, Method::YubiKey));
        }

        // 6. Ask the user.
        self.ask_passphrase(&name, device, &options, auto)
    }

    /// Runs a key script with the crypttab variables and opens the volume with
    /// what it prints.
    fn open_with_keyscript(
        &self,
        target: Target<'_>,
        script: &str,
        keyspec: &str,
        options: &str,
    ) -> Result<(), LuksError> {
        let device = target.device;
        let executable = self
            .system()
            .files
            .metadata(Path::new(script))
            .is_ok_and(|metadata| metadata.is_executable());
        if !executable {
            zbm_trace::warn!("key script {script} is not executable");
            return Err(LuksError::KeyScriptNotExecutable {
                script: script.to_owned(),
            });
        }
        zbm_trace::info!("unlocking {device} with key script {script}");
        let key = if keyspec.is_empty() { "none" } else { keyspec };
        let mut env = self.exported().to_vec();
        env.extend([
            ("CRYPTTAB_NAME", target.name),
            ("CRYPTTAB_SOURCE", device),
            ("CRYPTTAB_KEY", key),
            ("CRYPTTAB_OPTIONS", options),
            ("CRYPTTAB_TRIED", "0"),
        ]);
        let printed = programs::key_script(self.system().runner, script, key, &env)
            .ok()
            .and_then(|output| output.secret)
            .unwrap_or_default();
        let opened = self.cryptsetup_open(target.name, device, Key::Stdin(&printed), target.args);
        match &opened {
            Ok(()) => zbm_trace::info!("unlocked {device} as {} with key script", target.name),
            Err(_) => zbm_trace::warn!("key script {script} failed to unlock {device}"),
        }
        opened
    }

    /// The passphrase prompt of `luks_open_device`. Echo is off for the whole
    /// prompt, and input is polled, so that an unlock performed elsewhere
    /// (`zluks` over SSH) is noticed while the prompt waits.
    fn ask_passphrase(
        &mut self,
        name: &str,
        device: &str,
        options: &Options,
        auto: bool,
    ) -> Result<Opened, LuksError> {
        let description = describe_device(self.system(), device);
        let console = self.system().console;
        loop {
            let mut attempt = 0;
            while options.tries == 0 || attempt < options.tries {
                attempt += 1;
                console.set_echo(false);
                self.tput(&["cnorm"]);
                self.say("\n");
                let line = format!(
                    "Unlock LUKS volume {}\n",
                    self.paint("yellow", &description)
                );
                self.say(&self.paint("cyan", &line));
                if attempt > 1 {
                    self.say(&self.paint("red", "No key available with this passphrase.\n"));
                }
                self.say("Passphrase (leave empty to skip): ");

                let passphrase = loop {
                    match console.read_secret_within(Duration::from_secs(2)) {
                        Ok(Timed::Read(passphrase)) => break passphrase,
                        Ok(Timed::TimedOut) => {
                            if self.is_device_open(device) {
                                console.set_echo(true);
                                self.say("\n");
                                zbm_trace::info!("{device} was unlocked elsewhere");
                                return Ok(Opened::Elsewhere);
                            }
                        }
                        Ok(Timed::Ended) | Err(_) => {
                            // Nothing more can be done interactively.
                            console.set_echo(true);
                            self.say("\n");
                            zbm_trace::warn!("no console available to unlock {device}");
                            return Err(LuksError::NoConsole {
                                device: device.to_owned(),
                            });
                        }
                    }
                };
                console.set_echo(true);
                self.say("\n");

                if passphrase.is_empty() {
                    zbm_trace::info!("skipping LUKS volume {device} at user request");
                    self.mark_skipped(device);
                    return Err(LuksError::Skipped {
                        device: device.to_owned(),
                    });
                }

                self.say(&format!("Unlocking {device} ...\n"));
                if self
                    .cryptsetup_open(name, device, Key::Stdin(&passphrase), &options.args)
                    .is_ok()
                {
                    self.passphrases.push(passphrase);
                    zbm_trace::info!("unlocked {device} as {name}");
                    return Ok(self.opened(name, device, auto, Method::Passphrase));
                }
                drop(passphrase);

                if self.is_device_open(device) {
                    zbm_trace::info!("{device} was unlocked elsewhere");
                    return Ok(Opened::Elsewhere);
                }
                zbm_trace::warn!("failed to unlock {device} (attempt {attempt})");
            }

            let request = PromptRequest {
                delay: None,
                message: format!("Unable to unlock LUKS volume {description}"),
                on_return: "try again".to_owned(),
                on_escape: "skip this volume".to_owned(),
            };
            let answer = self.timed_prompt(&request);
            self.tput(&["cnorm"]);
            self.tput(&["clear"]);
            if answer == Answer::Escape {
                zbm_trace::warn!("giving up on LUKS volume {device}");
                self.mark_skipped(device);
                return Err(LuksError::Skipped {
                    device: device.to_owned(),
                });
            }
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
#[cfg(test)]
mod tests;
