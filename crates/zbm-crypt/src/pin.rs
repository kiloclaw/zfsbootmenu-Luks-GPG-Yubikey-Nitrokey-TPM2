//! The PIN of the locked menu (ROADMAP.md SE9, ADR 0010): its Argon2id hash
//! in the PHC string format (`$argon2id$v=19$m=65536,t=3,p=1$SALT$HASH`),
//! which the image holds, and the check of a PIN against it.
//!
//! The image lies on the EFI system partition, where anyone can read the
//! hash and try PINs against it offline: hence a PIN of
//! [`MIN_LENGTH`] characters at least, and parameters above OWASP's
//! minimum (19 MiB, 2 passes). They are part of the hash, so a check reads
//! them from it, and they may rise later.

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher as _, PasswordVerifier as _};
use argon2::{Algorithm, Argon2, Params, Version};
use zbm_platform::Secret;

/// The fewest characters a PIN has.
pub const MIN_LENGTH: usize = 8;

/// The memory of a hash, in KiB: 64 MiB.
pub const MEMORY: u32 = 64 * 1024;

/// The passes over it.
pub const PASSES: u32 = 3;

/// The bytes of a salt.
pub const SALT: usize = 16;

/// Why a PIN could not be hashed.
#[derive(Debug, thiserror::Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum PinError {
    /// It is shorter than [`MIN_LENGTH`] characters.
    #[error("the PIN has {0} characters; it needs {MIN_LENGTH} at least")]
    TooShort(usize),
    /// Argon2 refused it, or its salt.
    #[error("unable to hash the PIN: {0}")]
    Hash(String),
}

/// The Argon2 of the hashes this writes.
fn argon2() -> Result<Argon2<'static>, PinError> {
    let params =
        Params::new(MEMORY, PASSES, 1, None).map_err(|error| PinError::Hash(error.to_string()))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// The hash of `pin` with `salt`, as the image holds it.
///
/// # Errors
///
/// [`PinError::TooShort`] for a PIN of fewer than [`MIN_LENGTH`] characters;
/// [`PinError::Hash`] when Argon2 refuses it.
pub fn hash(pin: &Secret, salt: &[u8; SALT]) -> Result<String, PinError> {
    let characters = String::from_utf8_lossy(pin.expose()).chars().count();
    if characters < MIN_LENGTH {
        return Err(PinError::TooShort(characters));
    }
    let hashed = argon2()?
        .hash_password_with_salt(pin.expose(), salt)
        .map_err(|error| PinError::Hash(error.to_string()))?;
    Ok(hashed.to_string())
}

/// Whether `pin` is the PIN whose hash `phc` is: false too when `phc` is
/// not an Argon2id hash in the PHC format. The parameters are the hash's;
/// the comparison takes the same time wherever the bytes differ.
pub fn is_pin_of(phc: &str, pin: &Secret) -> bool {
    let Ok(parsed) = PasswordHash::new(phc) else {
        return false;
    };
    if parsed.algorithm.as_str() != Algorithm::Argon2id.as_str() {
        return false;
    }
    Argon2::default()
        .verify_password(pin.expose(), &parsed)
        .is_ok()
}

#[cfg(test)]
mod tests;
