use pretty_assertions::assert_eq;

use super::setup::{clevis_decrypt, clevis_token, token_types};
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
