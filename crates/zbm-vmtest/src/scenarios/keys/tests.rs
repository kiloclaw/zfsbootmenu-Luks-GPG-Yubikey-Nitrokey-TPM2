use pretty_assertions::assert_eq;

use super::setup::token_types;
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
