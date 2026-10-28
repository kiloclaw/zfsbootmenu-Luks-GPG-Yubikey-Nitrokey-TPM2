//! The hashes and encodings of the key methods, computed in the process so
//! that secrets never reach `sha256sum`, `openssl` or `base64`, where they
//! would pass through pipes and other programs' memory, or arguments that any
//! process can read in `/proc` (see "Secrets" in `crates/DESIGN.md`):
//!
//! - SHA-256 (FIPS 180-4), for the TPM2 authorization value of a PIN and the
//!   `ykchalresp-hash` challenge;
//! - PBKDF2-HMAC-SHA256 (RFC 8018, RFC 2104), for the salted PINs of
//!   systemd's TPM2 tokens;
//! - base64 (RFC 4648), the encoding of TPM2 secrets and token metadata;
//! - lower-case hexadecimal.
//!
//! The algorithms are the `sha2` and `pbkdf2` crates of the `RustCrypto`
//! project, and the `base64` crate (ROADMAP.md SE7): audited, widely used
//! code, in place of our own.
//! Their hash states are wiped when dropped (`sha2`'s `zeroize` feature), and
//! so is every buffer here that held secret material.

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use sha2::Digest as _;
use zbm_platform::secret::Secret;

/// A SHA-256 result, wiped when dropped: the digests here are of secrets (a
/// TPM2 PIN, a YubiKey passphrase), and the start-up program that computes
/// them lives until the boot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The bytes of the digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::ops::Deref for Digest {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for Digest {
    fn drop(&mut self) {
        zbm_sys::wipe(&mut self.0);
    }
}

/// The SHA-256 hash of `data`.
pub fn sha256(data: &[u8]) -> Digest {
    let mut output = sha2::Sha256::digest(data);
    let mut digest = Digest([0; 32]);
    digest.0.copy_from_slice(&output);
    zbm_sys::wipe(&mut output);
    digest
}

/// PBKDF2-HMAC-SHA256 of `password` with `salt` and `rounds` iterations,
/// `length` bytes long.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], rounds: u32, length: usize) -> Secret {
    let mut key = vec![0; length];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password, salt, rounds, &mut key);
    Secret::from_vec(key)
}

/// Standard base64 (RFC 4648, section 4) with padding when encoding; when
/// decoding, padding may be there or not, as the code before took it.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// `data` in base64 with padding, without line breaks, kept as a secret.
pub fn base64_encode(data: &[u8]) -> Secret {
    let length = base64::encoded_len(data.len(), true).unwrap_or(0);
    let mut text = vec![0; length];
    let written = BASE64.encode_slice(data, &mut text).unwrap_or(0);
    text.truncate(written);
    Secret::from_vec(text)
}

/// Decodes base64 text, ignoring line breaks, as `base64 -d` does; `None` if
/// the text is not base64.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let lines: Vec<u8> = text
        .bytes()
        .filter(|byte| !matches!(byte, b'\n' | b'\r'))
        .collect();
    BASE64.decode(lines).ok()
}

/// `data` in lower-case hexadecimal, kept as a secret.
pub fn hex(data: &[u8]) -> Secret {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = Secret::new();
    for byte in data {
        for nibble in [byte >> 4, byte & 0x0f] {
            if let Some(&digit) = DIGITS.get(usize::from(nibble)) {
                text.push(digit);
            }
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbm_platform::exec::{Command, Input, Runner, Stream, SystemRunner};
    use zbm_testkit::{Rng, have_program};

    fn hex_text(data: &[u8]) -> String {
        String::from_utf8(hex(data).expose().to_vec()).unwrap()
    }

    #[test]
    fn sha256_matches_the_standard_vectors() {
        assert_eq!(
            hex_text(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_text(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex_text(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let million = vec![b'a'; 1_000_000];
        assert_eq!(
            hex_text(&sha256(&million)),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn sha512_is_there_for_the_luks2_headers_to_come() {
        // LUKS2 key slots and digests can use SHA-512 (`cryptsetup --hash
        // sha512`): the crate brings it with SHA-256, for the native LUKS2
        // code (ROADMAP.md CD9). FIPS 180-4's "abc" vector.
        let digest = sha2::Sha512::digest(b"abc");
        assert_eq!(
            hex_text(&digest),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn pbkdf2_matches_the_standard_vectors() {
        // RFC 7914, section 11, which covers a password longer than a hash
        // block too.
        let key = pbkdf2_hmac_sha256(b"passwd", b"salt", 1, 64);
        assert_eq!(
            hex_text(key.expose()),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
        );
        let key = pbkdf2_hmac_sha256(b"Password", b"NaCl", 80_000, 64);
        assert_eq!(
            hex_text(key.expose()),
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56\
             a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d"
        );
        // A password longer than a block is hashed first (RFC 2104).
        let long = [b'p'; 100];
        assert_eq!(
            pbkdf2_hmac_sha256(&long, b"salt", 2, 32),
            pbkdf2_hmac_sha256(sha256(&long).as_bytes(), b"salt", 2, 32)
        );
        assert!(pbkdf2_hmac_sha256(b"pin", b"salt", 1, 0).is_empty());
    }

    #[test]
    fn base64_round_trips() {
        for (data, text) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(data).expose(), text.as_bytes());
            assert_eq!(base64_decode(text).as_deref(), Some(data));
        }
        assert_eq!(
            base64_decode("Zm9v\nYmFy\n").as_deref(),
            Some(&b"foobar"[..])
        );
        assert_eq!(base64_decode("Zm9v!"), None);
        assert_eq!(base64_decode("Zg==Zg"), None);
        assert_eq!(base64_decode("Z"), None);
        // Padding may be left out, as the code before took it.
        assert_eq!(base64_decode("Zg").as_deref(), Some(&b"f"[..]));
        assert_eq!(base64_decode("Zm8").as_deref(), Some(&b"fo"[..]));
        // Stray bits in the last character are refused: no encoder writes
        // them ("Zg==" is "f").
        assert_eq!(base64_decode("Zh=="), None);
    }

    /// Runs a program with `stdin` and returns its standard output.
    fn run(program: &str, args: &[&str], stdin: &[u8]) -> Vec<u8> {
        let command = Command::new(program)
            .args(args)
            .stdin(Input::Bytes(stdin.to_vec()))
            .stderr(Stream::Null);
        SystemRunner.run(&command).unwrap().stdout
    }

    #[test]
    fn results_match_the_programs_the_legacy_code_runs() {
        let mut rng = Rng::from_env(0x5a5a);
        for _ in 0..20 {
            let len = rng.below(200);
            let data: Vec<u8> = (0..len)
                .map(|_| u8::try_from(rng.below(256)).unwrap())
                .collect();
            if have_program("sha256sum") {
                let legacy = run("sha256sum", &[], &data);
                assert_eq!(hex(&sha256(&data)).expose(), &legacy[..64]);
            }
            if have_program("base64") {
                let legacy = run("base64", &[], &data);
                let legacy: Vec<u8> = legacy.into_iter().filter(|byte| *byte != b'\n').collect();
                assert_eq!(base64_encode(&data).expose(), legacy);
                let text = String::from_utf8(legacy).unwrap();
                assert_eq!(base64_decode(&text), Some(data.clone()));
            }
        }
        if !have_program("openssl") {
            return;
        }
        // The salted PIN of the legacy code: `openssl kdf ... PBKDF2`.
        for (pin, salt) in [
            ("1234", &b"0123456789abcdef"[..]),
            ("", b"s"),
            ("pïn é", b"\x00\xff"),
        ] {
            let hexsalt = String::from_utf8(hex(salt).expose().to_vec()).unwrap();
            let pass = format!("pass:{pin}");
            let salt_option = format!("hexsalt:{hexsalt}");
            let args = [
                "kdf",
                "-keylen",
                "32",
                "-kdfopt",
                "digest:SHA256",
                "-kdfopt",
                &pass,
                "-kdfopt",
                &salt_option,
                "-kdfopt",
                "iter:10000",
                "-binary",
                "PBKDF2",
            ];
            let legacy = run("openssl", &args, b"");
            if legacy.is_empty() {
                // An empty password cannot be given to openssl this way.
                continue;
            }
            let key = pbkdf2_hmac_sha256(pin.as_bytes(), salt, 10_000, 32);
            assert_eq!(key.expose(), legacy, "{pin:?}");
        }
    }
}
