//! Unlocking volumes: `luks_open_device`, `luks_wait_device`,
//! `luks_discover_devices`, `luks_may_hold_zfs` and `luks_unlock_all`.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use zbm_core::exec::{Input, Stream};
use zbm_core::shell::{command_substitution, read_lines};
use zbm_system::console::Timed;

use crate::config::{Mode, Options, load_config};
use crate::device::{
    canonical, default_name, describe_device, is_block_device, is_open, resolve_device,
};
use crate::keys::Target;
use crate::keys::gpg::is_gpg_keyfile;
use crate::mappings::Mapping;
use crate::session::{Key, PromptRequest, Session};

impl Session<'_> {
    /// Logs the messages of a configuration step.
    fn log_all(&self, messages: &[zbm_log::Message]) {
        for message in messages {
            self.log().log(message);
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
    fn device_open(&self, device: &str) -> bool {
        is_open(self.system().files, device, &self.settings().open_check)
    }

    /// Records a mapping this run opened.
    fn opened(&self, name: &str, device: &str, auto: bool) -> bool {
        self.record_mapping(name, device, auto);
        true
    }

    /// Unlocks one volume (`luks_open_device`), trying in order: a key script,
    /// a key file (decrypting a GnuPG key file), LUKS2 tokens, passphrases that
    /// unlocked other volumes, YubiKey challenge-response, and the passphrase
    /// prompt. `name` is the mapping name (empty for the default), `keyspec`
    /// the key file specification (may be empty), `options` the crypttab
    /// options and `auto` whether discovery found the volume. Returns whether
    /// the volume is open afterwards.
    pub fn open_device(
        &mut self,
        name: &str,
        device: &str,
        keyspec: &str,
        options_text: &str,
        auto: bool,
    ) -> bool {
        let (options, messages) = Options::parse(options_text, self.settings().default_tries);
        self.log_all(&messages);

        if self.device_open(device) {
            self.log()
                .info(format!("LUKS device {device} is already in use, skipping"));
            return true;
        }

        let header = options.header.clone().unwrap_or_else(|| device.to_owned());
        let is_luks = self
            .command("cryptsetup")
            .args(["isLuks", &header])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        let luks = self
            .system()
            .runner
            .run(&is_luks)
            .is_ok_and(|output| output.status.success());
        if !luks {
            self.log().warning(format!(
                "{header} is not a LUKS volume (header), skipping {device}"
            ));
            return false;
        }

        let name = if name.is_empty() {
            default_name(self.system(), &header)
        } else {
            name.to_owned()
        };
        if self.system().files.exists(Path::new(&self.mapper(&name))) {
            self.log().error(format!(
                "cannot unlock {device}: mapping {name} already exists"
            ));
            return false;
        }
        let target = Target {
            name: &name,
            device,
            args: &options.args,
        };

        // 1. A key script prints the key (Debian's crypttab semantics).
        if let Some(script) = &options.keyscript {
            if self.open_with_keyscript(target, script, keyspec, options_text) {
                return self.opened(&name, device, auto);
            }
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
                self.log().info(format!(
                    "unlocking {device} with GnuPG key file {keyspec} (already decrypted)"
                ));
                if self.cryptsetup_open(&name, device, Key::Stdin(&plain), &options.args) {
                    self.log()
                        .info(format!("unlocked {device} as {name} with key file"));
                    return self.opened(&name, device, auto);
                }
                self.log()
                    .warning(format!("key file {keyspec} failed to unlock {device}"));
            } else {
                let found = self.find_keyfile(keyspec);
                if let Some(path) = &found.path {
                    let opened = if options.gpg || is_gpg_keyfile(path, "") {
                        match self.gpg_decrypt(path, device) {
                            Some(plain) => {
                                self.log().info(format!(
                                    "unlocking {device} with GnuPG key file {keyspec}"
                                ));
                                let opened = self.cryptsetup_open(
                                    &name,
                                    device,
                                    Key::Stdin(&plain),
                                    &options.args,
                                );
                                // Kept for other volumes until the secrets are
                                // forgotten.
                                self.gpg_plain.insert(keyspec.to_owned(), plain);
                                opened
                            }
                            None => false,
                        }
                    } else {
                        self.log()
                            .info(format!("unlocking {device} with key file {keyspec}"));
                        self.cryptsetup_open(&name, device, Key::File(path), &options.args)
                    };
                    self.release_keyfile(&found);
                    if opened {
                        self.log()
                            .info(format!("unlocked {device} as {name} with key file"));
                        return self.opened(&name, device, auto);
                    }
                    self.log()
                        .warning(format!("key file {keyspec} failed to unlock {device}"));
                } else {
                    self.release_keyfile(&found);
                }
            }
        }

        // 3. LUKS2 tokens: TPM2, FIDO2 security keys and clevis.
        if !options.notoken && self.tokens_enabled() {
            if self.try_tokens(target, &header) {
                return self.opened(&name, device, auto);
            }
            // Another tool may have unlocked the device meanwhile.
            if self.device_open(device) {
                return true;
            }
        }

        // 4. Passphrases (and YubiKey responses) that unlocked another volume.
        let cached = self.passphrases.clone();
        for passphrase in &cached {
            if self.cryptsetup_open(&name, device, Key::Stdin(passphrase), &options.args) {
                self.log().info(format!(
                    "unlocked {device} as {name} with a previously entered passphrase"
                ));
                return self.opened(&name, device, auto);
            }
        }
        drop(cached);

        // 5. YubiKey challenge-response.
        if let Some(yubikey) = &options.yubikey {
            if self.try_ykchalresp(target, yubikey, options.tries) {
                return self.opened(&name, device, auto);
            }
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
    ) -> bool {
        let device = target.device;
        let executable = self
            .system()
            .files
            .metadata(Path::new(script))
            .is_ok_and(|metadata| metadata.is_executable());
        if !executable {
            self.log()
                .warning(format!("key script {script} is not executable"));
            return false;
        }
        self.log()
            .info(format!("unlocking {device} with key script {script}"));
        let key = if keyspec.is_empty() { "none" } else { keyspec };
        let command = self
            .command(script)
            .arg(key)
            .env("CRYPTTAB_NAME", target.name)
            .env("CRYPTTAB_SOURCE", device)
            .env("CRYPTTAB_KEY", key)
            .env("CRYPTTAB_OPTIONS", options)
            .env("CRYPTTAB_TRIED", "0")
            .stdin(Input::Inherit)
            .stdout(Stream::CaptureSecret);
        let printed = self
            .system()
            .runner
            .run(&command)
            .ok()
            .and_then(|output| output.secret)
            .unwrap_or_default();
        if self.cryptsetup_open(target.name, device, Key::Stdin(&printed), target.args) {
            self.log().info(format!(
                "unlocked {device} as {} with key script",
                target.name
            ));
            return true;
        }
        self.log()
            .warning(format!("key script {script} failed to unlock {device}"));
        false
    }

    /// The passphrase prompt of `luks_open_device`. Echo is off for the whole
    /// prompt, and input is polled, so that an unlock performed elsewhere
    /// (`zluks` over SSH) is noticed while the prompt waits.
    fn ask_passphrase(&mut self, name: &str, device: &str, options: &Options, auto: bool) -> bool {
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
                            if self.device_open(device) {
                                console.set_echo(true);
                                self.say("\n");
                                self.log().info(format!("{device} was unlocked elsewhere"));
                                return true;
                            }
                        }
                        Ok(Timed::Ended) | Err(_) => {
                            // Nothing more can be done interactively.
                            console.set_echo(true);
                            self.say("\n");
                            self.log()
                                .warning(format!("no console available to unlock {device}"));
                            return false;
                        }
                    }
                };
                console.set_echo(true);
                self.say("\n");

                if passphrase.is_empty() {
                    self.log()
                        .info(format!("skipping LUKS volume {device} at user request"));
                    self.mark_skipped(device);
                    return false;
                }

                self.say(&format!("Unlocking {device} ...\n"));
                if self.cryptsetup_open(name, device, Key::Stdin(&passphrase), &options.args) {
                    self.passphrases.push(passphrase);
                    self.log().info(format!("unlocked {device} as {name}"));
                    return self.opened(name, device, auto);
                }
                drop(passphrase);

                if self.device_open(device) {
                    self.log().info(format!("{device} was unlocked elsewhere"));
                    return true;
                }
                self.log()
                    .warning(format!("failed to unlock {device} (attempt {attempt})"));
            }

            let request = PromptRequest {
                delay: None,
                message: format!("Unable to unlock LUKS volume {description}"),
                on_return: "try again".to_owned(),
                on_escape: "skip this volume".to_owned(),
            };
            let again = self.timed_prompt(&request);
            self.tput(&["cnorm"]);
            self.tput(&["clear"]);
            if !again {
                self.log()
                    .warning(format!("giving up on LUKS volume {device}"));
                self.mark_skipped(device);
                return false;
            }
        }
    }

    /// Waits for the device of a specification (`luks_wait_device`); a
    /// missing device is skipped at once with `nofail`, and otherwise when the
    /// user says so.
    pub fn wait_device(&self, spec: &str, nofail: bool) -> Option<String> {
        let system = *self.system();
        loop {
            if let Some(device) = resolve_device(&system, self.log(), spec) {
                return Some(device);
            }
            self.settle();
            if let Some(device) = resolve_device(&system, self.log(), spec) {
                return Some(device);
            }
            if nofail {
                self.log()
                    .info(format!("optional LUKS device {spec} not found, skipping"));
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
            if !self.timed_prompt(&request) {
                self.tput(&["cnorm"]);
                self.tput(&["clear"]);
                self.log().error(format!("LUKS device {spec} not found"));
                return None;
            }
        }
    }

    /// Every LUKS block device on the machine (`luks_discover_devices`).
    pub fn discover_devices(&self) -> Vec<String> {
        let system = self.system();
        let blkid = self
            .command("blkid")
            .args(["-c", "/dev/null", "-o", "device", "-t", "TYPE=crypto_LUKS"])
            .stderr(Stream::Null);
        let output = system
            .runner
            .run(&blkid)
            .map(|output| output.stdout_text())
            .unwrap_or_default();
        let devices: BTreeSet<String> = read_lines(&format!("{}\n", command_substitution(&output)))
            .into_iter()
            .filter(|device| is_block_device(system.files, device))
            .map(|device| canonical(system.files, &device))
            .collect();
        devices.into_iter().collect()
    }

    /// Whether an open mapping might hold a ZFS vdev (`luks_may_hold_zfs`):
    /// unknown contents (or a partitioned device) count as maybe.
    pub fn may_hold_zfs(&self, name: &str) -> bool {
        let blkid = self
            .command("blkid")
            .args(["-c", "/dev/null", "-p", "-s", "TYPE", "-o", "value"])
            .arg(self.mapper(name))
            .stderr(Stream::Null);
        let kind = match self.system().runner.run(&blkid) {
            Ok(output) if output.status.success() => {
                command_substitution(&output.stdout_text()).to_owned()
            }
            _ => String::new(),
        };
        kind.is_empty() || kind == "zfs_member"
    }

    /// `cryptsetup close <name>`.
    pub fn cryptsetup_close(&self, name: &str) -> bool {
        let close = self
            .command("cryptsetup")
            .args(["close", name])
            .stdout(Stream::Null)
            .stderr(Stream::Null);
        self.system()
            .runner
            .run(&close)
            .is_ok_and(|output| output.status.success())
    }

    /// Unlocks every configured or discovered LUKS volume
    /// (`luks_unlock_all`). Returns whether at least one volume was unlocked
    /// or none needed unlocking; a retry of the import loop succeeds only if
    /// it unlocked something new, or the loop would retry the import forever
    /// without offering the recovery shell.
    pub fn unlock_all(&mut self) -> bool {
        let retry = self.settings().retry;
        if !self.available() {
            self.log()
                .debug("cryptsetup is not available; LUKS support disabled");
            return !retry;
        }

        let zbm_luks = self.zbm_arg("zbm.luks");
        let crypttab_path = self.settings().crypttab.clone();
        let crypttab = self
            .system()
            .files
            .read(&crypttab_path)
            .ok()
            .map(|text| String::from_utf8_lossy(&text).into_owned());
        let (mode, messages) = load_config(
            zbm_luks.as_deref(),
            crypttab.as_deref(),
            &crypttab_path.to_string_lossy(),
        );
        self.log_all(&messages);
        if mode == Mode::Disabled {
            return !retry;
        }

        // The default key for volumes without one of their own.
        let global_key = self.zbm_arg("zbm.luks_key").unwrap_or_default();
        self.settle();
        let mut unlocked: i64 = 0;
        let mut failed: i64 = 0;
        self.forget_secrets();

        if mode == Mode::Auto {
            let mut devices = self.discover_devices();
            // On retries, only volumes that appeared since the last pass
            // matter.
            if retry {
                devices.retain(|device| !self.retry_ignores(device));
            }
            if devices.is_empty() {
                self.log().debug("no LUKS volumes found");
                return !retry;
            }
            self.log()
                .info(format!("discovered LUKS volumes: {}", devices.join(" ")));
            for device in &devices {
                if self.open_device("", device, &global_key, "", true) {
                    unlocked += 1;
                } else {
                    failed += 1;
                }
            }
        } else {
            let mut seen = BTreeSet::new();
            for entry in mode.entries() {
                let key = if entry.keyfile.is_empty() {
                    &global_key
                } else {
                    &entry.keyfile
                };
                let (options, messages) =
                    Options::parse(&entry.options, self.settings().default_tries);
                self.log_all(&messages);
                if options.noauto {
                    self.log()
                        .debug(format!("not unlocking {} (noauto)", entry.spec));
                    continue;
                }

                // Retries never wait: a device still missing is checked again
                // next time.
                let device = if retry {
                    let system = *self.system();
                    match resolve_device(&system, self.log(), &entry.spec) {
                        Some(device) if !self.retry_ignores(&device) => device,
                        _ => continue,
                    }
                } else {
                    match self.wait_device(&entry.spec, options.nofail) {
                        Some(device) => device,
                        None => {
                            if !options.nofail {
                                failed += 1;
                            }
                            continue;
                        }
                    }
                };

                if !seen.insert(device.clone()) {
                    self.log()
                        .warning(format!("LUKS device {device} is configured more than once"));
                    continue;
                }
                if self.open_device(&entry.name, &device, key, &entry.options, false) {
                    unlocked += 1;
                } else {
                    failed += 1;
                }
            }
        }

        self.forget_secrets();
        // udev creates the /dev/disk/by-* links of the new mappings.
        self.settle();

        // Discovered volumes that clearly do not hold ZFS are closed again.
        if let Some(mappings) = self.read_mappings() {
            let mut keep: Vec<Mapping> = Vec::new();
            for mapping in mappings {
                let name = mapping.name.as_str();
                if mapping.auto == "1"
                    && is_block_device(self.system().files, &self.mapper(name))
                    && !self.may_hold_zfs(name)
                {
                    self.log().info(format!(
                        "closing {name}: {} does not contain a ZFS vdev",
                        mapping.device
                    ));
                    if self.cryptsetup_close(name) {
                        unlocked -= 1;
                        continue;
                    }
                    self.log().warning(format!("unable to close {name}"));
                }
                keep.push(mapping);
            }
            self.write_mappings(&keep);
        }

        self.log().info(format!(
            "LUKS: {unlocked} volume(s) available, {failed} not unlocked"
        ));
        if retry {
            return unlocked > 0;
        }
        failed == 0 || unlocked > 0
    }
}

#[cfg(test)]
mod tests;
