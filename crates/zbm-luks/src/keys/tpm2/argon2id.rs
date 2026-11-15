//! TPM2 PINs that systemd 262 hardened with Argon2id
//! (`--tpm2-with-pin=yes`), which only ZFSBootMenu's own unsealing opens.
//!
//! A token's costs come from its LUKS header, which a tampered disk or a
//! hostile USB device chooses (input I8 of `docs/dev/threat-model.md`), and
//! a derivation cannot be stopped once it started but by ending the
//! keyholder. So each derivation is checked twice against the memory and
//! the work it may take ([`kdf::check_cost`]): before the token's PIN is
//! asked for, before each further PIN is asked for, and again just before
//! it runs, as the memory free and the run's budget change. The run keeps
//! the work it spent ([`Argon2idRun`]),
//! and the PINs it caches are tried without a prompt on the first Argon2id
//! token of a volume only, and never twice on the same sealed object, salt
//! and costs: a header with many tokens that copy one sealed object then
//! costs no more derivations than the run allows.
//!
//! Before Argon2 reserves its memory, a keyholder raises its own
//! `oom_score_adj` to 1000: if the memory runs out all the same, the
//! kernel ends the keyholder, whose client says so, rather than the menu.
//! A client that unlocks itself, as no keyholder could start, leaves its
//! own score alone, as it goes on as the menu.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;

use zbm_crypt::Token;
use zbm_platform::interaction::Told;
use zbm_platform::secret::Secret;
use zbm_types::DevicePath;

use super::kdf::{
    self, Argon2idParams, Malformed, Memory, RUN_BUDGET, TooCostly, check_physical, size,
};
use super::{Sealed, Target};
use crate::crypto::kdf::{Argon2idSplit, WorkMemory, argon2id_split, hkdf_sha256};
use crate::crypto::{base64_encode, sha256};
use crate::error::TokenFailure;
use crate::unlocker::Unlocker;

/// What HKDF is told the key it derives is for, as systemd tells it.
const HKDF_INFO: &[u8] = b"systemd-tpm2-argon2id-lock";

/// A hash of a token's sealed object, salt and Argon2id costs, which holds
/// nothing derived from a PIN.
pub(super) type Fingerprint = [u8; 32];

/// An Argon2id PIN token found ready to try: the fingerprint of its sealed
/// object, salt and costs, and its costs.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ready {
    /// The fingerprint of its sealed object, salt and costs.
    pub(super) sealed: Fingerprint,
    /// Its costs.
    params: Argon2idParams,
}

/// What an unlocking run keeps of its Argon2id derivations.
#[derive(Debug)]
pub(crate) struct Argon2idRun {
    /// The work of the derivations that started, in bytes · passes.
    pub(super) spent: Cell<u128>,
    /// The most work the run may spend: [`RUN_BUDGET`].
    pub(crate) budget: u128,
    /// The volumes whose first Argon2id token was given the cached PINs.
    given_cached: BTreeSet<String>,
    /// The cached PINs, by their place in the cache, that the TPM did not
    /// accept for a sealed object, salt and costs.
    refused: BTreeSet<(usize, Fingerprint)>,
}

impl Default for Argon2idRun {
    fn default() -> Argon2idRun {
        Argon2idRun {
            spent: Cell::new(0),
            budget: RUN_BUDGET,
            given_cached: BTreeSet::new(),
            refused: BTreeSet::new(),
        }
    }
}

impl Argon2idRun {
    /// Keeps that the cached PIN `index` did not open `sealed`.
    pub(super) fn refused(&mut self, index: usize, sealed: Fingerprint) {
        self.refused.insert((index, sealed));
    }

    /// Forgets which cached PINs were tried, as the cache is emptied. The
    /// work spent stays spent.
    pub(crate) fn forget_pins(&mut self) {
        self.given_cached.clear();
        self.refused.clear();
    }
}

/// The fingerprint of `sealed` with `salt` and `params`.
fn fingerprint(sealed: &Sealed, salt: &[u8], params: &Argon2idParams) -> Fingerprint {
    let mut data = Vec::new();
    for part in [&sealed.private[..], &sealed.public, salt] {
        let len = u64::try_from(part.len()).unwrap_or(u64::MAX);
        data.extend_from_slice(&len.to_le_bytes());
        data.extend_from_slice(part);
    }
    for cost in [params.memory_kib, params.iterations, params.lanes] {
        data.extend_from_slice(&cost.to_le_bytes());
    }
    *sha256(&data).as_bytes()
}

/// The LUKS passphrase of an Argon2id PIN token, from the first half of
/// the PIN's derivation, `key1`, and the secret the TPM unsealed: the
/// base64 of HKDF-SHA256, as systemd derives it.
pub(super) fn passphrase(key1: &Secret, unsealed: &Secret) -> Secret {
    base64_encode(hkdf_sha256(key1.expose(), unsealed.expose(), HKDF_INFO).expose())
}

/// Says that the TPM2 token `id` on `device` has an Argon2id PIN that the
/// native unsealing cannot open.
pub(super) fn argon2id_unsupported(id: &str, device: &DevicePath) {
    zbm_trace::warn!(
        "TPM2 token {id} on {device} uses systemd 262's Argon2id PIN, which needs \
         tpm2-tools in the image and no signed or pcrlock policy"; %id, %device
    );
}

impl Unlocker<'_> {
    /// Whether the Argon2id PIN token `token`, with `salt` and `params`, can
    /// be tried, checked before its PIN is asked for: the native unsealing
    /// can open it, and the derivation is affordable now.
    pub(super) fn argon2id_ready(
        &self,
        target: Target<'_>,
        token: &Token,
        salt: &[u8],
        params: &Argon2idParams,
    ) -> Result<Ready, TokenFailure> {
        let (id, device) = (token.id.as_str(), target.device());
        let Argon2idParams {
            memory_kib,
            iterations,
            lanes,
            ..
        } = *params;
        zbm_trace::debug!(
            "TPM2 token {id} on {device} uses systemd 262's Argon2id PIN \
             ({memory_kib} KiB, {iterations} passes, {lanes} lanes)";
            %id, %device, memory_kib, iterations, lanes
        );
        let Some(sealed) = self.native_support(token) else {
            argon2id_unsupported(id, device);
            return Err(TokenFailure::Unsupported);
        };
        let ready = Ready {
            sealed: fingerprint(&sealed, salt, params),
            params: *params,
        };
        self.argon2id_still_ready(target, id, Some(&ready))?;
        Ok(ready)
    }

    /// Whether a PIN of the TPM2 token `id` of `target` may be asked for:
    /// on an Argon2id token, `argon2id`, its derivation is affordable now.
    pub(super) fn argon2id_still_ready(
        &self,
        target: Target<'_>,
        id: &str,
        argon2id: Option<&Ready>,
    ) -> Result<(), TokenFailure> {
        let Some(ready) = argon2id else {
            return Ok(());
        };
        self.argon2id_affordable(&ready.params).map_err(|why| {
            self.refuse_costly(target, id, &why);
            TokenFailure::TooCostly
        })
    }

    /// Whether a derivation with `params` can run now.
    fn argon2id_affordable(&self, params: &Argon2idParams) -> Result<(), TooCostly> {
        let memory = Memory::read(self.context().files());
        let run = &self.argon2id;
        kdf::check_cost(params, memory, run.spent.get(), run.budget)
    }

    /// systemd's last rule for a token with the three Argon2id fields, with
    /// a PIN or without: its memory cost no more than the machine's memory.
    /// A token without a PIN derives nothing, so where the memory cannot be
    /// read the rule is left out.
    pub(super) fn check_physical_memory(
        &self,
        target: Target<'_>,
        token: &Token,
    ) -> Result<(), TokenFailure> {
        let Ok(Some(fields)) = token.argon2id() else {
            return Ok(());
        };
        let Some(memory) = Memory::read(self.context().files()) else {
            zbm_trace::debug!("the machine's memory could not be read");
            return Ok(());
        };
        check_physical(fields.memcost_bytes, &memory).map_err(|why| {
            let (id, device) = (token.id.as_str(), target.device());
            zbm_trace::warn!(
                "TPM2 token {id} on {device} was not tried: {why}"; %id, %device, %why
            );
            TokenFailure::TooCostly
        })
    }

    /// Says why the TPM2 token `token` is malformed: in the log, and to the
    /// person at the console when it has a PIN.
    pub(super) fn refuse_malformed(&self, target: Target<'_>, token: &Token, why: &Malformed) {
        let (id, device) = (token.id.as_str(), target.device());
        zbm_trace::warn!(
            "TPM2 token {id} on {device} is malformed: {why}"; %id, %device, %why
        );
        if token.flag("tpm2-pin") == Some(true) {
            let malformed = format!("This volume's TPM2 token is malformed: {why}.");
            self.tell(Told::Refused, &malformed);
            self.tell(Told::Refused, "Use another unlock method.");
        }
    }

    /// Says why the TPM2 token `id` of `target` was not tried: in the log,
    /// and to the person at the console.
    pub(super) fn refuse_costly(&self, target: Target<'_>, id: &str, why: &TooCostly) {
        let device = target.device();
        zbm_trace::warn!(
            "TPM2 token {id} on {device} was not tried: {why}"; %id, %device, %why
        );
        self.tell(Told::Refused, &why.told());
        self.tell(Told::Refused, "Use another unlock method.");
    }

    /// Derives `pin` with `salt` and `params`, once the derivation is found
    /// affordable now; it is charged to the run before it starts.
    pub(super) fn argon2id_derive(
        &self,
        pin: &Secret,
        salt: &[u8],
        params: &Argon2idParams,
    ) -> Result<Argon2idSplit, TooCostly> {
        self.argon2id_affordable(params)?;
        let run = &self.argon2id;
        run.spent.set(run.spent.get().saturating_add(params.work()));
        let checking = format!("Checking the PIN (Argon2id, {}) ...", size(params.need()));
        self.tell(Told::Working, &checking);
        if self.luks.settings.keyholder {
            self.raise_oom_score();
        }
        let mut work = WorkMemory::new(0).map_err(TooCostly::Failed)?;
        let Argon2idParams {
            memory_kib,
            iterations,
            lanes,
            ..
        } = *params;
        argon2id_split(pin.expose(), salt, memory_kib, iterations, lanes, &mut work)
            .map_err(TooCostly::Failed)
    }

    /// Makes this process the first the kernel ends when memory runs out,
    /// which needs no privilege.
    fn raise_oom_score(&self) {
        let files = self.context().files();
        if let Err(error) = files.write(Path::new("/proc/self/oom_score_adj"), b"1000\n") {
            zbm_trace::debug!("the OOM score could not be raised: {error}"; %error);
        }
    }

    /// The cached PINs to try on a TPM2 token of `target`, with their
    /// places in the cache. On an Argon2id token, `argon2id`: none but on
    /// the volume's first Argon2id token, and none that its sealed object,
    /// salt and costs refused before.
    pub(super) fn cached_tpm2_pins(
        &mut self,
        target: Target<'_>,
        argon2id: Option<&Ready>,
    ) -> Vec<(usize, Secret)> {
        if let Some(&Ready { sealed, .. }) = argon2id {
            let run = &mut self.argon2id;
            if !run.given_cached.insert(target.device().to_string()) {
                return Vec::new();
            }
            let refused = &run.refused;
            return (self.tpm2_pins.iter().enumerate())
                .filter(|(index, _)| !refused.contains(&(*index, sealed)))
                .map(|(index, pin)| (index, pin.duplicate()))
                .collect();
        }
        (self.tpm2_pins.iter().enumerate())
            .map(|(index, pin)| (index, pin.duplicate()))
            .collect()
    }
}
