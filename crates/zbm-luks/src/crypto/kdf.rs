//! The key derivations of systemd 262's TPM2 PINs (`--tpm2-with-pin=yes`):
//!
//! - Argon2id (RFC 9106), which turns the PIN into 64 bytes: the first 32
//!   are kept, and the base64 of the last 32 is the PIN that the TPM checks,
//!   as the authorization value of a PIN without a salt;
//! - HKDF-SHA256 (RFC 5869), which turns those first 32 bytes and the secret
//!   the TPM released into the volume's key.
//!
//! systemd computes both with OpenSSL. Here Argon2id is `RustCrypto`'s
//! `argon2`, on one thread (the lanes are filled one after another, which
//! changes the time, not the result), and HKDF is two HMACs on the `hmac`
//! crate that `pbkdf2` brings.
//!
//! Argon2 works in memory that holds state derived from the PIN, up to
//! gigabytes of it. That memory is the caller's [`WorkMemory`]: reserved
//! without aborting when there is not enough, and wiped as soon as a
//! derivation ends, which `argon2`'s own allocation is not. The 64 bytes and
//! the HKDF key in between are wiped when dropped; what leaves this module is
//! a [`Secret`]. What is not wiped are the copies the crates leave on the
//! stack, until it is reused: `argon2`'s 1 KiB block derived from the PIN at
//! the start of each lane, its compression's temporaries, and HMAC's padded
//! key.

use argon2::{Algorithm, Argon2, Block, Params, Version};
use pbkdf2::hmac::{Hmac, KeyInit as _, Mac as _};
use zbm_platform::secret::Secret;

use super::base64_encode;

/// The length of what Argon2id derives from a PIN, in bytes.
const DERIVED: usize = 64;
/// The length of each half of it, and of an HMAC-SHA256 output.
const HALF: usize = 32;

/// What Argon2id derives from a TPM2 PIN, split as systemd splits it.
#[derive(Debug)]
pub struct Argon2idSplit {
    /// The first 32 bytes: the key that HKDF turns, with the secret the TPM
    /// releases, into the volume's key.
    pub key1: Secret,
    /// The base64 of the last 32 bytes, with padding: 44 characters, which
    /// the TPM checks as the PIN of a token without a salt.
    pub tpm_pin: Secret,
}

/// Why Argon2id derived nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum KdfError {
    /// The work memory could not be reserved.
    #[error("{blocks} KiB of work memory could not be reserved")]
    OutOfMemory {
        /// The memory asked for, in blocks of 1 KiB.
        blocks: usize,
    },
    /// Argon2 refused the costs or the inputs.
    #[error("Argon2id refused its inputs: {0}")]
    Argon2(argon2::Error),
}

/// The memory Argon2 works in, in blocks of 1 KiB, which the caller reserves
/// before a derivation and frees after it. It is wiped as soon as a
/// derivation ends, and so holds zeros but while one works in it.
pub struct WorkMemory {
    /// The blocks, kept from one derivation to the next.
    blocks: Vec<Block>,
    /// Whether a derivation has worked in the blocks since they were wiped.
    used: bool,
}

impl WorkMemory {
    /// `blocks` blocks of 1 KiB, zeroed. Argon2id with `memory_kib` KiB and
    /// `lanes` lanes uses ⌊`memory_kib` / (4 · `lanes`)⌋ · 4 · `lanes` of
    /// them; [`argon2id_split`] adds what is missing.
    ///
    /// # Errors
    ///
    /// [`KdfError::OutOfMemory`] when the allocator refuses them.
    pub fn new(blocks: usize) -> Result<WorkMemory, KdfError> {
        let mut memory = WorkMemory {
            blocks: Vec::new(),
            used: false,
        };
        memory.make_room(blocks)?;
        Ok(memory)
    }

    /// Makes at least `blocks` zeroed blocks ready, reserving the memory first
    /// so that a refusal is an error, not an abort.
    fn make_room(&mut self, blocks: usize) -> Result<(), KdfError> {
        let missing = blocks.saturating_sub(self.blocks.len());
        if missing == 0 {
            return Ok(());
        }
        // A larger allocation moves the blocks and frees the old ones:
        // whatever an earlier derivation left in them is wiped first.
        self.wipe_now();
        self.blocks
            .try_reserve_exact(missing)
            .map_err(|_| KdfError::OutOfMemory { blocks })?;
        self.blocks.resize(blocks, Block::new());
        Ok(())
    }

    /// At least `blocks` blocks for a derivation to work in, to be wiped by
    /// [`WorkMemory::wipe_now`] when it ends.
    fn lend(&mut self, blocks: usize) -> Result<&mut [Block], KdfError> {
        self.make_room(blocks)?;
        self.used = true;
        Ok(&mut self.blocks)
    }

    /// Overwrites every block a derivation worked in with zeros, keeping
    /// them for the next one. Nothing to do when none has since the last
    /// wipe.
    fn wipe_now(&mut self) {
        if self.used {
            zbm_sys::wipe_slice(&mut self.blocks, Block::new());
            self.used = false;
        }
    }
}

impl std::fmt::Debug for WorkMemory {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkMemory")
            .field("blocks", &self.blocks.len())
            .field("used", &self.used)
            .finish()
    }
}

impl Drop for WorkMemory {
    fn drop(&mut self) {
        self.wipe_now();
    }
}

/// A buffer that is wiped when dropped.
struct Wiped<const N: usize>([u8; N]);

impl<const N: usize> Drop for Wiped<N> {
    fn drop(&mut self) {
        zbm_sys::wipe(&mut self.0);
    }
}

/// Argon2id (version 0x13) of `pin` with `salt`, `memory_kib` KiB of memory,
/// `iterations` passes and `lanes` lanes, 64 bytes long, split as systemd
/// 262 splits it: the first 32 bytes, and the base64 of the last 32. The
/// derivation works in `work`, which it enlarges when needed and wipes when
/// it ends, whether it succeeded or not.
///
/// # Errors
///
/// [`KdfError::Argon2`] for costs or inputs that Argon2 refuses: less than
/// 8 KiB of memory per lane, no pass, no lane or more than 2²⁴ − 1, a salt
/// shorter than 8 bytes. [`KdfError::OutOfMemory`] when `work` cannot be
/// enlarged.
pub fn argon2id_split(
    pin: &[u8],
    salt: &[u8],
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
    work: &mut WorkMemory,
) -> Result<Argon2idSplit, KdfError> {
    let params =
        Params::new(memory_kib, iterations, lanes, Some(DERIVED)).map_err(KdfError::Argon2)?;
    let mut derived = Wiped([0; DERIVED]);
    let memory = work.lend(params.block_count())?;
    let hashed = Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(pin, salt, &mut derived.0, memory);
    work.wipe_now();
    hashed.map_err(KdfError::Argon2)?;
    let (key1, tpm_key) = derived.0.split_at_checked(HALF).unwrap_or_default();
    Ok(Argon2idSplit {
        key1: Secret::copy_from(key1),
        tpm_pin: base64_encode(tpm_key),
    })
}

/// HKDF-SHA256 (RFC 5869) of the input key `key` with `salt` and `info`, 32
/// bytes long: the key HMAC-SHA256(`salt`, `key`), then HMAC-SHA256 of
/// `info` and the byte 1 with that key.
pub fn hkdf_sha256(key: &[u8], salt: &[u8], info: &[u8]) -> Secret {
    let output = hmac_sha256(salt, &[key]).and_then(|prk| hmac_sha256(&prk.0, &[info, &[1]]));
    // HMAC takes keys of any length, so there is always an output.
    output.map_or_else(Secret::new, |okm| Secret::copy_from(&okm.0))
}

/// HMAC-SHA256 of `parts`, one after another, with `key`; `None` if HMAC
/// refused the key, which it does for no length.
fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> Option<Wiped<HALF>> {
    let mut mac = Hmac::<sha2::Sha256>::new_from_slice(key).ok()?;
    for part in parts {
        mac.update(part);
    }
    let mut tag = mac.finalize().into_bytes();
    let mut output = Wiped([0; HALF]);
    output.0.copy_from_slice(&tag);
    zbm_sys::wipe(&mut tag);
    Some(output)
}

#[cfg(test)]
mod tests;
