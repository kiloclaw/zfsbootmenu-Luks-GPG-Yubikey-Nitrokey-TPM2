use super::*;
use crate::keys::tests::with_session;
use crate::testing::{legacy_function, legacy_luks};
use crate::unlocker::Settings;
use zbm_platform::exec::{Input, Output, Status};
use zbm_testkit::fakes::FakeSystem;
use zbm_testkit::{can_run, have_program, parity, repo_root};

fn fixture(name: &str) -> String {
    let path = repo_root()
        .join("crates/zbm-luks/tests/fixtures")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

/// The token 0 of a fixture of `cryptsetup token export`.
fn token(name: &str) -> Token {
    Token::from_export("0", &fixture(name)).unwrap()
}

/// The token 0 of the fixture `name`, with the JSON members `fields` added.
fn token_with(name: &str, fields: &str) -> Token {
    let json = fixture(name);
    let json = format!("{},{fields}}}", json.trim_end().strip_suffix('}').unwrap());
    Token::from_export("0", &json).unwrap()
}

/// The PIN block of the legacy `luks_tpm2_native_unseal`, which turns the
/// PIN into the authorization value.
fn legacy_auth(json: &str, pin: &str) -> String {
    let function = legacy_function(
        "zfsbootmenu/lib/zfsbootmenu-luks-keys.sh",
        "luks_tpm2_native_unseal",
    );
    let start = function.find("  if [ -n \"${pin}\" ]; then\n").unwrap();
    let end = start + function[start..].find("\n  fi\n\n  ret=1").unwrap() + 5;
    let block = &function[start..end];
    let code = format!(
        "derive() {{ local json=\"$1\" pin=\"$2\" work=/nonexistent salt hexsalt auth\n\
         {block}\necho \"${{auth}}\"; }}"
    );
    let output = legacy_luks().code(&code).call("derive", &[json, pin], b"");
    output.stdout_text().trim_end().to_owned()
}

#[test]
fn authorization_values_match_the_legacy_derivation() {
    let mut parity = parity!("authorization_values_match_the_legacy_derivation");
    let tools = have_program("sha256sum") && have_program("openssl");
    if !can_run(
        !parity.mode().needs_legacy() || tools,
        "sha256sum or openssl",
    ) {
        return;
    }
    let salted = fixture("token-tpm2-pin.json");
    let plain = fixture("token-tpm2.json");
    for pin in ["1234", "0000", "pïn with spaces", "-x", "a\\b"] {
        for (token, json) in [("salted", &salted), ("plain", &plain)] {
            let salt = Token::from_export("0", json)
                .unwrap()
                .text("tpm2_salt")
                .map(|salt| base64_decode(salt).unwrap());
            let rust = auth_value(pin.as_bytes(), salt.as_deref());
            let legacy: String =
                parity.legacy(format!("{pin:?} {token}"), || legacy_auth(json, pin));
            assert_eq!(String::from_utf8_lossy(rust.expose()), legacy, "{pin:?}");
        }
    }
}

#[test]
fn trailing_zero_bytes_are_left_out() {
    // A PIN whose hash ends in a zero byte is rare, so check the trimming
    // directly.
    let digest = sha256(b"1234");
    let full = hex(&digest);
    assert_eq!(auth_value(b"1234", None).expose(), full.expose());
    let mut found = None;
    for n in 0_u32..5000 {
        let pin = n.to_string();
        if sha256(pin.as_bytes())[31] == 0 {
            found = Some(pin);
            break;
        }
    }
    let pin = found.unwrap();
    let auth = auth_value(pin.as_bytes(), None);
    assert!(auth.len() <= 62 && auth.len().is_multiple_of(2));
    assert!(!auth.expose().ends_with(b"00"));
}

#[test]
fn blobs_hold_a_private_and_a_public_part() {
    let blob = base64_decode(token("token-tpm2.json").text("tpm2-blob").unwrap()).unwrap();
    let (private, public) = split_blob(&blob).unwrap();
    assert_eq!(private.len(), 0x9e + 2);
    assert_eq!(private.len() + public.len(), blob.len());
    let mut seeded = blob.clone();
    seeded.extend_from_slice(b"seed");
    assert!(split_blob(&seeded).is_none());
    assert!(split_blob(&blob[..1]).is_none());
    assert!(split_blob(&[0, 0, 0]).is_none());
    assert_eq!(split_blob(&[0, 0, 0, 0]), Some((&[0, 0][..], &[0, 0][..])));
}

/// A machine with every tool, whose `tpm2_unseal` releases `secret` when
/// it gets `auth` (or none) on standard input.
fn tpm_machine(auth: Option<Vec<u8>>, secret: &'static [u8]) -> FakeSystem {
    let fake = FakeSystem::new();
    fake.environment.set("PATH", "/bin");
    fake.files.dir("/run");
    for tool in TOOLS.iter().chain(&["tpm2_getcap"]) {
        fake.files.file_with_mode(format!("/bin/{tool}"), "", 0o755);
    }
    fake.runner.on(move |command| {
        if command.program() != "tpm2_unseal" {
            return None;
        }
        let given = match command.get_stdin() {
            Input::Secret(text) => Some(text.expose().to_vec()),
            _ => None,
        };
        Some(Ok(if given == auth {
            Output::with_secret(Status::exited(0), Secret::copy_from(secret))
        } else {
            Output::new(Status::exited(1), "")
        }))
    });
    fake
}

#[test]
fn unsealing_runs_the_policy_and_keeps_the_secret_off_files() {
    let json = token("token-tpm2-pin.json");
    let salt = base64_decode(json.text("tpm2_salt").unwrap()).unwrap();
    let mut auth = b"hex:".to_vec();
    auth.extend_from_slice(auth_value(b"1234", Some(&salt)).expose());
    let fake = tpm_machine(Some(auth), b"\x01sealed\x00secret");
    let mut settings = Settings::new("/zfsbootmenu");
    settings.tpm2_tcti = "swtpm:port=2321".to_owned();
    with_session(&fake, &settings, |session| {
        let pin = Secret::copy_from(b"1234");
        match session.tpm2_native_unseal(&json, Some(&pin)) {
            Outcome::Key(key) => assert_eq!(key.expose(), b"AXNlYWxlZABzZWNyZXQ="),
            _ => panic!("the token was not unsealed"),
        }
        let wrong = Secret::copy_from(b"0000");
        assert!(matches!(
            session.tpm2_native_unseal(&json, Some(&wrong)),
            Outcome::Failed
        ));
    });
    let lines = fake.runner.lines();
    let work = "/run/zbm-tpm2.4242-0";
    assert_eq!(
        lines[..7],
        [
            format!("tpm2_load -C 0x81000001 -u {work}/pub -r {work}/priv -c {work}/obj.ctx"),
            format!("tpm2_startauthsession --policy-session -c 0x81000001 -S {work}/session.ctx"),
            format!("tpm2_sessionconfig {work}/session.ctx --enable-encrypt"),
            format!("tpm2_policypcr -S {work}/session.ctx -l sha256:7"),
            format!("tpm2_policyauthvalue -S {work}/session.ctx"),
            format!("tpm2_unseal -c {work}/obj.ctx -p session:{work}/session.ctx+file:-"),
            format!("tpm2_flushcontext {work}/session.ctx"),
        ]
    );
    assert_eq!(lines[7], "tpm2_flushcontext -t");
    assert!(fake.runner.calls().iter().all(|call| {
        call.get_env().iter().any(|(key, value)| {
            key == "TPM2TOOLS_TCTI" && value.as_deref() == Some("swtpm:port=2321".as_ref())
        })
    }));
    // Nothing is left behind, and the PIN went nowhere it could be read.
    assert!(
        !fake
            .files
            .paths()
            .iter()
            .any(|path| path.starts_with("/run/zbm-tpm2"))
    );
    fake.assert_no_leak(b"1234");
    fake.assert_no_leak(b"sealed");
}

#[test]
fn tokens_that_need_the_plugin_are_reported() {
    let fake = tpm_machine(None, b"s");
    with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        let signed = r#"{"type":"systemd-tpm2","tpm2_pubkey":"AA==","tpm2_srk":"x"}"#;
        assert!(matches!(
            session.tpm2_native_unseal(&crate::keys::tests::token(signed), None),
            Outcome::Unsupported
        ));
        let old = r#"{"type":"systemd-tpm2","tpm2-blob":"AAAAAA=="}"#;
        assert!(matches!(
            session.tpm2_native_unseal(&crate::keys::tests::token(old), None),
            Outcome::Unsupported
        ));
        let seeded = r#"{"tpm2_srk":"x","tpm2-blob":"AAAAAAA="}"#;
        assert!(matches!(
            session.tpm2_native_unseal(&crate::keys::tests::token(seeded), None),
            Outcome::Unsupported
        ));
    });
    let debug = fake.kmsg();
    assert!(debug.contains("TPM2 token uses a signed or pcrlock policy"));
    assert!(debug.contains("TPM2 token predates systemd's persistent SRK"));
    assert!(debug.contains("TPM2 blob has an unsupported layout"));
}

#[test]
fn pins_are_asked_for_and_kept_for_other_volumes() {
    let json = token("token-tpm2-pin.json");
    let salt = base64_decode(json.text("tpm2_salt").unwrap()).unwrap();
    let mut auth = b"hex:".to_vec();
    auth.extend_from_slice(auth_value(b"1234", Some(&salt)).expose());
    let fake = tpm_machine(Some(auth), b"secret");
    fake.files.device("/dev/tpmrm0");
    fake.console.secret(b"0000").secret(b"1234");
    let args = [];
    with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        let a = crate::testing::target("a", "/dev/sda", &args);
        assert!(session.try_tpm2_token(a, &json).is_ok());
        // The second volume gets the PIN that worked, without asking.
        let b = crate::testing::target("b", "/dev/sdb", &args);
        assert!(session.try_tpm2_token(b, &json).is_ok());
    });
    let output = fake.console.output_text();
    assert_eq!(output.matches("TPM2 PIN (leave empty").count(), 2);
    assert!(output.contains("The TPM did not release the key"));
    let opened: Vec<String> = fake
        .runner
        .lines()
        .into_iter()
        .filter(|line| line.starts_with("cryptsetup open"))
        .collect();
    assert_eq!(
        opened,
        [
            "cryptsetup open --type luks --key-file=- /dev/sda a",
            "cryptsetup open --type luks --key-file=- /dev/sdb b",
        ]
    );
    fake.assert_no_leak(b"1234");
}

#[test]
fn a_locked_tpm_is_not_asked_for_pins() {
    let fake = tpm_machine(None, b"s");
    fake.files.device("/dev/tpmrm0");
    fake.runner.respond(
        "tpm2_getcap properties-variable",
        0,
        "TPM2_PT_PERMANENT:\n  inLockout:                 1\n",
    );
    let args = [];
    let target = crate::testing::target("a", "/dev/sda", &args);
    with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        assert!(
            session
                .try_tpm2_token(target, &token("token-tpm2-pin.json"))
                .is_err()
        );
    });
    assert!(
        fake.console
            .output_text()
            .contains("locked against PIN guessing")
    );
    assert!(
        fake.kmsg()
            .contains("TPM2 in dictionary-attack lockout; skipping PIN token 0")
    );
}

#[test]
fn missing_tpm_devices_are_probed_once() {
    let fake = FakeSystem::new();
    fake.files.dir("/sys/class/tpm");
    let args = [];
    let target = crate::testing::target("a", "/dev/sda", &args);
    with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        assert!(
            session
                .try_tpm2_token(target, &crate::keys::tests::token("{}"))
                .is_err()
        );
        assert!(
            session
                .try_tpm2_token(target, &crate::keys::tests::token("{}"))
                .is_err()
        );
    });
    assert_eq!(
        fake.runner.lines(),
        [
            "modprobe -q tpm_crb",
            "modprobe -q tpm_tis",
            "udevadm settle"
        ]
    );
}

/// A machine with a TPM (not a device, so it is always there) and the
/// systemd-tpm2 plugin, whose `cryptsetup` accepts the PIN `1234`.
fn plugin_machine() -> (FakeSystem, Settings) {
    let fake = FakeSystem::new();
    fake.files.file(
        "/usr/lib/cryptsetup/libcryptsetup-token-systemd-tpm2.so",
        "",
    );
    fake.runner.on(|command| {
        let right = matches!(
            command.get_stdin(),
            Input::Secret(pin) if pin.expose().starts_with(b"1234")
        );
        let pinless = matches!(command.get_stdin(), Input::Null);
        let status = if right || pinless { 0 } else { 2 };
        Some(Ok(Output::new(Status::exited(status), "")))
    });
    let mut settings = Settings::new("/zfsbootmenu");
    settings.tpm2_tcti = "swtpm:port=2321".to_owned();
    (fake, settings)
}

#[test]
fn the_plugin_opens_tokens_with_and_without_a_pin() {
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let (fake, settings) = plugin_machine();
    fake.console.secret(b"bad").secret(b"1234");
    with_session(&fake, &settings, |session| {
        assert!(
            session
                .try_tpm2_token(target, &token("token-tpm2-pin.json"))
                .is_ok()
        );
        // The PIN that worked is tried first on the next volume.
        assert_eq!(session.tpm2_pins.len(), 1);
        assert!(
            session
                .try_tpm2_token(target, &token("token-tpm2-pin.json"))
                .is_ok()
        );
        assert_eq!(session.tpm2_pins.len(), 1);
        assert!(
            session
                .try_tpm2_token(target, &token("token-tpm2.json"))
                .is_ok()
        );
    });
    let output = fake.console.output_text();
    assert!(output.contains("The TPM did not release the key"));
    assert!(fake.kmsg().contains("unlocked /dev/sda with TPM2 and PIN"));

    // An empty PIN gives up; so do three wrong ones.
    let (fake, settings) = plugin_machine();
    fake.console.secret(b"");
    let given_up = with_session(&fake, &settings, |session| {
        session.try_tpm2_token(target, &token("token-tpm2-pin.json"))
    });
    assert!(given_up.is_err());
    let (fake, settings) = plugin_machine();
    fake.console
        .secret(b"a")
        .secret(b"b")
        .secret(b"c")
        .secret(b"d");
    let too_many = with_session(&fake, &settings, |session| {
        session.try_tpm2_token(target, &token("token-tpm2-pin.json"))
    });
    assert!(too_many.is_err());
    // Three PINs, three attempts to open.
    let opens = fake.runner.lines();
    let opens = opens
        .iter()
        .filter(|line| line.starts_with("cryptsetup open"));
    assert_eq!(opens.count(), 3);
}

#[test]
fn tokens_with_systemd_s_tpm2_pubkey_field_need_the_plugin() {
    // A token ZFSBootMenu could unseal, but for its signed policy, which
    // systemd writes as tpm2_pubkey.
    let signed = token_with("token-tpm2.json", r#""tpm2_pubkey":"AA==""#);
    let fake = tpm_machine(None, b"s");
    with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        assert!(matches!(
            session.tpm2_native_unseal(&token("token-tpm2.json"), None),
            Outcome::Key(_)
        ));
        assert!(matches!(
            session.tpm2_native_unseal(&signed, None),
            Outcome::Unsupported
        ));
    });
    assert!(
        fake.kmsg()
            .contains("TPM2 token uses a signed or pcrlock policy")
    );
}

/// The result of `token`'s PIN token on `fake`, without the plugin, and
/// whether a PIN was asked for.
fn try_without_plugin(fake: &FakeSystem, token: &Token) -> (Result<OpenMapping, LuksError>, bool) {
    fake.console.secret(b"1234");
    let mut settings = Settings::new("/zfsbootmenu");
    settings.tpm2_tcti = "swtpm:port=2321".to_owned();
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let result = with_session(fake, &settings, |session| {
        session.try_tpm2_token(target, token)
    });
    (result, fake.console.remaining().is_empty())
}

#[test]
fn a_token_the_image_cannot_open_asks_no_pin_without_the_plugin() {
    let unsupported = |result: Result<OpenMapping, LuksError>| {
        matches!(
            result,
            Err(LuksError::Token {
                failure: TokenFailure::Unsupported,
                ..
            })
        )
    };
    // No tools.
    let fake = FakeSystem::new();
    let (result, asked) = try_without_plugin(&fake, &token("token-tpm2-pin.json"));
    assert!(unsupported(result) && !asked);
    let kmsg = fake.kmsg();
    assert!(kmsg.contains("native TPM2 unlocking needs tpm2_load"));
    assert!(kmsg.contains("TPM2 token 0 on /dev/sda needs the systemd-tpm2 cryptsetup plugin"));

    // A signed policy.
    let signed = token_with("token-tpm2-pin.json", r#""tpm2_pubkey":"AA==""#);
    let fake = tpm_machine(None, b"s");
    let (result, asked) = try_without_plugin(&fake, &signed);
    assert!(unsupported(result) && !asked);
    assert!(
        fake.kmsg()
            .contains("needs the systemd-tpm2 cryptsetup plugin")
    );
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("tpm2_load"))
    );

    // A PIN hardened with Argon2id, which only systemd 262's plugin knows.
    let fake = tpm_machine(None, b"s");
    let (result, asked) = try_without_plugin(&fake, &token("token-tpm2-argon2id-f45.json"));
    assert!(unsupported(result) && !asked);
    let kmsg = fake.kmsg();
    assert!(
        kmsg.contains(
            "TPM2 token uses systemd 262's Argon2id PIN (3256447 KiB, 2 passes, 24 lanes)"
        )
    );
    assert!(kmsg.contains(
        "TPM2 token 0 on /dev/sda uses systemd 262's Argon2id PIN, which only the \
         systemd-tpm2 cryptsetup plugin of systemd 262 or later opens"
    ));
    assert!(!kmsg.contains("needs the systemd-tpm2 cryptsetup plugin"));
    assert!(
        !fake
            .runner
            .lines()
            .iter()
            .any(|line| line.starts_with("tpm2_") && !line.starts_with("tpm2_getcap"))
    );
}

#[test]
fn a_pin_token_without_a_salt_sends_the_pin_s_own_hash() {
    let json = fixture("token-tpm2-pin.json");
    let salt = token("token-tpm2-pin.json")
        .text("tpm2_salt")
        .unwrap()
        .to_owned();
    let unsalted = json.replace(&salt, "");
    let unsalted = Token::from_export("0", &unsalted).unwrap();
    let mut auth = b"hex:".to_vec();
    auth.extend_from_slice(auth_value(b"1234", None).expose());
    let fake = tpm_machine(Some(auth), b"secret");
    with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        let pin = Secret::copy_from(b"1234");
        assert!(matches!(
            session.tpm2_native_unseal(&unsalted, Some(&pin)),
            Outcome::Key(_)
        ));
    });
}

#[test]
fn a_token_found_unsupported_after_its_pin_ends_without_asking_again() {
    // The check before the PIN found the tools; here they are gone.
    let fake = FakeSystem::new();
    let args = [];
    let target = crate::testing::target("v", "/dev/sda", &args);
    let result = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
        let pin = Secret::copy_from(b"1234");
        session.tpm2_open_with_pin(target, "0", &token("token-tpm2-pin.json"), &pin)
    });
    assert_eq!(result.err(), Some(TokenFailure::Unsupported));
    assert!(
        fake.kmsg()
            .contains("TPM2 token 0 on /dev/sda needs the systemd-tpm2 cryptsetup plugin")
    );
}

mod argon2id;
