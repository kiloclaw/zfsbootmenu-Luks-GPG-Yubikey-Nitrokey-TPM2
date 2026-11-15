use pretty_assertions::assert_eq;

use super::setup::{
    PinKdf, clevis_decrypt, clevis_token, is_hardened_as_enrolled, pin_kdf, pin_options,
    token_types,
};
use super::*;
use crate::testing::{checks, cleanup, settings};

#[test]
fn the_tokens_of_a_volume_are_read_from_its_luks_metadata() {
    let dump = r#"{"keyslots": {"0": {"type": "luks2"}},
        "tokens": {"0": {"type": "systemd-tpm2", "keyslots": ["1"]},
                   "1": {"type": "clevis", "keyslots": ["2"]},
                   "2": {"type": "systemd-fido2"}}}"#;

    let types = token_types(dump).unwrap();

    assert_eq!(types, ["systemd-tpm2", "clevis"]);
    assert_eq!(token_types("{}").unwrap(), Vec::<String>::new());
}

#[test]
fn the_clevis_binding_is_checked_against_the_software_tpm_from_its_token() {
    let dump = r#"{"keyslots": {"0": {"type": "luks2"}},
        "tokens": {"0": {"type": "systemd-tpm2", "keyslots": ["1"]},
                   "1": {"type": "clevis", "keyslots": ["2"]}}}"#;
    let tcti = "swtpm:host=127.0.0.1,port=2321";

    let id = clevis_token(dump).unwrap();
    let decrypt = clevis_decrypt("/dev/loop7p1", id, tcti);

    let args: Vec<_> = decrypt.get_args().collect();
    assert_eq!(args.len(), 2);
    let line = args[1].to_string_lossy();
    assert!(
        line.starts_with("cryptsetup token export --token-id 1 /dev/loop7p1 | jose fmt"),
        "{line}"
    );
    assert!(
        line.ends_with("| jose jwe fmt -i- -c | clevis decrypt"),
        "{line}"
    );
    assert!(!line.contains("luks pass"), "{line}");
    let mut envs: Vec<_> = decrypt.get_envs().collect();
    envs.sort();
    let named = Some(std::ffi::OsStr::new(tcti));
    assert_eq!(
        envs,
        [
            (std::ffi::OsStr::new("TCTI"), named),
            (std::ffi::OsStr::new("TPM2TOOLS_TCTI"), named)
        ]
    );
    assert_eq!(clevis_token(r#"{"tokens": {}}"#), None);
    assert_eq!(clevis_token("not json"), None);
}

#[test]
fn every_boot_checks_both_pools_the_teardown_and_the_boot_environment() {
    let settings = settings("keys-boot-checks");
    let test = Test::new(&settings, "keys-notpm").unwrap();
    let boot = Boot {
        zbm_status: "  pool: zroot\n state: ONLINE\n  pool: tank\n state: DEGRADED".into(),
        teardown: Some((String::new(), String::new(), String::new())),
        ..Boot::default()
    };

    boot_checks(&test, &boot);

    assert_eq!(
        checks(&test),
        [
            "FAIL - ZFSBootMenu imported zroot and tank: {'zroot': 'ONLINE', 'tank': 'DEGRADED'}",
            "ok   - teardown exported pools and closed volumes (dm=[] pools=[] state=[])",
            "FAIL - kexec into the ZFS 2.4.4 boot environment",
        ]
    );
    cleanup(&settings);
}

#[test]
fn the_crypttab_names_the_mappings_and_the_key_file() {
    let entries: Vec<Vec<&str>> = CRYPTTAB
        .lines()
        .map(|line| line.split_whitespace().collect())
        .collect();

    assert_eq!(
        entries,
        [
            ["zroot-a", "PARTLABEL=zfs0", "none", "discard"],
            ["zroot-b", "PARTLABEL=zfs1", "none", "discard"],
            ["tank-a", "PARTLABEL=zfs2", "none", "-"],
            [
                "tank-b",
                "PARTLABEL=zfs3",
                "/etc/zfsbootmenu/keys/tank.key.gpg",
                "-"
            ],
        ]
    );
    assert_eq!(
        RC,
        "mkdir -p /etc/zfsbootmenu/keys && cp /build/keys/tank.key.gpg /etc/zfsbootmenu/keys/"
    );
}

#[test]
fn journal_lines_in_the_serial_log_are_copied_to_the_record() {
    let log = "\x1b[0mZBMTEST-STATUS-END\r\n\
        ZW|    412 WARN  zbm: clevis token 1 on /dev/vdc1 did not recover the key of slot 2: \
        clevis decrypt failed (1): ERROR: Unable to run tpm2_load | Unable to load TPM2 object\r\n\
        \x1b[1;1HZD|    412 DEBUG zbm: tpm2_load failed (1): ErrorCode (0x000001df)\r\n\
        not a journal line: ZX| other\r\n\
        ZC|    1      38 ms  clevis decrypt\r\n\
        ZBMTEST-TEARDOWN dm=[] pools=[] state=[]\r\n";

    let lines = journal_lines(log);

    assert_eq!(
        lines,
        [
            "ZW|    412 WARN  zbm: clevis token 1 on /dev/vdc1 did not recover the key of slot \
             2: clevis decrypt failed (1): ERROR: Unable to run tpm2_load | Unable to load TPM2 \
             object",
            "ZD|    412 DEBUG zbm: tpm2_load failed (1): ErrorCode (0x000001df)",
            "ZC|    1      38 ms  clevis decrypt",
        ]
    );
    let settings = settings("keys-journal");
    let test = Test::new(&settings, "keys-native").unwrap();
    record_journal(&test, log);
    record_journal(&test, "ZBMTEST-STATUS-END\r\n");
    let steps = std::fs::read_to_string(test.dir().join("steps.log")).unwrap();
    let recorded: Vec<&str> = steps
        .lines()
        .map(|line| line.get(11..).unwrap_or_default())
        .filter(|line| line.starts_with("note - ") || line.starts_with("      "))
        .collect();
    assert_eq!(
        recorded,
        [
            "note - the runtime's journal at teardown (warnings, key programs):",
            &format!("      {}", lines[0]),
            &format!("      {}", lines[1]),
            &format!("      {}", lines[2]),
            "note - the serial log has no line of the runtime's journal",
        ]
    );
    cleanup(&settings);
}

/// Lines of the help of systemd 262's `systemd-cryptenroll`.
const HELP_262: &str = "\
     --tpm2-with-pin=yes|no|direct
                       Whether to require entering a PIN to unlock the volume
     --tpm2-argon2id-memory=BYTES
                       Argon2id memory cost in bytes (default: 64M)
     --tpm2-argon2id-iterations=NUM
";

/// Lines of the help of systemd 255's `systemd-cryptenroll`.
const HELP_255: &str = "\
     --tpm2-with-pin=BOOL
                       Whether to require entering a PIN to unlock the volume
";

/// The LUKS metadata of a volume whose only token is `token`, as `cryptsetup
/// luksDump --dump-json-metadata` prints it.
fn dump_of(token: &str) -> String {
    format!(r#"{{"keyslots": {{"0": {{"type": "luks2"}}}}, "tokens": {{"0": {token}}}}}"#)
}

#[test]
fn enrolment_fixes_small_argon2id_parameters_where_cryptenroll_has_them() {
    let small = [
        "--tpm2-with-pin=yes",
        "--tpm2-argon2id-memory=64M",
        "--tpm2-argon2id-iterations=2",
        "--tpm2-argon2id-parallelism=4",
    ];

    assert_eq!(pin_options(HELP_262, 0), small);
    assert_eq!(pin_options(HELP_262, 2), small);
    let options = pin_options(HELP_262, 0);
    let small_kdf = PinKdf::Argon2id {
        memory: 64 << 20,
        passes: 2,
        lanes: 4,
    };
    assert!(is_hardened_as_enrolled(0, &options, &small_kdf));
    let benchmarked = PinKdf::Argon2id {
        memory: 3_334_601_937,
        passes: 2,
        lanes: 24,
    };
    assert!(!is_hardened_as_enrolled(0, &options, &benchmarked));
    assert!(!is_hardened_as_enrolled(0, &options, &PinKdf::None));
}

#[test]
fn enrolment_keeps_yes_on_hosts_before_262() {
    for index in 0..3 {
        let options = pin_options(HELP_255, index);

        assert_eq!(options, ["--tpm2-with-pin=yes"]);
        assert!(is_hardened_as_enrolled(index, &options, &PinKdf::Pbkdf2));
        assert!(is_hardened_as_enrolled(index, &options, &PinKdf::None));
    }
    assert_eq!(pin_options("", 0), ["--tpm2-with-pin=yes"]);
    let small_kdf = PinKdf::Argon2id {
        memory: 64 << 20,
        passes: 2,
        lanes: 4,
    };
    assert!(!is_hardened_as_enrolled(
        0,
        &pin_options(HELP_255, 0),
        &small_kdf
    ));
}

#[test]
fn zfs1_is_enrolled_direct_on_new_hosts() {
    let options = pin_options(HELP_262, 1);

    assert_eq!(options, ["--tpm2-with-pin=direct"]);
    assert!(is_hardened_as_enrolled(1, &options, &PinKdf::Pbkdf2));
    let small_kdf = PinKdf::Argon2id {
        memory: 64 << 20,
        passes: 2,
        lanes: 4,
    };
    assert!(!is_hardened_as_enrolled(1, &options, &small_kdf));
    // systemd 262's `direct` always salts the PIN: none means no PIN token.
    assert!(!is_hardened_as_enrolled(1, &options, &PinKdf::None));
    assert!(is_hardened_as_enrolled(
        3,
        &pin_options(HELP_262, 3),
        &PinKdf::None
    ));
}

#[test]
fn pin_kdf_reads_argon2id_pbkdf2_and_none_from_a_dump() {
    let f45 = include_str!("../../../../zbm-luks/tests/fixtures/token-tpm2-argon2id-f45.json");
    let pbkdf2 = r#"{"type": "systemd-tpm2", "keyslots": ["1"], "tpm2-pin": true,
        "tpm2_salt": "HsuY4RescHIpV8t0ZFuV8VIlynjVLBhUtmf9jPD2JV0="}"#;
    let unsalted = r#"{"type": "systemd-tpm2", "keyslots": ["1"], "tpm2-pin": true}"#;
    let no_pin = r#"{"type": "systemd-tpm2", "keyslots": ["1"], "tpm2-pin": false,
        "tpm2_salt": "HsuY4RescHIpV8t0ZFuV8VIlynjVLBhUtmf9jPD2JV0="}"#;

    let kdf = pin_kdf(&dump_of(f45));

    assert_eq!(
        kdf,
        PinKdf::Argon2id {
            memory: 3_334_601_937,
            passes: 2,
            lanes: 24
        }
    );
    assert_eq!(kdf.to_string(), "argon2id (3334601937 bytes, 2, 24)");
    assert_eq!(pin_kdf(f45), kdf, "a token export");
    let small = f45.replace("3334601937", "67108864").replace(":24}", ":4}");
    assert_eq!(pin_kdf(&small).to_string(), "argon2id (64 MiB, 2, 4)");
    assert_eq!(pin_kdf(&dump_of(pbkdf2)), PinKdf::Pbkdf2);
    assert_eq!(PinKdf::Pbkdf2.to_string(), "pbkdf2");
    assert_eq!(pin_kdf(&dump_of(unsalted)), PinKdf::None);
    assert_eq!(pin_kdf(&dump_of(no_pin)), PinKdf::None);
    assert_eq!(PinKdf::None.to_string(), "none");
    assert_eq!(pin_kdf(r#"{"tokens": {}}"#), PinKdf::None);
    assert_eq!(pin_kdf("not json"), PinKdf::None);
}

#[test]
fn a_yes_that_fell_back_to_direct_is_read_as_pbkdf2() {
    // systemd 262 without Argon2id in its libcrypto enrols `direct` for
    // `yes`, and still lists the Argon2id options.
    let fell_back = r#"{"type": "systemd-tpm2", "keyslots": ["1"], "tpm2-pin": true,
        "tpm2-pcrs": [], "tpm2_salt": "HsuY4RescHIpV8t0ZFuV8VIlynjVLBhUtmf9jPD2JV0="}"#;
    let options = pin_options(HELP_262, 0);

    let kdf = pin_kdf(&dump_of(fell_back));

    assert_eq!(kdf, PinKdf::Pbkdf2);
    assert!(is_hardened_as_enrolled(0, &options, &kdf));
    let hardening = format!("zfs0 PIN: {kdf}\nzfs1 PIN: pbkdf2\n");
    assert_eq!(
        expected_prompts(ZbmRuntime::Legacy, &hardening),
        ["tpm2pin", "tpm2pin", "gpg"]
    );
}

#[test]
fn legacy_on_an_argon2id_host_expects_the_passphrase_for_zfs0() {
    let hardening = "zfs0 PIN: argon2id (64 MiB, 2, 4)\nzfs1 PIN: pbkdf2\n\
                     zfs2 PIN: argon2id (64 MiB, 2, 4)\nzfs3 PIN: none\n";
    let settings = settings("keys-legacy-prompts");
    let test = Test::new(&settings, "keys-native").unwrap();
    let boot = Boot {
        prompts: ["luks", "tpm2pin", "tpm2pin", "gpg"]
            .map(str::to_owned)
            .to_vec(),
        ..Boot::default()
    };

    let legacy = expected_prompts(ZbmRuntime::Legacy, hardening);
    check_prompts(&test, &boot, legacy);
    check_prompts(&test, &boot, expected_prompts(ZbmRuntime::Rust, hardening));

    assert_eq!(legacy, ["luks", "tpm2pin", "tpm2pin", "gpg"]);
    assert_eq!(
        expected_prompts(ZbmRuntime::Legacy, "zfs0 PIN: pbkdf2\n"),
        ["tpm2pin", "tpm2pin", "gpg"]
    );
    assert_eq!(
        checks(&test),
        [
            "ok   - zfs0's passphrase (its Argon2id PIN refused), wrong PIN, right PIN for zfs1, \
             no prompt for zfs2, GnuPG passphrase: ['luks', 'tpm2pin', 'tpm2pin', 'gpg']",
            "ok   - no fallback to the LUKS passphrase: zfs2 was unlocked by its second token \
             (clevis)",
            "FAIL - wrong PIN, right PIN (reused for zfs1), no prompt for zfs2, GnuPG \
             passphrase: ['luks', 'tpm2pin', 'tpm2pin', 'gpg']",
            "FAIL - no fallback to the LUKS passphrase: zfs2 was unlocked by its second token \
             (clevis)",
        ]
    );
    cleanup(&settings);
}

#[test]
fn a_missing_hardening_file_asks_for_keys_setup() {
    let settings = settings("keys-hardening");
    let test = Test::new(&settings, "keys-native").unwrap();
    let disks = settings.disks(GROUP);
    std::fs::create_dir_all(&disks).unwrap();
    std::fs::write(gpg_file(&settings), "key").unwrap();

    run(&test, "native").unwrap();

    let path = disks.join(HARDENING);
    assert_eq!(
        checks(&test),
        [format!(
            "FAIL - run keys-setup again: it records how the PINs are hardened in {}",
            path.display()
        )]
    );
    let recorded = "zfs0 PIN: pbkdf2\nzfs0 UUID: 5d0c\n";
    std::fs::write(&path, recorded).unwrap();
    assert_eq!(hardening_of(&test, Some("5d0c")), Some(recorded.to_owned()));
    cleanup(&settings);
}

#[test]
fn a_hardening_file_of_other_disks_asks_for_keys_setup() {
    // A `setup` from before the file, run since, left the file of a later
    // one beside disks it made otherwise.
    let settings = settings("keys-hardening-other");
    let test = Test::new(&settings, "keys-native").unwrap();
    let disks = settings.disks(GROUP);
    std::fs::create_dir_all(&disks).unwrap();
    let path = disks.join(HARDENING);
    std::fs::write(
        &path,
        "zfs0 PIN: argon2id (64 MiB, 2, 4)\nzfs0 UUID: 5d0c\n",
    )
    .unwrap();

    assert_eq!(hardening_of(&test, Some("77aa")), None);
    assert_eq!(hardening_of(&test, None), None);
    assert_eq!(hardening(&test), None);

    let other = |uuid: &str| {
        format!(
            "FAIL - run keys-setup again: {} describes other disks than zfs0 ({uuid})",
            path.display()
        )
    };
    assert_eq!(
        checks(&test),
        [
            other("77aa"),
            other("no LUKS volume"),
            other("no LUKS volume")
        ]
    );
    cleanup(&settings);
}
