//! systemd-tpm2 tokens: whether a TPM is there and in lockout, unsealing
//! without cryptsetup's plugin, and trying a token.
//!
//! Without cryptsetup's plugin, the token is unsealed with tpm2-tools as
//! systemd's `tpm2_unseal()` does: the sealed object is loaded below the
//! storage root key, a policy session satisfies `PolicyPCR` (and
//! `PolicyAuthValue` when a PIN is set), and the LUKS passphrase is the base64
//! encoding of the unsealed secret. The secret is captured from
//! `tpm2_unseal`'s output and reaches `cryptsetup` on standard input; the PIN's
//! authorization value reaches `tpm2_unseal` on standard input as
//! `hex:<value>`, because tpm2-tools reads it as text.

use std::path::Path;
use std::time::Duration;

use zbm_crypt::tpm2::{TOOLS, Tpm2};
use zbm_platform::files::FileKind;
use zbm_platform::secret::Secret;
use zbm_tools::modules::Modules;

use super::{Outcome, Target};
use crate::crypto::{base64_decode, base64_encode, hex, pbkdf2_hmac_sha256, sha256};
use crate::error::{LuksError, TokenFailure};
use crate::unlocker::{Key, Unlocker};
use zbm_crypt::Token;
use zbm_platform::interaction::{SecretKind, SecretRequest, Told, VolumeRef};

/// The authorization value of a TPM2 PIN, in hexadecimal: the SHA-256 of the
/// PIN without trailing zero bytes. systemd 255 and later salt the PIN first:
/// the base64 encoding of PBKDF2-HMAC-SHA256 with the token's salt, 10000
/// rounds and 32 bytes.
pub fn auth_value(pin: &[u8], salt: Option<&[u8]>) -> Secret {
    let salted = salt.map(|salt| base64_encode(pbkdf2_hmac_sha256(pin, salt, 10_000, 32).expose()));
    let digest = sha256(salted.as_ref().map_or(pin, Secret::expose));
    let len = digest
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |last| last + 1);
    hex(digest.get(..len).unwrap_or_default())
}

/// The sizes of the `TPM2B_PRIVATE` and the `TPM2B_PUBLIC` at the start of a
/// sealed blob, which must make up all of it: anything after them is an
/// import seed, which only the plugin handles.
pub fn split_blob(blob: &[u8]) -> Option<(&[u8], &[u8])> {
    let size = |at: usize| -> Option<usize> {
        let [high, low] = *blob.get(at..at + 2)? else {
            return None;
        };
        Some(usize::from(high) << 8 | usize::from(low))
    };
    let private = size(0)? + 2;
    let public = size(private)? + 2;
    (private + public == blob.len()).then(|| blob.split_at(private))
}

impl Unlocker<'_> {
    /// tpm2-tools for the TPM of the settings.
    fn tpm2(&self) -> Tpm2<'_> {
        Tpm2::new(self.context().runner(), &self.settings().tpm2_tcti)
    }

    /// Whether the TPM device is there, as `[ -c ]` finds it.
    fn tpm2_device_present(&self, device: &str) -> bool {
        self.context()
            .files()
            .metadata(Path::new(device))
            .is_ok_and(|metadata| metadata.kind == FileKind::CharDevice)
    }

    /// Whether a TPM2 is available. A TPM that is not a device (a simulator)
    /// always is; otherwise the drivers are loaded once, and the device is
    /// given a moment to appear.
    pub fn have_tpm2(&mut self) -> bool {
        let Some(device) = self.settings().tpm2_tcti.strip_prefix("device:") else {
            return true;
        };
        if self.tpm2_device_present(device) {
            return true;
        }
        if !self.tpm2_probed {
            self.tpm2_probed = true;
            let modules = Modules::new(self.context().runner());
            for module in ["tpm_crb", "tpm_tis"] {
                modules.probe_quietly(module);
            }
            for _ in 0..30 {
                self.settle();
                if self.tpm2_device_present(device) {
                    return true;
                }
                // Give up early when the kernel found no TPM at all.
                let tpms = self.context().files().read_dir(Path::new("/sys/class/tpm"));
                if tpms.unwrap_or_default().is_empty() {
                    break;
                }
                self.context().clock().sleep(Duration::from_millis(100));
            }
        }
        self.tpm2_device_present(device)
    }

    /// Whether the TPM refuses PIN-protected objects after too many wrong
    /// PINs.
    pub fn tpm2_in_lockout(&self) -> bool {
        self.context().command_exists("tpm2_getcap") && self.tpm2().in_lockout()
    }

    /// Says that the TPM is in lockout.
    fn tpm2_lockout_message(&self) {
        let locked = "The TPM is locked against PIN guessing after too many wrong PINs.\n";
        self.tell(Told::Refused, locked);
        let other = "Use another unlock method; the lockout clears by itself over time.\n";
        self.tell(Told::Refused, other);
    }

    /// Unseals a systemd-tpm2 token with tpm2-tools, as systemd does.
    pub fn tpm2_native_unseal(&self, token: &Token, pin: Option<&Secret>) -> Outcome {
        match self.unseal_plan(token, pin) {
            Ok(plan) => self.unseal(&plan),
            Err(outcome) => outcome,
        }
    }

    /// What unsealing `token` needs, or why it cannot be unsealed here.
    fn unseal_plan(&self, token: &Token, pin: Option<&Secret>) -> Result<Unseal, Outcome> {
        let context = *self.context();
        if let Some(tool) = TOOLS.iter().find(|tool| !context.command_exists(tool)) {
            zbm_trace::debug!("native TPM2 unlocking needs {tool}");
            return Err(Outcome::Unsupported);
        }
        // Signed PCR policies, pcrlock and pre-254 primary keys need the
        // plugin.
        if token.text("tpm2-pubkey").is_some() || token.flag("tpm2_pcrlock") == Some(true) {
            zbm_trace::debug!("TPM2 token uses a signed or pcrlock policy");
            return Err(Outcome::Unsupported);
        }
        if token.text("tpm2_srk").is_none() {
            zbm_trace::debug!("TPM2 token predates systemd's persistent SRK");
            return Err(Outcome::Unsupported);
        }
        let blob = token.text("tpm2-blob").ok_or(Outcome::Unsupported)?;
        let blob = base64_decode(blob).unwrap_or_default();
        let Some((private, public)) = split_blob(&blob) else {
            zbm_trace::debug!("TPM2 blob has an unsupported layout");
            return Err(Outcome::Unsupported);
        };
        let auth = pin.map(|pin| {
            let salt = token
                .text("tpm2_salt")
                .map(|salt| base64_decode(salt).unwrap_or_default());
            auth_value(pin.expose(), salt.as_deref())
        });
        Ok(Unseal {
            private: private.to_vec(),
            public: public.to_vec(),
            pcrs: token.list("tpm2-pcrs").unwrap_or_default().join(","),
            bank: token.text("tpm2-pcr-bank").unwrap_or("sha256").to_owned(),
            auth,
        })
    }

    /// Loads the sealed object of `plan` below the storage root key in a
    /// private directory, satisfies its policy and unseals it.
    fn unseal(&self, plan: &Unseal) -> Outcome {
        let Some(work) = self
            .temporary_dir("/run/zbm-tpm2")
            .or_else(|| self.temporary_dir("/tmp/zbm-tpm2"))
        else {
            return Outcome::Failed;
        };
        let files = self.context().files();
        let _ = files.set_mode(Path::new(&work), 0o700);
        let path = |name: &str| format!("{work}/{name}");
        let written = files.write(Path::new(&path("priv")), &plan.private).is_ok()
            && files.write(Path::new(&path("pub")), &plan.public).is_ok();

        let mut outcome = Outcome::Failed;
        let tpm = self.tpm2();
        let (object, context) = (path("obj.ctx"), path("session.ctx"));
        let (object, context) = (Path::new(&object), Path::new(&context));
        let loaded = written
            && tpm
                .load(Path::new(&path("pub")), Path::new(&path("priv")), object)
                .is_ok();
        // The session is flushed when it goes out of scope.
        if loaded
            && let Ok(session) = tpm.policy_session(context)
            && (plan.pcrs.is_empty()
                || session
                    .pcrs(&format!("{}:{}", plan.bank, plan.pcrs))
                    .is_ok())
            && (plan.auth.is_none() || session.auth_value().is_ok())
            && let Some(secret) = session.unseal(object, plan.auth.as_ref())
        {
            outcome = Outcome::Key(base64_encode(secret.expose()));
        }
        tpm.flush_transient();

        for name in ["priv", "pub", "obj.ctx", "session.ctx"] {
            let _ = files.remove_file(Path::new(&path(name)));
        }
        let _ = files.remove_dir(Path::new(&work));
        outcome
    }

    /// Unseals with ZFSBootMenu's own code and opens the volume; `Err` with
    /// whether the token is unsupported.
    fn tpm2_native_open(
        &self,
        target: Target<'_>,
        token: &Token,
        pin: Option<&Secret>,
    ) -> Result<(), TokenFailure> {
        match self.tpm2_native_unseal(token, pin) {
            Outcome::Key(key) => self
                .cryptsetup_open(target.name, target.device, Key::Stdin(&key), target.args)
                .map_err(|_| TokenFailure::NotReleased),
            Outcome::Failed => Err(TokenFailure::NotReleased),
            Outcome::Unsupported => Err(TokenFailure::Unsupported),
        }
    }

    /// Opens a volume with a systemd-tpm2 token. A PIN that already worked
    /// is tried once, then the user is asked, up to three times: wrong PINs
    /// count towards the TPM's dictionary-attack lockout, so no PIN is asked
    /// for during a lockout.
    ///
    /// # Errors
    ///
    /// [`LuksError::Token`] with the reason: no TPM2, a lockout, no PIN
    /// given, too many wrong PINs, a key not released, or a token the image
    /// cannot open.
    pub fn try_tpm2_token(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<(), LuksError> {
        let device = target.device;
        let failed = |failure| target.token_error("systemd-tpm2", id, failure);
        if !self.have_tpm2() {
            zbm_trace::info!("no TPM2 available for token {id} on {device}");
            return Err(failed(TokenFailure::NoTpm));
        }
        if token.flag("tpm2-pin") != Some(true) {
            return self.try_tpm2_without_pin(target, id, token).map_err(failed);
        }
        if self.tpm2_in_lockout() {
            self.tpm2_lockout_message();
            zbm_trace::warn!(
                "TPM2 in dictionary-attack lockout; skipping PIN token {id} on {device}"
            );
            return Err(failed(TokenFailure::Lockout));
        }
        self.try_tpm2_pins(target, id, token).map_err(failed)
    }

    /// A token without a PIN, which needs no interaction: the plugin, then
    /// ZFSBootMenu's own unsealing.
    fn try_tpm2_without_pin(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<(), TokenFailure> {
        let device = target.device;
        if self.have_token_plugin("systemd-tpm2")
            && self.open_token_plugin(target, id, None).is_ok()
        {
            return Ok(());
        }
        let result = self.tpm2_native_open(target, token, None);
        if result == Err(TokenFailure::Unsupported) && !self.have_token_plugin("systemd-tpm2") {
            zbm_trace::warn!(
                "TPM2 token {id} on {device} needs the systemd-tpm2 cryptsetup plugin"
            );
        }
        result?;
        zbm_trace::info!("unlocked {device} with TPM2");
        Ok(())
    }

    /// A token with a PIN: each PIN that worked before, once, then up to
    /// three PINs the user types.
    fn try_tpm2_pins(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<(), TokenFailure> {
        let device = target.device;
        let mut asked = 0;
        let cached: Vec<Secret> = self.tpm2_pins.iter().map(Secret::duplicate).collect();
        for cached in cached.iter().map(Some).chain([None]) {
            loop {
                let pin = match cached {
                    Some(pin) => pin.duplicate(),
                    None => self.ask_tpm2_pin(target, &mut asked)?,
                };
                if self.tpm2_open_with_pin(target, id, token, &pin)? {
                    if cached.is_none() {
                        self.tpm2_pins.push(pin);
                    }
                    zbm_trace::info!("unlocked {device} with TPM2 and PIN");
                    return Ok(());
                }
                drop(pin);
                if self.tpm2_in_lockout() {
                    self.tpm2_lockout_message();
                    return Err(TokenFailure::Lockout);
                }
                // A cached PIN is only ever tried once.
                if cached.is_some() {
                    break;
                }
                let refused = "The TPM did not release the key (wrong PIN or changed PCRs).\n";
                self.tell(Told::Refused, refused);
            }
        }
        Err(TokenFailure::NotReleased)
    }

    /// Asks for the PIN of the TPM2 token of `target`, the `asked`th time.
    fn ask_tpm2_pin(&self, target: Target<'_>, asked: &mut u32) -> Result<Secret, TokenFailure> {
        const TRIES: u32 = 3;
        *asked += 1;
        if *asked > TRIES {
            return Err(TokenFailure::TooManyTries);
        }
        let device = target.device;
        let request = SecretRequest {
            volume: VolumeRef {
                name: target.name,
                device,
            },
            kind: SecretKind::Tpm2Pin,
            attempt: *asked,
            tries: Some(TRIES),
            opened_elsewhere: None,
            description: None,
        };
        let asked = self.ask(&request);
        asked
            .secret()
            .filter(|pin| !pin.is_empty())
            .ok_or(TokenFailure::NoPin)
    }

    /// Tries `pin`, with the plugin or ZFSBootMenu's own unsealing; whether
    /// the volume opened.
    ///
    /// # Errors
    ///
    /// [`TokenFailure::Unsupported`] when only the plugin could open the
    /// token and the image has none.
    fn tpm2_open_with_pin(
        &self,
        target: Target<'_>,
        id: &str,
        token: &Token,
        pin: &Secret,
    ) -> Result<bool, TokenFailure> {
        if self.have_token_plugin("systemd-tpm2") {
            return Ok(self.open_token_plugin(target, id, Some(pin)).is_ok());
        }
        match self.tpm2_native_open(target, token, Some(pin)) {
            Ok(()) => Ok(true),
            Err(TokenFailure::Unsupported) => {
                zbm_trace::warn!(
                    "TPM2 token {id} on {} needs the systemd-tpm2 cryptsetup plugin",
                    target.device
                );
                Err(TokenFailure::Unsupported)
            }
            Err(_) => Ok(false),
        }
    }
}

/// What unsealing a token needs: the sealed object, its PCR policy and the
/// authorization value of its PIN.
struct Unseal {
    private: Vec<u8>,
    public: Vec<u8>,
    pcrs: String,
    bank: String,
    auth: Option<Secret>,
}

#[cfg(test)]
mod tests;
