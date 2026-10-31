use pretty_assertions::assert_eq;

use super::*;

/// A salt of the tests.
const SALT_BYTES: [u8; SALT] = *b"zbm-test-salt-16";

#[test]
fn a_pin_hashes_to_a_phc_string_that_checks_it_alone() {
    let pin = Secret::copy_from(b"correct horse");
    let phc = hash(&pin, &SALT_BYTES).unwrap();

    assert!(phc.starts_with("$argon2id$v=19$m=65536,t=3,p=1$"), "{phc}");
    // The same salt gives the same hash: the parameters and the salt are
    // all of it.
    assert_eq!(hash(&pin, &SALT_BYTES).unwrap(), phc);
    assert!(is_pin_of(&phc, &pin));
    assert!(!is_pin_of(&phc, &Secret::copy_from(b"correct horsf")));
    assert!(!is_pin_of(&phc, &Secret::copy_from(b"")));
}

#[test]
fn a_short_pin_is_refused_and_a_broken_or_other_hash_checks_nothing() {
    assert_eq!(
        hash(&Secret::copy_from("ünf7".as_bytes()), &SALT_BYTES),
        Err(PinError::TooShort(4))
    );
    let pin = Secret::copy_from(b"correct horse");
    assert!(!is_pin_of("", &pin));
    assert!(!is_pin_of("not a hash", &pin));
    // An Argon2i hash, of a weaker kind than the image's.
    let phc = hash(&pin, &SALT_BYTES)
        .unwrap()
        .replace("argon2id", "argon2i");
    assert!(!is_pin_of(&phc, &pin));
}
