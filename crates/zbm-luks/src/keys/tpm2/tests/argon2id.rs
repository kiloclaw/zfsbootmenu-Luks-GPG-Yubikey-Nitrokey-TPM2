//! Tokens whose PIN systemd 262 hardened with Argon2id, and the Argon2id
//! fields systemd's parser refuses in any token.
//!
//! The fixture `token-tpm2-argon2id.json` has the salt and costs of the
//! SB21 plan's known-answer vector V3, which OpenSSL computed: the PIN
//! `pässwörd 1`, 1 MiB, 3 passes, one lane. The fake TPM releases V3's
//! unsealed secret for V3's authorization value alone, and V3's passphrase
//! is the one `cryptsetup` must get, so the whole path is checked against
//! values the code under test did not compute.

use super::*;
use crate::crypto::kdf::{WorkMemory, argon2id_split};

mod costs;

/// V3's PIN.
const PIN: &str = "pässwörd 1";
/// V3's authorization value.
const AUTH: &str = "0c6703c802369b13f15c8de42260955939291cc380e585a6f7cfbc2bba64b079";
/// V3's LUKS passphrase.
const PASSPHRASE: &str = "ze0Vg/k2P3jA60/7iG9YmTcMHNdvazb9se4eJD6bt9A=";
/// Enough memory for any test token: 8 GiB, 6 GiB of it available.
const ROOMY: &str = "MemTotal:        8388608 kB\nMemFree:         1048576 kB\n\
                     MemAvailable:    6291456 kB\n";

/// Whether `result` ended the token with `failure`.
fn ended_with(result: &Result<OpenMapping, LuksError>, failure: TokenFailure) -> bool {
    matches!(result, Err(LuksError::Token { failure: f, .. }) if *f == failure)
}

/// Whether `fake` ran a `tpm2_*` command other than the lockout probe.
fn ran_tpm2_tools(fake: &FakeSystem) -> bool {
    fake.runner
        .lines()
        .iter()
        .any(|line| line.starts_with("tpm2_") && !line.starts_with("tpm2_getcap"))
}

/// What `tpm2_unseal` is given on standard input for `auth`.
fn stdin_auth(auth: &[u8]) -> Vec<u8> {
    [b"hex:".as_slice(), auth].concat()
}

/// A machine with every tool and roomy memory, whose TPM releases V3's
/// unsealed secret for V3's authorization value.
fn v3_machine() -> FakeSystem {
    let fake = FakeSystem::new();
    fake.environment.set("PATH", "/bin");
    fake.files.dir("/run");
    fake.files.file("/proc/meminfo", ROOMY);
    for tool in TOOLS.iter().chain(&["tpm2_getcap"]) {
        fake.files.file_with_mode(format!("/bin/{tool}"), "", 0o755);
    }
    let auth = stdin_auth(AUTH.as_bytes());
    fake.runner.on(move |command| {
        if command.program() != "tpm2_unseal" {
            return None;
        }
        let right = matches!(command.get_stdin(), Input::Secret(given) if given.expose() == auth);
        Some(Ok(if right {
            Output::with_secret(Status::exited(0), Secret::copy_from(&sha256(b"unsealed")))
        } else {
            Output::new(Status::exited(1), "")
        }))
    });
    fake
}

/// The settings of a machine whose TPM is a simulator, so always there.
fn simulated() -> Settings {
    let mut settings = Settings::new("/zfsbootmenu");
    settings.tpm2_tcti = "swtpm:port=2321".to_owned();
    settings
}

/// V3's token.
fn v3() -> Token {
    token("token-tpm2-argon2id.json")
}

/// The result of V3's token on `fake` with `settings`, the PINs `pins`
/// typed in turn.
fn try_v3(fake: &FakeSystem, settings: &Settings, pins: &[&str]) -> Result<OpenMapping, LuksError> {
    for pin in pins {
        fake.console.secret(pin.as_bytes());
    }
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    with_session(fake, settings, |session| {
        session.try_tpm2_token(target, &v3())
    })
}

/// What `program` was given on standard input, each time it ran.
fn stdins(fake: &FakeSystem, program: &str) -> Vec<Vec<u8>> {
    fake.runner
        .calls()
        .iter()
        .filter(|call| call.program() == program)
        .filter_map(|call| match call.get_stdin() {
            Input::Secret(given) => Some(given.expose().to_vec()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_argon2id_token_opens_with_the_hkdf_passphrase() {
    let fake = v3_machine();
    assert!(try_v3(&fake, &simulated(), &[PIN]).is_ok());
    assert_eq!(stdins(&fake, "tpm2_unseal"), [stdin_auth(AUTH.as_bytes())]);
    assert_eq!(stdins(&fake, "cryptsetup"), [PASSPHRASE.as_bytes()]);
    // The object is loaded before the PIN is derived, and the session
    // starts after it.
    let lines = fake.runner.lines();
    assert!(lines[0].starts_with("tpm2_getcap"));
    assert!(lines[1].starts_with("tpm2_load"));
    assert!(lines[2].starts_with("tpm2_startauthsession"));
    let output = fake.console.output_text();
    assert!(output.contains("Checking the PIN (Argon2id, 1 MiB) ..."));
    assert!(fake.kmsg().contains(
        "TPM2 token 0 on /dev/sda uses systemd 262's Argon2id PIN (1024 KiB, 3 passes, 1 lanes)"
    ));
}

#[test]
fn a_wrong_pin_on_an_argon2id_token_sends_the_argon2id_authorization_and_is_counted() {
    let fake = v3_machine();
    assert!(try_v3(&fake, &simulated(), &["0000", PIN]).is_ok());
    let mut work = WorkMemory::new(0).unwrap();
    let wrong = argon2id_split(b"0000", &sha256(b"salt"), 1024, 3, 1, &mut work).unwrap();
    let wrong = auth_value(wrong.tpm_pin.expose(), None);
    assert_eq!(
        stdins(&fake, "tpm2_unseal"),
        [stdin_auth(wrong.expose()), stdin_auth(AUTH.as_bytes())]
    );
    let output = fake.console.output_text();
    assert_eq!(output.matches("Checking the PIN (Argon2id").count(), 2);
    assert!(output.contains("The TPM did not release the key"));
    // The lockout is checked again after the refusal.
    let probes = fake.runner.lines();
    let probes = probes.iter().filter(|line| line.starts_with("tpm2_getcap"));
    assert_eq!(probes.count(), 2);
}

#[test]
fn argon2id_tokens_are_unsealed_natively_even_with_the_plugin() {
    let fake = v3_machine();
    fake.files.file(
        "/usr/lib/cryptsetup/libcryptsetup-token-systemd-tpm2.so",
        "",
    );
    assert!(try_v3(&fake, &simulated(), &[PIN]).is_ok());
    let lines = fake.runner.lines();
    assert!(!lines.iter().any(|line| line.contains("--token-id")));
    assert_eq!(stdins(&fake, "cryptsetup"), [PASSPHRASE.as_bytes()]);
}

#[test]
fn pbkdf2_tokens_still_go_to_the_plugin_first() {
    let (fake, settings) = plugin_machine();
    fake.files.file("/proc/meminfo", ROOMY);
    fake.console.secret(b"1234");
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let result = with_session(&fake, &settings, |session| {
        session.try_tpm2_token(target, &token("token-tpm2-pin.json"))
    });
    assert!(result.is_ok());
    let lines = fake.runner.lines();
    assert!(lines.iter().any(|line| line.contains("--token-id 0")));
    assert!(!lines.iter().any(|line| line.starts_with("tpm2_load")));
}

#[test]
fn no_argon2_work_is_done_for_a_blob_that_does_not_load() {
    let fake = v3_machine();
    fake.runner.respond_program("tpm2_load", 1, "");
    let result = try_v3(&fake, &simulated(), &[PIN]);
    assert!(ended_with(&result, TokenFailure::NoPin));
    let output = fake.console.output_text();
    assert!(!output.contains("Checking the PIN"));
    assert!(output.contains("The TPM did not release the key"));
    assert!(stdins(&fake, "tpm2_unseal").is_empty());
    assert!(fake.files.contents("/proc/self/oom_score_adj").is_none());
}

#[test]
fn a_released_key_that_opens_no_slot_ends_the_token_without_asking_again() {
    let fake = v3_machine();
    fake.runner.respond_program("cryptsetup", 2, "");
    let result = try_v3(&fake, &simulated(), &[PIN, PIN]);
    assert!(ended_with(&result, TokenFailure::Mismatch));
    assert_eq!(fake.console.remaining().len(), 1);
    assert!(
        fake.kmsg().contains(
            "TPM2 token 0 on /dev/sda: the TPM released the key, but it opened no key slot"
        )
    );
    assert!(
        !fake
            .console
            .output_text()
            .contains("The TPM did not release the key")
    );
}

#[test]
fn a_malformed_argon2id_token_is_refused_before_any_pin() {
    let json = fixture("token-tpm2-argon2id.json")
        .replace(r#""tpm2_argon2id_lanes":1"#, r#""tpm2_argon2id_lanes":0"#);
    let malformed = Token::from_export("0", &json).unwrap();
    let fake = tpm_machine(None, b"s");
    let (result, asked) = try_without_plugin(&fake, &malformed);
    assert!(ended_with(&result, TokenFailure::Malformed) && !asked);
    let kmsg = fake.kmsg();
    assert!(kmsg.contains("TPM2 token 0 on /dev/sda is malformed: tpm2_argon2id_lanes is 0"));
    assert!(!kmsg.contains("needs the systemd-tpm2 cryptsetup plugin"));
    let output = fake.console.output_text();
    assert!(output.contains("This volume's TPM2 token is malformed: tpm2_argon2id_lanes is 0."));
    assert!(output.contains("Use another unlock method."));

    // A rule of the PIN's own: a salt too short to derive with.
    let short = fixture("token-tpm2-argon2id.json").replace(
        "Y0ea1poJCyWCd+yPum+ZQZov+ySJgVEGV8lEzNEUjpc=",
        "MTIzNDU2Nw==",
    );
    let short = Token::from_export("0", &short).unwrap();
    let fake = tpm_machine(None, b"s");
    let (result, asked) = try_without_plugin(&fake, &short);
    assert!(ended_with(&result, TokenFailure::Malformed) && !asked);
    assert!(
        fake.kmsg()
            .contains("malformed: tpm2_salt has 7 bytes, fewer than 8")
    );
}

#[test]
fn an_argon2id_token_with_a_signed_policy_is_refused_before_any_pin() {
    let signed = token_with("token-tpm2-argon2id.json", r#""tpm2_pubkey":"AA==""#);
    let refused = "TPM2 token 0 on /dev/sda uses systemd 262's Argon2id PIN, which needs \
                   tpm2-tools in the image and no signed or pcrlock policy";
    // With the plugin, which before systemd 262 would derive it wrong.
    let fake = v3_machine();
    fake.files.file(
        "/usr/lib/cryptsetup/libcryptsetup-token-systemd-tpm2.so",
        "",
    );
    let (result, asked) = try_without_plugin(&fake, &signed);
    assert!(ended_with(&result, TokenFailure::Unsupported) && !asked);
    assert!(fake.kmsg().contains(refused));
    assert!(
        fake.runner
            .lines()
            .iter()
            .all(|line| line.starts_with("tpm2_getcap"))
    );
    // Without the tools.
    let fake = FakeSystem::new();
    let (result, asked) = try_without_plugin(&fake, &v3());
    assert!(ended_with(&result, TokenFailure::Unsupported) && !asked);
    assert!(fake.kmsg().contains(refused));
}

#[test]
fn an_argon2id_token_found_unsupported_after_its_pin_ends_without_asking_again() {
    // The check before the PIN found the tools; here they are gone.
    let fake = FakeSystem::new();
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let result = with_session(&fake, &simulated(), |session| {
        let pin = Secret::copy_from(PIN.as_bytes());
        session.tpm2_open_with_pin(target, "0", &v3(), &pin)
    });
    assert_eq!(result.err(), Some(TokenFailure::Unsupported));
    assert!(
        fake.kmsg()
            .contains("uses systemd 262's Argon2id PIN, which needs tpm2-tools")
    );
}

#[test]
fn a_cached_pin_crosses_between_argon2id_and_pbkdf2_tokens() {
    let pbkdf2 = token("token-tpm2-pin.json");
    let salt = base64_decode(pbkdf2.text("tpm2_salt").unwrap()).unwrap();
    let fake = v3_machine();
    // The PBKDF2 token's object releases its secret for V3's PIN too.
    let auth = stdin_auth(auth_value(PIN.as_bytes(), Some(&salt)).expose());
    fake.runner.first(move |command| {
        let right = matches!(command.get_stdin(), Input::Secret(given) if given.expose() == auth);
        (command.program() == "tpm2_unseal" && right).then(|| {
            Ok(Output::with_secret(
                Status::exited(0),
                Secret::copy_from(b"pbkdf2"),
            ))
        })
    });
    fake.console.secret(PIN.as_bytes());
    let args = [];
    with_session(&fake, &simulated(), |session| {
        let a = crate::testing::target("a", "/dev/sda", &args);
        assert!(session.try_tpm2_token(a, &v3()).is_ok());
        let b = crate::testing::target("b", "/dev/sdb", &args);
        assert!(session.try_tpm2_token(b, &pbkdf2).is_ok());
        let c = crate::testing::target("c", "/dev/sdc", &args);
        assert!(session.try_tpm2_token(c, &v3()).is_ok());
    });
    // One PIN typed; each token derived it its own way.
    assert!(fake.console.remaining().is_empty());
    assert_eq!(
        fake.console
            .output_text()
            .matches("TPM2 PIN (leave empty")
            .count(),
        1
    );
    let passphrases = stdins(&fake, "cryptsetup");
    let pbkdf2 = base64_encode(b"pbkdf2");
    assert_eq!(
        passphrases,
        [
            PASSPHRASE.as_bytes(),
            pbkdf2.expose(),
            PASSPHRASE.as_bytes()
        ]
    );
}

#[test]
fn a_typed_pin_that_is_cached_already_is_not_cached_twice() {
    // Volume a opens with V3's PIN, which is cached. Volume b's first token
    // refuses it, and its second, offered no cached PIN, opens with it
    // typed again: the cache keeps it once, so volume c's first token,
    // with another salt that refuses it too, derives it once.
    let salted = |salt: &str| {
        let json = fixture("token-tpm2-argon2id.json")
            .replace("Y0ea1poJCyWCd+yPum+ZQZov+ySJgVEGV8lEzNEUjpc=", salt);
        Token::from_export("0", &json).unwrap()
    };
    let other = salted("T3RoZXIgc2FsdCwgMzIgYnl0ZXMgbG9uZywgaGVyZS4=");
    let third = salted("QSB0aGlyZCBzYWx0LCAzMiBieXRlcyBsb25nLCB0b28=");
    let fake = v3_machine();
    for pin in [PIN, "", PIN, ""] {
        fake.console.secret(pin.as_bytes());
    }
    let args = [];
    let cached = with_session(&fake, &simulated(), |session| {
        let a = crate::testing::target("a", "/dev/sda", &args);
        assert!(session.try_tpm2_token(a, &v3()).is_ok());
        let b = crate::testing::target("b", "/dev/sdb", &args);
        assert!(session.try_tpm2_token(b, &other).is_err());
        assert!(session.try_tpm2_token(b, &v3()).is_ok());
        let c = crate::testing::target("c", "/dev/sdc", &args);
        assert!(session.try_tpm2_token(c, &third).is_err());
        session.tpm2_pins.len()
    });
    assert_eq!(cached, 1);
    // a's, b's first token's cached PIN, b's second token's typed PIN, and
    // c's first token's cached PIN.
    assert_eq!(stdins(&fake, "tpm2_unseal").len(), 4);
    assert!(fake.console.remaining().is_empty());
}

#[test]
fn argon2id_secrets_stay_out_of_arguments_files_and_logs() {
    let fake = v3_machine();
    assert!(try_v3(&fake, &simulated(), &["wrong pin", PIN]).is_ok());
    let mut work = WorkMemory::new(0).unwrap();
    let split = argon2id_split(PIN.as_bytes(), &sha256(b"salt"), 1024, 3, 1, &mut work).unwrap();
    for secret in [
        PIN.as_bytes(),
        b"wrong pin",
        AUTH.as_bytes(),
        PASSPHRASE.as_bytes(),
        split.tpm_pin.expose(),
        hex(split.key1.expose()).expose(),
        base64_encode(split.key1.expose()).expose(),
        hex(&sha256(b"unsealed")).expose(),
    ] {
        fake.assert_no_leak(secret);
    }
}

#[test]
fn a_no_pin_token_ignores_complete_argon2id_fields() {
    // V3's fields on a token without a PIN: nothing is derived, as systemd
    // derives nothing.
    let json =
        fixture("token-tpm2-argon2id.json").replace(r#""tpm2-pin":true"#, r#""tpm2-pin":false"#);
    let pinless = Token::from_export("0", &json).unwrap();
    let fake = tpm_machine(None, b"secret");
    fake.files.file("/proc/meminfo", ROOMY);
    let (result, _) = try_without_plugin(&fake, &pinless);
    assert!(result.is_ok());
    assert!(!fake.console.output_text().contains("Checking the PIN"));
    assert_eq!(stdins(&fake, "cryptsetup"), [b"c2VjcmV0"]);
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
        assert!(ended_with(&result, TokenFailure::Malformed), "{fields}");
        assert!(fake.kmsg().contains(why), "{why}");
        assert!(!ran_tpm2_tools(&fake), "{fields}");
        // Nobody is told about a token that asks nothing.
        assert!(
            !fake.console.output_text().contains("malformed"),
            "{fields}"
        );

        // The plugin is not given it either.
        let (fake, settings) = plugin_machine();
        let args = [];
        let target = crate::testing::target("v", "/dev/sda", &args);
        let result = with_session(&fake, &settings, |session| {
            session.try_tpm2_token(target, &pinless)
        });
        assert!(ended_with(&result, TokenFailure::Malformed), "{fields}");
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
    assert!(ended_with(&result, TokenFailure::Malformed));
    assert!(!fake.console.remaining().is_empty());
    assert!(
        fake.kmsg()
            .contains("TPM2 token 0 on /dev/sda is malformed: tpm2_argon2id_lanes is 0")
    );
    assert!(fake.runner.lines().is_empty());
}
