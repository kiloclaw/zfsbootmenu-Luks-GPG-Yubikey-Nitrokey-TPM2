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
//!
//! The PIN is turned into that value as the token records it. A token
//! whose Argon2id fields systemd's parser refuses is refused, with a PIN
//! or without. Without the plugin, a PIN token that only the plugin can
//! open (a signed or pcrlock policy, a tool missing) is reported before any
//! PIN is asked for.
//!
//! A PIN that systemd 262 hardened with Argon2id (`--tpm2-with-pin=yes`)
//! is only ever unsealed here, never by the plugin, which before systemd
//! 262 derives it the old way and spends a try of the TPM's lockout on it.
//! Before its PIN is asked for, the token is checked: the native unsealing
//! can open it, and the derivation is within the memory and work allowed.
//! After the sealed object is loaded, the PIN is derived
//! (the checks run again first), its second half authorizes the unseal,
//! and HKDF-SHA256 of its first half and the unsealed secret gives the
//! passphrase. A key the TPM released that opens no key slot ends the
//! token: the PIN was right, so asking again would not help.

mod argon2id;
mod kdf;

use std::path::Path;
use std::time::Duration;

use zbm_crypt::tpm2::{TOOLS, Tpm2};
use zbm_platform::files::FileKind;
use zbm_platform::secret::Secret;
use zbm_tools::modules::Modules;

use super::{Outcome, Target};
use crate::crypto::{base64_decode, base64_encode, hex, pbkdf2_hmac_sha256, sha256};
use crate::error::{LuksError, TokenFailure};
use crate::keys::OpenMapping;
use crate::unlocker::{Key, Unlocker};
pub(crate) use argon2id::Argon2idRun;
use argon2id::{Ready, passphrase};
use kdf::{Argon2idParams, PinKdf, TooCostly, check_systemd_rules};
use zbm_crypt::Token;
use zbm_platform::interaction::{SecretKind, SecretRequest, Told};

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
            let context = self.context();
            let modules = Modules::new(context.runner(), context.files(), context.modules());
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
        match self.native_unseal(token, pin) {
            Unsealed::Key(key) => Outcome::Key(key),
            Unsealed::Failed => Outcome::Failed,
            Unsealed::Unsupported => Outcome::Unsupported,
            Unsealed::TooCostly(_) => Outcome::TooCostly,
        }
    }

    /// Unseals a systemd-tpm2 token with tpm2-tools, and says why not.
    fn native_unseal(&self, token: &Token, pin: Option<&Secret>) -> Unsealed {
        match self.unseal_plan(token, pin) {
            Some(plan) => self.unseal(&plan),
            None => Unsealed::Unsupported,
        }
    }

    /// What unsealing `token` needs; `None` when it cannot be unsealed here.
    fn unseal_plan(&self, token: &Token, pin: Option<&Secret>) -> Option<Unseal> {
        let sealed = self.native_support(token)?;
        let auth = match pin {
            None => Auth::None,
            Some(pin) => match PinKdf::of(token).ok()? {
                PinKdf::Plain => Auth::Value(auth_value(pin.expose(), None)),
                PinKdf::Pbkdf2 { salt } => Auth::Value(auth_value(pin.expose(), Some(&salt))),
                PinKdf::Argon2id { salt, params } => Auth::Argon2id {
                    pin: pin.duplicate(),
                    salt,
                    params,
                },
            },
        };
        Some(Unseal {
            private: sealed.private,
            public: sealed.public,
            pcrs: token.list("tpm2-pcrs").unwrap_or_default().join(","),
            bank: token.text("tpm2-pcr-bank").unwrap_or("sha256").to_owned(),
            auth,
        })
    }

    /// The sealed object of `token`, when ZFSBootMenu's own unsealing can
    /// open it; why not, otherwise, at debug level.
    fn native_support(&self, token: &Token) -> Option<Sealed> {
        let context = *self.context();
        if let Some(tool) = TOOLS.iter().find(|tool| !context.command_exists(tool)) {
            zbm_trace::debug!("native TPM2 unlocking needs {tool}"; %tool);
            return None;
        }
        // Signed PCR policies, pcrlock and pre-254 primary keys need the
        // plugin.
        if token.text("tpm2_pubkey").is_some() || token.flag("tpm2_pcrlock") == Some(true) {
            zbm_trace::debug!("TPM2 token uses a signed or pcrlock policy");
            return None;
        }
        if token.text("tpm2_srk").is_none() {
            zbm_trace::debug!("TPM2 token predates systemd's persistent SRK");
            return None;
        }
        let blob = base64_decode(token.text("tpm2-blob")?).unwrap_or_default();
        let Some((private, public)) = split_blob(&blob) else {
            zbm_trace::debug!("TPM2 blob has an unsupported layout");
            return None;
        };
        Some(Sealed {
            private: private.to_vec(),
            public: public.to_vec(),
        })
    }

    /// Loads the sealed object of `plan` below the storage root key in a
    /// private directory, satisfies its policy and unseals it.
    fn unseal(&self, plan: &Unseal) -> Unsealed {
        let Some(work) = self
            .temporary_dir("/run/zbm-tpm2")
            .or_else(|| self.temporary_dir("/tmp/zbm-tpm2"))
        else {
            return Unsealed::Failed;
        };
        let files = self.context().files();
        let _ = files.set_mode(Path::new(&work), 0o700);
        let path = |name: &str| format!("{work}/{name}");
        let written = files.write(Path::new(&path("priv")), &plan.private).is_ok()
            && files.write(Path::new(&path("pub")), &plan.public).is_ok();

        let tpm = self.tpm2();
        let (object, context) = (path("obj.ctx"), path("session.ctx"));
        let (object, context) = (Path::new(&object), Path::new(&context));
        let loaded = written
            && tpm
                .load(Path::new(&path("pub")), Path::new(&path("priv")), object)
                .is_ok();
        let outcome = if loaded {
            self.authorize_and_unseal(&tpm, plan, object, context)
        } else {
            Unsealed::Failed
        };
        tpm.flush_transient();

        for name in ["priv", "pub", "obj.ctx", "session.ctx"] {
            let _ = files.remove_file(Path::new(&path(name)));
        }
        let _ = files.remove_dir(Path::new(&work));
        outcome
    }

    /// Unseals the loaded `object` of `plan` in a policy session whose
    /// context is kept in `context`: an Argon2id PIN is derived first, and
    /// the passphrase is then derived from what the TPM released.
    fn authorize_and_unseal(
        &self,
        tpm: &Tpm2<'_>,
        plan: &Unseal,
        object: &Path,
        context: &Path,
    ) -> Unsealed {
        let (auth, key1) = match &plan.auth {
            Auth::None => (None, None),
            Auth::Value(auth) => (Some(auth.duplicate()), None),
            Auth::Argon2id { pin, salt, params } => match self.argon2id_derive(pin, salt, params) {
                Ok(split) => (
                    Some(auth_value(split.tpm_pin.expose(), None)),
                    Some(split.key1),
                ),
                Err(why) => return Unsealed::TooCostly(why),
            },
        };
        // The session is flushed when it goes out of scope.
        let Ok(session) = tpm.policy_session(context) else {
            return Unsealed::Failed;
        };
        let pcrs = || session.pcrs(&format!("{}:{}", plan.bank, plan.pcrs));
        if (!plan.pcrs.is_empty() && pcrs().is_err())
            || (auth.is_some() && session.auth_value().is_err())
        {
            return Unsealed::Failed;
        }
        let Some(secret) = session.unseal(object, auth.as_ref()) else {
            return Unsealed::Failed;
        };
        Unsealed::Key(match key1 {
            Some(key1) => passphrase(&key1, &secret),
            None => base64_encode(secret.expose()),
        })
    }

    /// Unseals with ZFSBootMenu's own code and opens the volume; `Err` with
    /// why not: the key was not released, the token is unsupported or too
    /// costly, or the key opened no key slot.
    fn tpm2_native_open(
        &self,
        target: Target<'_>,
        token: &Token,
        pin: Option<&Secret>,
    ) -> Result<OpenMapping, TokenFailure> {
        let key = match self.native_unseal(token, pin) {
            Unsealed::Key(key) => key,
            Unsealed::Failed => return Err(TokenFailure::NotReleased),
            Unsealed::Unsupported => return Err(TokenFailure::Unsupported),
            Unsealed::TooCostly(why) => {
                self.refuse_costly(target, &token.id, &why);
                return Err(TokenFailure::TooCostly);
            }
        };
        self.cryptsetup_open(target, Key::Stdin(&key))
            .map_err(|error| {
                let (id, device) = (token.id.as_str(), target.device());
                zbm_trace::warn!(
                    "TPM2 token {id} on {device}: the TPM released the key, but it opened no \
                     key slot ({error})"; %id, %device, %error
                );
                TokenFailure::Mismatch
            })
    }

    /// Opens a volume with a systemd-tpm2 token. A PIN that already worked
    /// is tried once, then the user is asked, up to three times: wrong PINs
    /// count towards the TPM's dictionary-attack lockout, so no PIN is asked
    /// for during a lockout.
    ///
    /// # Errors
    ///
    /// [`LuksError::Token`] with the reason: no TPM2, a lockout, no PIN
    /// given, too many wrong PINs, a key not released, a token the image
    /// cannot open, one that is malformed or too costly to derive, or a key
    /// released that opened no key slot.
    pub fn try_tpm2_token(
        &mut self,
        target: Target<'_>,
        token: &Token,
    ) -> Result<OpenMapping, LuksError> {
        let id = token.id.as_str();
        let device = target.device();
        let failed = |failure| target.token_error("systemd-tpm2", id, failure);
        if !self.have_tpm2() {
            zbm_trace::info!("no TPM2 available for token {id} on {device}"; %id, %device);
            return Err(failed(TokenFailure::NoTpm));
        }
        // systemd refuses these fields in any token, with a PIN or without.
        if let Err(why) = check_systemd_rules(token) {
            self.refuse_malformed(target, token, &why);
            return Err(failed(TokenFailure::Malformed));
        }
        if token.flag("tpm2-pin") != Some(true) {
            self.check_physical_memory(target, token).map_err(failed)?;
            return self.try_tpm2_without_pin(target, id, token).map_err(failed);
        }
        if self.tpm2_in_lockout() {
            self.tpm2_lockout_message();
            zbm_trace::warn!(
                "TPM2 in dictionary-attack lockout; skipping PIN token {id} on {device}";
                %id, %device
            );
            return Err(failed(TokenFailure::Lockout));
        }
        let argon2id = self.check_pin_token(target, token).map_err(failed)?;
        self.try_tpm2_pins(target, id, token, argon2id)
            .map_err(failed)
    }

    /// Whether the PIN token `token` can be tried, checked before any PIN
    /// is asked for; the reason it cannot is a warning. An Argon2id PIN is
    /// checked whatever plugin the image has, and is found [`Ready`];
    /// another, without the plugin only.
    fn check_pin_token(
        &self,
        target: Target<'_>,
        token: &Token,
    ) -> Result<Option<Ready>, TokenFailure> {
        match PinKdf::of(token) {
            Err(why) => {
                self.refuse_malformed(target, token, &why);
                Err(TokenFailure::Malformed)
            }
            Ok(PinKdf::Argon2id { salt, params }) => {
                self.argon2id_ready(target, token, &salt, &params).map(Some)
            }
            // Without the plugin, a token only the plugin could open costs
            // no PIN.
            Ok(_) if !self.have_token_plugin("systemd-tpm2") => {
                self.check_native_pin_token(target, token).map(|()| None)
            }
            Ok(_) => Ok(None),
        }
    }

    /// Whether ZFSBootMenu's own unsealing can open the PIN token `token`;
    /// a warning when it cannot.
    fn check_native_pin_token(
        &self,
        target: Target<'_>,
        token: &Token,
    ) -> Result<(), TokenFailure> {
        if self.native_support(token).is_some() {
            return Ok(());
        }
        zbm_trace::warn!(
            "TPM2 token {id} on {device} needs the systemd-tpm2 cryptsetup plugin";
            id = %token.id, device = %target.device()
        );
        Err(TokenFailure::Unsupported)
    }

    /// A token without a PIN, which needs no interaction: the plugin, then
    /// ZFSBootMenu's own unsealing.
    fn try_tpm2_without_pin(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
    ) -> Result<OpenMapping, TokenFailure> {
        let device = target.device();
        if self.have_token_plugin("systemd-tpm2")
            && let Ok(open) = self.open_token_plugin(target, id, None)
        {
            return Ok(open);
        }
        let result = self.tpm2_native_open(target, token, None);
        if result == Err(TokenFailure::Unsupported) && !self.have_token_plugin("systemd-tpm2") {
            zbm_trace::warn!(
                "TPM2 token {id} on {device} needs the systemd-tpm2 cryptsetup plugin"; %id, %device
            );
        }
        let open = result?;
        zbm_trace::info!("unlocked {device} with TPM2"; %device);
        Ok(open)
    }

    /// A token with a PIN: each PIN that worked before, once, then up to
    /// three PINs the user types, each once its derivation is affordable.
    /// For an Argon2id token, `argon2id`, the cached PINs follow the rules
    /// of [`Unlocker::cached_tpm2_pins`].
    fn try_tpm2_pins(
        &mut self,
        target: Target<'_>,
        id: &str,
        token: &Token,
        argon2id: Option<Ready>,
    ) -> Result<OpenMapping, TokenFailure> {
        let device = target.device();
        let mut asked = 0;
        let cached = self.cached_tpm2_pins(target, argon2id.as_ref());
        for cached in cached.iter().map(Some).chain([None]) {
            loop {
                let pin = match cached {
                    Some((_, pin)) => pin.duplicate(),
                    None => self.ask_tpm2_pin(target, id, argon2id.as_ref(), &mut asked)?,
                };
                if let Some(open) = self.tpm2_open_with_pin(target, id, token, &pin)? {
                    // A typed PIN may be one an Argon2id token was not
                    // offered from the cache.
                    if !self.tpm2_pins.contains(&pin) {
                        self.tpm2_pins.push(pin);
                    }
                    zbm_trace::info!("unlocked {device} with TPM2 and PIN"; %device);
                    return Ok(open);
                }
                drop(pin);
                if let (Some((index, _)), Some(ready)) = (cached, &argon2id) {
                    self.argon2id.refused(*index, ready.sealed);
                }
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

    /// Asks for the PIN of the TPM2 token `id` of `target`, the `asked`th
    /// time, once the derivation of an Argon2id token, `argon2id`, is
    /// affordable.
    fn ask_tpm2_pin(
        &self,
        target: Target<'_>,
        id: &str,
        argon2id: Option<&Ready>,
        asked: &mut u32,
    ) -> Result<Secret, TokenFailure> {
        const TRIES: u32 = 3;
        *asked += 1;
        if *asked > TRIES {
            return Err(TokenFailure::TooManyTries);
        }
        self.argon2id_still_ready(target, id, argon2id)?;
        let request = SecretRequest {
            subject: target.volume().into(),
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

    /// Tries `pin`, with the plugin or ZFSBootMenu's own unsealing; the
    /// mapping, when the volume opened; `None` when the key was not
    /// released. A PIN that systemd 262 hardened with Argon2id never goes
    /// to the plugin: before systemd 262 it derives the PIN the old way,
    /// and the TPM counts the refusal towards its lockout.
    ///
    /// # Errors
    ///
    /// [`TokenFailure::Unsupported`] when only the plugin could open the
    /// token and the image has none; [`TokenFailure::TooCostly`] when the
    /// derivation could not run; [`TokenFailure::Mismatch`] when the TPM
    /// released a key that opened no key slot.
    fn tpm2_open_with_pin(
        &self,
        target: Target<'_>,
        id: &str,
        token: &Token,
        pin: &Secret,
    ) -> Result<Option<OpenMapping>, TokenFailure> {
        let argon2id = matches!(PinKdf::of(token), Ok(PinKdf::Argon2id { .. }));
        if !argon2id && self.have_token_plugin("systemd-tpm2") {
            return Ok(self.open_token_plugin(target, id, Some(pin)).ok());
        }
        match self.tpm2_native_open(target, token, Some(pin)) {
            Ok(open) => Ok(Some(open)),
            Err(TokenFailure::NotReleased) => Ok(None),
            Err(TokenFailure::Unsupported) => {
                let device = target.device();
                if argon2id {
                    argon2id::argon2id_unsupported(id, device);
                } else {
                    zbm_trace::warn!(
                        "TPM2 token {id} on {device} needs the systemd-tpm2 cryptsetup plugin";
                        %id, %device
                    );
                }
                Err(TokenFailure::Unsupported)
            }
            Err(failure) => Err(failure),
        }
    }
}

/// The sealed object of a token that ZFSBootMenu's own unsealing can open.
struct Sealed {
    private: Vec<u8>,
    public: Vec<u8>,
}

/// How the TPM checks a token's PIN.
enum Auth {
    /// No PIN.
    None,
    /// The authorization value, derived before the object is loaded.
    Value(Secret),
    /// A PIN hardened with Argon2id, derived once the object is loaded.
    Argon2id {
        /// The PIN as it was typed.
        pin: Secret,
        /// The token's salt.
        salt: Vec<u8>,
        /// The token's costs.
        params: Argon2idParams,
    },
}

/// What unsealing a token needs: the sealed object, its PCR policy and how
/// the TPM checks its PIN.
struct Unseal {
    private: Vec<u8>,
    public: Vec<u8>,
    pcrs: String,
    bank: String,
    auth: Auth,
}

/// How ZFSBootMenu's own unsealing of a token ended.
enum Unsealed {
    /// The LUKS passphrase.
    Key(Secret),
    /// The TPM did not release the key.
    Failed,
    /// Only the plugin can unseal the token.
    Unsupported,
    /// The PIN's derivation could not run, so the TPM was not asked.
    TooCostly(TooCostly),
}

#[cfg(test)]
mod tests;
