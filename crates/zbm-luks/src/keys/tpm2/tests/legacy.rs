//! The legacy runtime's TPM2 token code with tokens of systemd 262, run
//! against fake tools: it refuses PINs hardened with Argon2id, and reads
//! the signed policy's field by the name systemd writes.

use super::*;
use crate::testing::legacy_luks;
use zbm_testkit::TempDir;
use zbm_testkit::legacy::{log_lines, shell_quote};

/// What a legacy function did: its exit status, what it printed, its log
/// and the fake tools it ran, one line each: `cryptsetup` with its
/// arguments, the TPM2 tools by name only, since their arguments name
/// temporary files.
type Legacy = (i32, String, String, String);

/// The TPM2 tools of the legacy native path, and `cryptsetup`.
const TOOLS: [&str; 8] = [
    "cryptsetup",
    "tpm2_getcap",
    "tpm2_load",
    "tpm2_startauthsession",
    "tpm2_policypcr",
    "tpm2_policyauthvalue",
    "tpm2_unseal",
    "tpm2_flushcontext",
];

/// Runs `function` with `args` from the legacy library, with the fake
/// [`TOOLS`] first in `PATH`, each of which notes that it ran and runs the
/// shell code `body(tool)`, and with the shell code `stubs` after the
/// library.
fn legacy_run(
    body: impl Fn(&str) -> &'static str,
    stubs: &str,
    function: &str,
    args: &[&str],
    stdin: &[u8],
) -> Legacy {
    let dir = TempDir::new("zbm-luks-legacy-tpm2");
    for tool in TOOLS {
        let noted = if tool == "cryptsetup" {
            "cryptsetup $*"
        } else {
            tool
        };
        let script = format!(
            "#!/bin/sh\necho \"{noted}\" >> \"$CALLS\"\n{}\n",
            body(tool)
        );
        dir.write_executable(format!("bin/{tool}"), script);
    }
    let calls = dir.write("calls", "");
    let prelude = format!(
        "export CALLS={} PATH={}:\"$PATH\" LUKS_TPM2_TCTI=swtpm:port=2321\n\
         stty() {{ :; }}\ntput() {{ :; }}\ncolorize() {{ shift; printf '%b' \"$*\"; }}\n{stubs}",
        shell_quote(&calls.to_string_lossy()),
        shell_quote(&dir.join("bin").to_string_lossy()),
    );
    let output = legacy_luks().code(&prelude).call(function, args, stdin);
    (
        output.status.code().unwrap_or(-1),
        output.stdout_text(),
        log_lines(&output.stderr_text()),
        std::fs::read_to_string(calls).unwrap(),
    )
}

/// The fixture `name` with the JSON members `fields` added.
fn with_fields(name: &str, fields: &str) -> String {
    let json = fixture(name);
    format!("{},{fields}}}", json.trim_end().strip_suffix('}').unwrap())
}

/// What the legacy `luks_try_tpm2_token` does with token 0 of `json` on
/// `/dev/sda`, with a plugin installed or not, when `1234` is typed at
/// its prompt. Every fake tool fails.
fn legacy_try(json: &str, plugin: bool) -> Legacy {
    let plugin = format!("luks_have_token_plugin() {{ {plugin}; }}");
    let args = ["v", "/dev/sda", "0", json];
    legacy_run(
        |_| "exit 1",
        &plugin,
        "luks_try_tpm2_token",
        &args,
        b"1234\n",
    )
}

#[test]
fn legacy_refuses_argon2id_pin_tokens_before_the_plugin() {
    let mut parity = parity!("legacy_refuses_argon2id_pin_tokens_before_the_plugin");
    let tools = have_program("base64") && have_program("od") && have_program("sha256sum");
    if !can_run(
        !parity.mode().needs_legacy() || tools,
        "base64, od or sha256sum",
    ) {
        return;
    }
    let refused = "WARN: TPM2 token 0 on /dev/sda uses systemd 262's Argon2id PIN, \
                   which this runtime does not support\n";
    for name in ["token-tpm2-argon2id.json", "token-tpm2-argon2id-f45.json"] {
        let json = fixture(name);
        for plugin in [true, false] {
            let (status, stdout, log, calls) = parity
                .legacy(format!("{name} plugin={plugin}"), || {
                    legacy_try(&json, plugin)
                });
            // Unsupported, with no PIN asked for and no command run: not
            // the plugin, not the lockout probe, not the TPM.
            assert_eq!(status, 2, "{name} {plugin}");
            assert!(!stdout.contains("TPM2 PIN"), "{name} {plugin}: {stdout}");
            assert_eq!(log, refused, "{name} {plugin}");
            assert_eq!(calls, "", "{name} {plugin}");
        }
    }

    // The same token without the Argon2id fields asks for the PIN and goes
    // to the plugin.
    let (status, stdout, log, calls) = parity.legacy("token-tpm2-pin.json plugin=true", || {
        legacy_try(&fixture("token-tpm2-pin.json"), true)
    });
    assert_eq!(status, 1);
    assert!(stdout.contains("TPM2 PIN"), "{stdout}");
    assert!(!log.contains("Argon2id"), "{log}");
    let plugin = "cryptsetup open --type luks --token-only --token-id 0 /dev/sda v\n";
    assert_eq!(calls, format!("tpm2_getcap\n{plugin}tpm2_getcap\n"));

    // Without a PIN, the legacy runtime does not look at valid Argon2id
    // fields; systemd checks them but derives nothing with them.
    let json = with_fields(
        "token-tpm2.json",
        r#""tpm2_argon2id_memcost":1049599,"tpm2_argon2id_iterations":3,"tpm2_argon2id_lanes":1"#,
    );
    let (status, _, log, calls) = parity
        .legacy("token-tpm2.json with Argon2id fields plugin=true", || {
            legacy_try(&json, true)
        });
    assert_eq!(status, 1);
    assert!(!log.contains("Argon2id"), "{log}");
    assert!(calls.starts_with(plugin), "{calls}");
    assert!(calls.contains("\ntpm2_load\n"), "{calls}");
}

/// What the legacy `luks_tpm2_native_unseal` does with `json` and no PIN,
/// when every fake tool succeeds and `tpm2_unseal` releases `s`.
fn legacy_unseal(json: &str) -> Legacy {
    let body = |tool: &str| {
        if tool == "tpm2_unseal" {
            "printf s"
        } else {
            "exit 0"
        }
    };
    let dir = TempDir::new("zbm-luks-legacy-key");
    let key = dir.join("key").to_string_lossy().into_owned();
    legacy_run(body, "", "luks_tpm2_native_unseal", &[json, "", &key], b"")
}

/// The legacy function's status for `outcome`: 0 for a key, 2 when the
/// token needs the plugin, 1 otherwise.
fn legacy_status(outcome: &Outcome) -> i32 {
    match outcome {
        Outcome::Key(_) => 0,
        Outcome::Unsupported => 2,
        _ => 1,
    }
}

#[test]
fn legacy_reads_systemd_s_tpm2_pubkey_field() {
    let mut parity = parity!("legacy_reads_systemd_s_tpm2_pubkey_field");
    let tools = have_program("base64") && have_program("od") && have_program("sha256sum");
    if !can_run(
        !parity.mode().needs_legacy() || tools,
        "base64, od or sha256sum",
    ) {
        return;
    }
    // A token both runtimes unseal, but for its signed policy, which
    // systemd writes as tpm2_pubkey.
    let plain = fixture("token-tpm2.json");
    let signed = with_fields("token-tpm2.json", r#""tpm2_pubkey":"AA==""#);
    let fake = tpm_machine(None, b"s");
    for (case, json) in [("plain", plain), ("signed", signed)] {
        let token = Token::from_export("0", &json).unwrap();
        let (status, _, log, calls) = parity.legacy(case, || legacy_unseal(&json));
        let rust = with_session(&fake, &Settings::new("/zfsbootmenu"), |session| {
            session.tpm2_native_unseal(&token, None)
        });
        assert_eq!(status, legacy_status(&rust), "{case}: {log}");
        if case == "signed" {
            assert_eq!(status, 2);
            assert_eq!(log, "DEBUG: TPM2 token uses a signed or pcrlock policy\n");
            assert_eq!(calls, "");
        } else {
            assert_eq!(status, 0);
            assert!(calls.starts_with("tpm2_load\n"), "{calls}");
        }
    }
}
