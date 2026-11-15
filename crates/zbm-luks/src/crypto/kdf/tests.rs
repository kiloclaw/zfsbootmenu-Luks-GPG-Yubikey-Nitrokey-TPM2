//! Known answers for the derivations of systemd 262's Argon2id TPM2 PINs.
//!
//! The primitives are checked against published vectors: RFC 9106's
//! Argon2id vector (section 5.3) and RFC 5869's test case 1 (appendix A.1).
//!
//! Vectors V1 to V3 were computed with OpenSSL 4.0.2's `ARGON2ID` KDF, the
//! one systemd calls, and its HKDF, through python-cryptography 50.0.1; each
//! HKDF output was checked again with `openssl kdf ... HKDF` (OpenSSL
//! 3.0.13), which agreed on all three. The script, with its lines wrapped:
//!
//! ```text
//! def vector(name, pin, salt, memcost_bytes, iterations, lanes, unsealed):
//!     kib = memcost_bytes // 1024
//!     derived = Argon2id(salt=salt, length=64, iterations=iterations,
//!         lanes=lanes, memory_cost=kib).derive(pin)
//!     key1, key2 = derived[:32], derived[32:]
//!     b64pin = base64.b64encode(key2)
//!     auth = hashlib.sha256(b64pin).digest().rstrip(b"\0")
//!     volume = HKDF(algorithm=hashes.SHA256(), length=32, salt=unsealed,
//!         info=b"systemd-tpm2-argon2id-lock").derive(key1)
//!
//! vector("V1", b"1234", bytes(range(32)), 64 * 1024 * 1024, 8, 4,
//!        bytes(range(32, 64)))
//! vector("V2", b"5678",
//!        base64.b64decode("4ro80Y0RyjD3nRNKhbB1SkEvfNRRCdfRWjYui50mOU8="),
//!        8 * 1024 * 1024 + 1000, 2, 24, b"\xff" * 32)
//! vector("V3", "pässwörd 1".encode(), hashlib.sha256(b"salt").digest(),
//!        1024 * 1024 + 1023, 3, 1, hashlib.sha256(b"unsealed").digest())
//! ```
//!
//! Its output, with the derived 64 bytes split into `key1` and the rest:
//!
//! ```text
//! V1 memcost=67108864 (=65536 KiB) iterations=8 lanes=4
//! key1=9a54064c868853a49258bce01cc99c1fcb3016d2da59a384ee99539d484f821f
//! key2=4513e122a1923c17b05c3cc077d8e420d05192f0cbab36ed84634d3dd003209d
//! tpm_pin_string=RRPhIqGSPBewXDzAd9jkINBRkvDLqzbthGNNPdADIJ0=
//! auth=ea5a217f7c2693c9297c87f882a1acc4bb5d79683dbfcbf110f5efb42e0a7bed
//! volume=4f3bf0a7af51a7fe6e8b4d2482666ca7350ceeae06c32198f37b098f8d60078f
//! luks_passphrase=Tzvwp69Rp/5ui00kgmZspzUM7q4GwyGY83sJj41gB48=
//!
//! V2 memcost=8389608 (=8192 KiB) iterations=2 lanes=24
//! key1=92719c1e1812b1fb77d38fd2b87264a12104e3d606cdf2acc25f2bfb7ee47c5f
//! key2=5012336ca5dc09b28a79401100e0a3d3f7e49a7021a534b83fc17fe6031cb86e
//! tpm_pin_string=UBIzbKXcCbKKeUARAOCj0/fkmnAhpTS4P8F/5gMcuG4=
//! auth=7c1f0aadb989351256b7395a512c59dc180af1a97e830d932d1215148ca11a49
//! volume=1ec2c0cf9b37c97060d3c7fed205bd74aa28bbbf52cb12ebea6f6f038cb64eac
//! luks_passphrase=HsLAz5s3yXBg08f+0gW9dKoou79SyxLr6m9vA4y2Tqw=
//!
//! V3 memcost=1049599 (=1024 KiB) iterations=3 lanes=1
//! key1=d0c87623e99e16a59609eca33d75729b6b6592478b2de56517349dd5a0c671bc
//! key2=695744fe9b7ba32c8444c19f7e2f99e7f1a6a0fb598a087973d35e60d134dc78
//! tpm_pin_string=aVdE/pt7oyyERMGffi+Z5/GmoPtZigh5c9NeYNE03Hg=
//! auth=0c6703c802369b13f15c8de42260955939291cc380e585a6f7cfbc2bba64b079
//! volume=cded1583f9363f78c0eb4ffb886f5899370c1cd76f6b36fdb1ee1e243e9bb7d0
//! luks_passphrase=ze0Vg/k2P3jA60/7iG9YmTcMHNdvazb9se4eJD6bt9A=
//! ```
//!
//! These test the primitives against OpenSSL. How systemd composes them
//! (which half is which, the base64, the authorization value, HKDF's
//! roles) is read from systemd 262's sources; only the VM tests check it
//! end to end.

use super::*;
use crate::crypto::{base64_decode, hex, sha256};
use crate::keys::tpm2::auth_value;

/// The HKDF `info` of systemd 262's Argon2id tokens.
const INFO: &[u8] = b"systemd-tpm2-argon2id-lock";

/// One of the vectors V1 to V3.
struct Vector {
    pin: &'static [u8],
    salt: Vec<u8>,
    memcost_bytes: u64,
    iterations: u32,
    lanes: u32,
    unsealed: Vec<u8>,
    key1: &'static str,
    tpm_pin: &'static str,
    auth: &'static str,
    volume_key: &'static str,
    passphrase: &'static str,
}

/// systemd's defaults: 64 MiB, 8 passes, 4 lanes.
fn v1() -> Vector {
    Vector {
        pin: b"1234",
        salt: (0..32).collect(),
        memcost_bytes: 64 * 1024 * 1024,
        iterations: 8,
        lanes: 4,
        unsealed: (32..64).collect(),
        key1: "9a54064c868853a49258bce01cc99c1fcb3016d2da59a384ee99539d484f821f",
        tpm_pin: "RRPhIqGSPBewXDzAd9jkINBRkvDLqzbthGNNPdADIJ0=",
        auth: "ea5a217f7c2693c9297c87f882a1acc4bb5d79683dbfcbf110f5efb42e0a7bed",
        volume_key: "4f3bf0a7af51a7fe6e8b4d2482666ca7350ceeae06c32198f37b098f8d60078f",
        passphrase: "Tzvwp69Rp/5ui00kgmZspzUM7q4GwyGY83sJj41gB48=",
    }
}

/// zfs2's real salt, 24 lanes, and a cost that is a multiple neither of
/// 1024 bytes nor of 4 · 24 KiB.
fn v2() -> Vector {
    Vector {
        pin: b"5678",
        salt: base64_decode("4ro80Y0RyjD3nRNKhbB1SkEvfNRRCdfRWjYui50mOU8=").unwrap(),
        memcost_bytes: 8 * 1024 * 1024 + 1000,
        iterations: 2,
        lanes: 24,
        unsealed: vec![0xff; 32],
        key1: "92719c1e1812b1fb77d38fd2b87264a12104e3d606cdf2acc25f2bfb7ee47c5f",
        tpm_pin: "UBIzbKXcCbKKeUARAOCj0/fkmnAhpTS4P8F/5gMcuG4=",
        auth: "7c1f0aadb989351256b7395a512c59dc180af1a97e830d932d1215148ca11a49",
        volume_key: "1ec2c0cf9b37c97060d3c7fed205bd74aa28bbbf52cb12ebea6f6f038cb64eac",
        passphrase: "HsLAz5s3yXBg08f+0gW9dKoou79SyxLr6m9vA4y2Tqw=",
    }
}

/// One lane, 1 MiB and a UTF-8 PIN.
fn v3() -> Vector {
    Vector {
        pin: "pässwörd 1".as_bytes(),
        salt: sha256(b"salt").to_vec(),
        memcost_bytes: 1024 * 1024 + 1023,
        iterations: 3,
        lanes: 1,
        unsealed: sha256(b"unsealed").to_vec(),
        key1: "d0c87623e99e16a59609eca33d75729b6b6592478b2de56517349dd5a0c671bc",
        tpm_pin: "aVdE/pt7oyyERMGffi+Z5/GmoPtZigh5c9NeYNE03Hg=",
        auth: "0c6703c802369b13f15c8de42260955939291cc380e585a6f7cfbc2bba64b079",
        volume_key: "cded1583f9363f78c0eb4ffb886f5899370c1cd76f6b36fdb1ee1e243e9bb7d0",
        passphrase: "ze0Vg/k2P3jA60/7iG9YmTcMHNdvazb9se4eJD6bt9A=",
    }
}

/// `data` in hexadecimal, as a string.
fn hex_text(data: &[u8]) -> String {
    String::from_utf8(hex(data).expose().to_vec()).unwrap()
}

/// The bytes of the hexadecimal `text`.
fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

/// Derives `vector` as systemd does, and checks the split and the
/// authorization value the TPM is given.
fn check_split(vector: &Vector) {
    let memory_kib = u32::try_from(vector.memcost_bytes / 1024).unwrap();
    let mut work = WorkMemory::new(0).unwrap();
    let split = argon2id_split(
        vector.pin,
        &vector.salt,
        memory_kib,
        vector.iterations,
        vector.lanes,
        &mut work,
    )
    .unwrap();
    assert_eq!(hex_text(split.key1.expose()), vector.key1);
    assert_eq!(split.tpm_pin.expose(), vector.tpm_pin.as_bytes());
    assert_eq!(
        auth_value(split.tpm_pin.expose(), None).expose(),
        vector.auth.as_bytes()
    );
}

#[test]
fn argon2id_split_matches_openssl_for_systemd_s_defaults() {
    check_split(&v1());
}

#[test]
fn argon2id_split_matches_openssl_for_24_lanes_and_an_odd_memory_cost() {
    check_split(&v2());
}

#[test]
fn argon2id_split_matches_openssl_for_one_lane_and_a_utf8_pin() {
    check_split(&v3());
}

#[test]
fn the_argon2id_passphrase_matches_openssl_s_hkdf() {
    for vector in [v1(), v2(), v3()] {
        let volume_key = hkdf_sha256(&unhex(vector.key1), &vector.unsealed, INFO);
        assert_eq!(hex_text(volume_key.expose()), vector.volume_key);
        let passphrase = base64_encode(volume_key.expose());
        assert_eq!(passphrase.expose(), vector.passphrase.as_bytes());
    }
}

#[test]
fn hkdf_sha256_matches_rfc_5869_test_case_1() {
    // The output for 32 bytes is the start of the test case's 42.
    let okm = hkdf_sha256(
        &[0x0b; 22],
        &(0x00..=0x0c).collect::<Vec<u8>>(),
        &[0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9],
    );
    assert_eq!(
        hex_text(okm.expose()),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
    );
}

#[test]
fn argon2_matches_rfc_9106_s_argon2id_vector() {
    // Section 5.3: 32 KiB, 3 passes, 4 lanes, with a secret and associated
    // data, in a caller's work memory.
    let params = argon2::ParamsBuilder::new()
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .data(argon2::AssociatedData::new(&[0x04; 12]).unwrap())
        .output_len(32)
        .build()
        .unwrap();
    let mut work = WorkMemory::new(0).unwrap();
    let memory = work.lend(params.block_count()).unwrap();
    let mut tag = [0; 32];
    Argon2::new_with_secret(&[0x03; 8], Algorithm::Argon2id, Version::V0x13, params)
        .unwrap()
        .hash_password_into_with_memory(&[0x01; 32], &[0x02; 16], &mut tag, memory)
        .unwrap();
    assert_eq!(
        hex_text(&tag),
        "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659"
    );
}

/// Whether every word of every block of `work` is zero.
fn is_zero(work: &WorkMemory) -> bool {
    work.blocks
        .iter()
        .all(|block| block.as_ref().iter().all(|&word| word == 0))
}

/// Works V3's derivation in the first 1024 blocks of `work`, without the
/// wipe that ends `argon2id_split`.
fn fill(work: &mut WorkMemory) {
    let vector = v3();
    let params = Params::new(1024, 3, 1, Some(64)).unwrap();
    let memory = work.lend(1024).unwrap();
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(vector.pin, &vector.salt, &mut [0; 64], memory)
        .unwrap();
}

#[test]
fn work_memory_is_zero_after_a_derivation() {
    let vector = v3();
    let mut work = WorkMemory::new(1024).unwrap();
    assert_eq!(work.blocks.len(), 1024);
    assert!(is_zero(&work));
    // A derivation leaves state in the blocks until they are wiped.
    fill(&mut work);
    assert!(!is_zero(&work));
    work.wipe_now();
    assert_eq!(work.blocks.len(), 1024);
    assert!(is_zero(&work));
    // `argon2id_split` wipes them when it ends, and keeps them for the next
    // derivation, which gives the same.
    let first = argon2id_split(vector.pin, &vector.salt, 1024, 3, 1, &mut work).unwrap();
    assert!(is_zero(&work));
    let again = argon2id_split(vector.pin, &vector.salt, 1024, 3, 1, &mut work).unwrap();
    assert_eq!(again.key1, first.key1);
    assert_eq!(work.blocks.len(), 1024);
    assert!(is_zero(&work));
    // A derivation that fails wipes them too.
    fill(&mut work);
    let failed = argon2id_split(vector.pin, b"short", 1024, 3, 1, &mut work);
    assert_eq!(
        failed.unwrap_err(),
        KdfError::Argon2(argon2::Error::SaltTooShort)
    );
    assert!(is_zero(&work));
    // Blocks a derivation left are wiped before more room moves them: what
    // the new allocation holds is zero.
    fill(&mut work);
    work.make_room(2048).unwrap();
    assert_eq!(work.blocks.len(), 2048);
    assert!(is_zero(&work));
    assert_eq!(
        format!("{work:?}"),
        "WorkMemory { blocks: 2048, used: false }"
    );
}

#[test]
fn a_work_memory_too_large_to_reserve_is_an_error_not_an_abort() {
    let blocks = usize::MAX / 1024;
    let error = WorkMemory::new(blocks).unwrap_err();
    assert_eq!(error, KdfError::OutOfMemory { blocks });
    assert_eq!(
        error.to_string(),
        format!("{blocks} KiB of work memory could not be reserved")
    );
}

#[test]
fn costs_argon2_refuses_are_errors() {
    let mut work = WorkMemory::new(0).unwrap();
    for (memory_kib, iterations, lanes, refusal) in [
        (7, 1, 1, argon2::Error::MemoryTooLittle),
        (64, 0, 1, argon2::Error::TimeTooSmall),
        (64, 1, 0, argon2::Error::ThreadsTooFew),
        (1024, 1, 0x0100_0000, argon2::Error::ThreadsTooMany),
    ] {
        let error = argon2id_split(b"1234", &[0; 32], memory_kib, iterations, lanes, &mut work)
            .unwrap_err();
        assert_eq!(error, KdfError::Argon2(refusal));
        assert!(
            error
                .to_string()
                .starts_with("Argon2id refused its inputs: ")
        );
    }
    assert!(work.blocks.is_empty());
}
