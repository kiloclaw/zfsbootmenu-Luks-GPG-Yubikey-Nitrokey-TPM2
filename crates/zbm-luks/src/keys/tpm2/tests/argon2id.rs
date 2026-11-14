//! Tokens whose PIN systemd 262 hardened with Argon2id, and the Argon2id
//! fields systemd's parser refuses in any token.

use super::*;

/// Whether `result` ended the token as one the image cannot open.
fn is_unsupported(result: &Result<OpenMapping, LuksError>) -> bool {
    matches!(
        result,
        Err(LuksError::Token {
            failure: TokenFailure::Unsupported,
            ..
        })
    )
}

/// Whether `fake` ran a `tpm2_*` command other than the lockout probe.
fn ran_tpm2_tools(fake: &FakeSystem) -> bool {
    fake.runner
        .lines()
        .iter()
        .any(|line| line.starts_with("tpm2_") && !line.starts_with("tpm2_getcap"))
}

#[test]
fn a_malformed_argon2id_pin_token_is_named_before_any_pin() {
    let json = fixture("token-tpm2-argon2id.json")
        .replace(r#""tpm2_argon2id_lanes":1"#, r#""tpm2_argon2id_lanes":0"#);
    let malformed = Token::from_export("0", &json).unwrap();
    let fake = tpm_machine(None, b"s");
    let (result, asked) = try_without_plugin(&fake, &malformed);
    assert!(result.is_err() && !asked);
    let kmsg = fake.kmsg();
    assert!(kmsg.contains("TPM2 token 0 on /dev/sda is malformed: tpm2_argon2id_lanes is 0"));
    assert!(!kmsg.contains("needs the systemd-tpm2 cryptsetup plugin"));
}

#[test]
fn a_no_pin_token_with_pin_only_defects_still_unseals() {
    // One lane too many and a 4-byte salt: a derivation could not use them,
    // but a token without a PIN derives nothing, and systemd ignores them.
    let pinless = token_with(
        "token-tpm2.json",
        r#""tpm2_salt":"AAAAAA==","tpm2_argon2id_memcost":1024,
           "tpm2_argon2id_iterations":1,"tpm2_argon2id_lanes":16777216"#,
    );
    let fake = tpm_machine(None, b"secret");
    fake.files.device("/dev/tpmrm0");
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let result = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        session.try_tpm2_token(target, &pinless)
    });
    assert!(result.is_ok());
    fake.assert_no_leak(b"secret");
}

#[test]
fn a_no_pin_token_systemd_s_parser_refuses_is_refused() {
    let costs = r#""tpm2_argon2id_memcost":1048576,"tpm2_argon2id_iterations":2"#;
    let cases = [
        (
            format!(r#""tpm2_salt":"AAAAAA==",{costs},"tpm2_argon2id_lanes":0"#),
            "malformed: tpm2_argon2id_lanes is 0",
        ),
        (
            format!(r#""tpm2_salt":"AAAAAA==",{costs}"#),
            "malformed: the Argon2id fields are incomplete",
        ),
        (
            format!(r#"{costs},"tpm2_argon2id_lanes":1"#),
            "malformed: tpm2_salt is missing",
        ),
    ];
    for (fields, why) in cases {
        let pinless = token_with("token-tpm2.json", &fields);
        let fake = tpm_machine(None, b"secret");
        fake.files.device("/dev/tpmrm0");
        let (result, _) = try_without_plugin(&fake, &pinless);
        assert!(is_unsupported(&result), "{fields}");
        assert!(fake.kmsg().contains(why), "{why}");
        assert!(!ran_tpm2_tools(&fake), "{fields}");

        // The plugin is not given it either.
        let (fake, settings) = plugin_machine();
        let args = [];
        let target = crate::testing::target("v", "/dev/sda", &args);
        let result = with_session(&fake, &settings, |session| {
            session.try_tpm2_token(target, &pinless)
        });
        assert!(is_unsupported(&result), "{fields}");
        assert!(fake.runner.lines().is_empty(), "{fields}");
    }
}

#[test]
fn a_malformed_pin_token_is_refused_before_any_pin_even_with_the_plugin() {
    let json = fixture("token-tpm2-argon2id.json")
        .replace(r#""tpm2_argon2id_lanes":1"#, r#""tpm2_argon2id_lanes":0"#);
    let malformed = Token::from_export("0", &json).unwrap();
    let (fake, settings) = plugin_machine();
    fake.console.secret(b"1234");
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let result = with_session(&fake, &settings, |session| {
        session.try_tpm2_token(target, &malformed)
    });
    assert!(is_unsupported(&result));
    assert!(!fake.console.remaining().is_empty());
    assert!(
        fake.kmsg()
            .contains("TPM2 token 0 on /dev/sda is malformed: tpm2_argon2id_lanes is 0")
    );
    assert!(fake.runner.lines().is_empty());
}
