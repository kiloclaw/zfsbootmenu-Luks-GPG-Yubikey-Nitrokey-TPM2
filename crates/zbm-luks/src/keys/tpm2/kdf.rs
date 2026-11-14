//! How a systemd-tpm2 token's PIN becomes what the TPM checks, as the token
//! records it:
//!
//! - the PIN itself, for tokens of systemd before 255;
//! - PBKDF2 with the token's `tpm2_salt`, for systemd 255 to 261 and systemd
//!   262's `--tpm2-with-pin=direct`;
//! - Argon2id with the token's salt and the costs of its
//!   `tpm2_argon2id_memcost`, `_iterations` and `_lanes`, for systemd 262's
//!   `--tpm2-with-pin=yes`.
//!
//! The fields come from the LUKS header, which a tampered disk or a hostile
//! USB device chooses (input I8 of `docs/dev/threat-model.md`). [`PinKdf::of`]
//! reads them with systemd's own parser ([`Token::argon2id`]), and refuses,
//! for a PIN that is to be derived, what Argon2 could never derive: more
//! lanes than Argon2 has (RFC 9106, section 3.1), less than 8 KiB of memory
//! per lane, 4 TiB of memory or more, and a salt shorter than 8 bytes or
//! missing. Nothing is allocated or derived before that. The salt is
//! decoded as systemd decodes it, with whitespace anywhere skipped.
//! [`check_systemd_rules`] applies systemd's parser alone, to every token.

use zbm_crypt::Token;
use zbm_crypt::token::BadField;

use crate::crypto::base64_decode;

/// The most lanes Argon2 has: 2²⁴ − 1.
const MAX_LANES: u32 = 0x00FF_FFFF;
/// The least memory per lane, in KiB, that Argon2 accepts.
const MIN_KIB_PER_LANE: u64 = 8;
/// The shortest salt Argon2 accepts, in bytes; systemd writes 32.
const MIN_SALT: usize = 8;

/// How a PIN token's PIN is turned into the TPM's authorization value.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum PinKdf {
    /// The PIN as it is typed.
    Plain,
    /// PBKDF2-HMAC-SHA256 with the token's salt. A salt that is not base64
    /// is empty, as it always was here.
    Pbkdf2 {
        /// The decoded `tpm2_salt`.
        salt: Vec<u8>,
    },
    /// Argon2id with the token's salt and costs.
    Argon2id {
        /// The decoded `tpm2_salt`, 8 bytes or more.
        salt: Vec<u8>,
        /// The costs, valid for a derivation.
        params: Argon2idParams,
    },
}

/// The costs of an Argon2id PIN, checked so that Argon2 can derive with
/// them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Argon2idParams {
    /// The memory, in KiB: the token's bytes divided by 1024, rounded down,
    /// as systemd passes them to OpenSSL.
    pub(super) memory_kib: u32,
    /// The passes over the memory.
    pub(super) iterations: u32,
    /// The lanes, from 1 to 2²⁴ − 1, with at least 8 KiB each.
    pub(super) lanes: u32,
}

/// Why a PIN token's Argon2id fields cannot be used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Malformed {
    /// systemd's own parser refuses them.
    #[error(transparent)]
    Field(#[from] BadField),
    /// There is no salt.
    #[error("tpm2_salt is missing")]
    NoSalt,
    /// The salt is not base64.
    #[error("tpm2_salt is not base64")]
    SaltNotBase64,
    /// The salt is shorter than Argon2 accepts.
    #[error("tpm2_salt has {0} bytes, fewer than 8")]
    ShortSalt(usize),
    /// The memory is 2³² KiB or more.
    #[error("tpm2_argon2id_memcost is 4 TiB or more")]
    Memory,
    /// There are more lanes than Argon2 has.
    #[error("tpm2_argon2id_lanes is more than 16777215")]
    Lanes,
    /// The memory is less than 8 KiB per lane.
    #[error("tpm2_argon2id_memcost is less than 8 KiB per lane")]
    MemoryPerLane,
}

impl PinKdf {
    /// How the PIN of the PIN token `token` is derived.
    ///
    /// # Errors
    ///
    /// [`Malformed`] when the Argon2id fields are not systemd's, or are, but
    /// Argon2 could not derive with them.
    pub(super) fn of(token: &Token) -> Result<PinKdf, Malformed> {
        let salt = token.text("tpm2_salt");
        let Some(fields) = token.argon2id()? else {
            return Ok(salt.map_or(PinKdf::Plain, |salt| PinKdf::Pbkdf2 {
                salt: base64_decode(salt).unwrap_or_default(),
            }));
        };
        let salt =
            systemd_base64(salt.ok_or(Malformed::NoSalt)?).ok_or(Malformed::SaltNotBase64)?;
        if salt.len() < MIN_SALT {
            return Err(Malformed::ShortSalt(salt.len()));
        }
        let memory_kib =
            u32::try_from(fields.memcost_bytes / 1024).map_err(|_| Malformed::Memory)?;
        if fields.lanes > MAX_LANES {
            return Err(Malformed::Lanes);
        }
        if u64::from(memory_kib) < MIN_KIB_PER_LANE * u64::from(fields.lanes) {
            return Err(Malformed::MemoryPerLane);
        }
        let params = Argon2idParams {
            memory_kib,
            iterations: fields.iterations,
            lanes: fields.lanes,
        };
        Ok(PinKdf::Argon2id { salt, params })
    }
}

/// Whether `token`'s Argon2id fields pass the rules systemd's parser
/// applies to every token, with a PIN or without: those of
/// [`Token::argon2id`], and a salt when the three fields are there.
///
/// # Errors
///
/// [`Malformed`] for the first rule the fields break.
pub(super) fn check_systemd_rules(token: &Token) -> Result<(), Malformed> {
    if token.argon2id()?.is_some() && token.text("tpm2_salt").is_none() {
        return Err(Malformed::NoSalt);
    }
    Ok(())
}

/// Decodes base64 text as systemd does, skipping the spaces, tabs and line
/// breaks anywhere in it.
fn systemd_base64(text: &str) -> Option<Vec<u8>> {
    let text: String = text.chars().filter(|c| !matches!(c, ' ' | '\t')).collect();
    base64_decode(&text)
}

#[cfg(test)]
mod tests;
