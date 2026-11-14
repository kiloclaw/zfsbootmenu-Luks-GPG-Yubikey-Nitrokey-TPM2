//! The fixture `token-tpm2-argon2id.json` is the PIN token of
//! `token-tpm2-pin.json` with the salt and costs of known-answer vector V3
//! of the SB21 plan (salt SHA-256 of `salt`, 1049599 bytes, 3 passes, one
//! lane); `token-tpm2-argon2id-f45.json` is the token 0 that systemd 262.1
//! enrolled with `--tpm2-with-pin=yes` on a Fedora 45 host.

use super::*;
use crate::crypto::sha256;
use zbm_testkit::repo_root;

/// The token of the fixture `name`.
fn fixture(name: &str) -> Token {
    let path = repo_root()
        .join("crates/zbm-luks/tests/fixtures")
        .join(name);
    Token::from_export("0", &std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A PIN token with `fields` added.
fn token(fields: &str) -> Token {
    let json = format!(r#"{{"type":"systemd-tpm2","tpm2-pin":true{fields}}}"#);
    Token::from_export("0", &json).unwrap()
}

/// A PIN token with a 32-byte salt and the Argon2id costs given.
fn costs(memcost: u64, iterations: u32, lanes: u32) -> Token {
    token(&format!(
        r#","tpm2_salt":"Y0ea1poJCyWCd+yPum+ZQZov+ySJgVEGV8lEzNEUjpc=",
           "tpm2_argon2id_memcost":{memcost},"tpm2_argon2id_iterations":{iterations},
           "tpm2_argon2id_lanes":{lanes}"#
    ))
}

#[test]
fn three_argon2id_fields_select_argon2id_with_kib_rounded_down() {
    let kdf = PinKdf::of(&fixture("token-tpm2-argon2id.json")).unwrap();
    assert_eq!(
        kdf,
        PinKdf::Argon2id {
            salt: sha256(b"salt").to_vec(),
            params: Argon2idParams {
                memory_kib: 1024,
                iterations: 3,
                lanes: 1,
            },
        }
    );
    // 1023 bytes over a whole KiB are left out, as systemd leaves them.
    assert!(matches!(
        PinKdf::of(&costs(1024 * 1024 + 1023, 3, 1)),
        Ok(PinKdf::Argon2id { params, .. }) if params.memory_kib == 1024
    ));
}

#[test]
fn a_salt_alone_keeps_pbkdf2_and_no_salt_keeps_the_pin() {
    let salted = fixture("token-tpm2-pin.json");
    let salt = base64_decode(salted.text("tpm2_salt").unwrap()).unwrap();
    assert_eq!(PinKdf::of(&salted), Ok(PinKdf::Pbkdf2 { salt }));
    assert_eq!(PinKdf::of(&token("")), Ok(PinKdf::Plain));
    assert_eq!(PinKdf::of(&token(r#","tpm2_salt":"""#)), Ok(PinKdf::Plain));
    // A salt that is not base64 is empty, as it always was here.
    assert_eq!(
        PinKdf::of(&token(r#","tpm2_salt":"!""#)),
        Ok(PinKdf::Pbkdf2 { salt: Vec::new() })
    );
}

#[test]
fn pin_tokens_refuse_what_argon2_cannot_derive() {
    let four_tib = 1_u64 << 42;
    assert_eq!(PinKdf::of(&costs(four_tib, 1, 1)), Err(Malformed::Memory));
    assert!(PinKdf::of(&costs(four_tib - 1, 1, 1)).is_ok());
    assert_eq!(
        PinKdf::of(&costs(1 << 40, 1, 1 << 24)),
        Err(Malformed::Lanes)
    );
    assert!(PinKdf::of(&costs(1 << 40, 1, (1 << 24) - 1)).is_ok());
    assert_eq!(
        PinKdf::of(&costs(8 * 1024 * 4 - 1, 1, 4)),
        Err(Malformed::MemoryPerLane)
    );
    assert!(PinKdf::of(&costs(8 * 1024 * 4, 1, 4)).is_ok());
    // systemd's own parser comes first, and is named.
    let zero = PinKdf::of(&costs(1024, 1, 0)).unwrap_err();
    assert_eq!(
        zero,
        Malformed::Field(BadField::Zero("tpm2_argon2id_lanes"))
    );
    assert_eq!(zero.to_string(), "tpm2_argon2id_lanes is 0");
    assert_eq!(
        Malformed::Lanes.to_string(),
        "tpm2_argon2id_lanes is more than 16777215"
    );
}

#[test]
fn an_argon2id_pin_token_without_a_long_enough_salt_is_malformed() {
    let fields = r#""tpm2_argon2id_memcost":1048576,"tpm2_argon2id_iterations":2,
                    "tpm2_argon2id_lanes":1"#;
    assert_eq!(
        PinKdf::of(&token(&format!(",{fields}"))),
        Err(Malformed::NoSalt)
    );
    assert_eq!(
        PinKdf::of(&token(&format!(r#","tpm2_salt":"!",{fields}"#))),
        Err(Malformed::SaltNotBase64)
    );
    let seven = PinKdf::of(&token(&format!(r#","tpm2_salt":"MTIzNDU2Nw==",{fields}"#)));
    assert_eq!(seven, Err(Malformed::ShortSalt(7)));
    assert_eq!(
        seven.unwrap_err().to_string(),
        "tpm2_salt has 7 bytes, fewer than 8"
    );
    assert!(PinKdf::of(&token(&format!(r#","tpm2_salt":"MTIzNDU2Nzg=",{fields}"#))).is_ok());
}

#[test]
fn the_fedora_45_token_reads_as_3256447_kib_2_passes_24_lanes() {
    let token = fixture("token-tpm2-argon2id-f45.json");
    let Ok(PinKdf::Argon2id { salt, params }) = PinKdf::of(&token) else {
        panic!("the token is not Argon2id");
    };
    assert_eq!(salt.len(), 32);
    assert_eq!(
        params,
        Argon2idParams {
            memory_kib: 3_256_447,
            iterations: 2,
            lanes: 24,
        }
    );
}

#[test]
fn an_argon2id_salt_is_read_with_systemd_s_whitespace_skipped() {
    let fields = r#""tpm2_argon2id_memcost":1048576,"tpm2_argon2id_iterations":2,
                    "tpm2_argon2id_lanes":1"#;
    let spaced = token(&format!(r#","tpm2_salt":" MTIz NDU2\tNzg=\r\n",{fields}"#));
    assert!(matches!(
        PinKdf::of(&spaced),
        Ok(PinKdf::Argon2id { salt, .. }) if salt == b"12345678"
    ));
    // A PBKDF2 salt keeps the old reading: not base64, so empty.
    assert_eq!(
        PinKdf::of(&token(r#","tpm2_salt":"MTIz NDU2Nzg=""#)),
        Ok(PinKdf::Pbkdf2 { salt: Vec::new() })
    );
}

#[test]
fn systemd_s_rules_alone_hold_for_every_token() {
    let costs = |lanes: &str| {
        format!(
            r#","tpm2_argon2id_memcost":1024,"tpm2_argon2id_iterations":1,
               "tpm2_argon2id_lanes":{lanes}"#
        )
    };
    assert_eq!(check_systemd_rules(&token("")), Ok(()));
    assert_eq!(
        check_systemd_rules(&token(&costs("0"))),
        Err(Malformed::Field(BadField::Zero("tpm2_argon2id_lanes")))
    );
    assert_eq!(
        check_systemd_rules(&token(r#","tpm2_argon2id_lanes":1"#)),
        Err(Malformed::Field(BadField::Incomplete))
    );
    assert_eq!(
        check_systemd_rules(&token(&costs("1"))),
        Err(Malformed::NoSalt)
    );
    let empty_salt = format!(r#","tpm2_salt":""{}"#, costs("1"));
    assert_eq!(
        check_systemd_rules(&token(&empty_salt)),
        Err(Malformed::NoSalt)
    );
    // What only a derivation could not use is left to the PIN's own rules.
    let short = format!(r#","tpm2_salt":"AAAAAA=="{}"#, costs("16777216"));
    assert_eq!(check_systemd_rules(&token(&short)), Ok(()));
    assert!(PinKdf::of(&token(&short)).is_err());
}
