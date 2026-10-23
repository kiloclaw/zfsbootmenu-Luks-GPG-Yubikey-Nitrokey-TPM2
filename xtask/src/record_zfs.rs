//! `cargo xtask record-zfs`: recording what `zfs` and `zpool` print, for the
//! fixture tests of `zbm-zfs`.
//!
//! The recording runs in a virtual machine of the VM test harness, whose
//! ZFSBootMenu image has the ZFS of the build container: the scenario
//! `runtime-record-zfs` of `zbm-vmtest` boots it to the recovery shell,
//! asks this command for the [`manifest`]
//! (`--manifest`), runs the setup commands on an empty disk, runs each
//! recorded command and saves its standard output in
//! `crates/zbm-zfs/tests/fixtures/<major.minor>/<name>`.
//! The commands are defined here, in Rust; the harness only runs them.
//!
//! The pool `fixture` has what ZFSBootMenu looks at: boot environments below
//! `ROOT` with a default (`bootfs`), user properties, snapshots, a dataset that
//! is not a boot environment, and an encryption root whose key is not loaded.
//! Each listing is recorded as JSON (`-j -p`, OpenZFS 2.3 and later; without
//! `-p` values such as `creation` are written for people) and as the `-H -p`
//! text that older versions print, so that both parsers are tested against
//! the same pool.

use serde_json::json;

/// The commands that build the pool `fixture` on the disk `virtio-d0`.
pub const SETUP: &[&str] = &[
    "zpool create -f -o ashift=12 -O compression=lz4 -O mountpoint=none -O canmount=off \
     fixture /dev/disk/by-id/virtio-d0",
    "zfs create -o mountpoint=none fixture/ROOT",
    "zfs set org.zfsbootmenu:commandline='quiet loglevel=4' fixture/ROOT",
    "zfs create -o mountpoint=/ -o canmount=noauto fixture/ROOT/void",
    "zfs create -o mountpoint=/ -o canmount=noauto fixture/ROOT/arch",
    "zfs set org.zfsbootmenu:kernel=6.1 fixture/ROOT/arch",
    "zfs snapshot fixture/ROOT/void@first",
    "zfs snapshot fixture/ROOT/void@second",
    "zfs create -o mountpoint=/home fixture/home",
    "printf 'fixturekey\\n' | zfs create -o encryption=on -o keyformat=passphrase \
     -o keylocation=prompt -o mountpoint=/ -o canmount=noauto fixture/ROOT/secret",
    "zfs unload-key fixture/ROOT/secret",
    "zpool set bootfs=fixture/ROOT/void fixture",
];

/// The properties of datasets that ZFSBootMenu reads, in the order of the
/// recorded listings.
pub const DATASET_PROPERTIES: &str = "name,mountpoint,canmount,encryptionroot,keystatus,\
                                      keylocation,keyformat,org.zfsbootmenu:kernel,\
                                      org.zfsbootmenu:commandline";

/// The properties of pools that ZFSBootMenu reads.
pub const POOL_PROPERTIES: &str = "name,health,readonly,bootfs,guid";

/// The listings recorded in both formats, as (file stem, command with `{}`
/// where the format options go).
const LISTINGS: &[(&str, &str)] = &[
    ("pools", "zpool list {} -o name,health,readonly,bootfs,guid"),
    (
        "pool-properties",
        "zpool get {} -o name,property,value,source readonly,bootfs,guid fixture",
    ),
    (
        "filesystems",
        "zfs list {} -t filesystem -o {DATASETS} -r fixture",
    ),
    (
        "snapshots",
        "zfs list {} -t snapshot -o name,creation -r fixture",
    ),
    (
        "properties",
        "zfs get {} -o name,property,value,source \
         mountpoint,canmount,org.zfsbootmenu:commandline fixture/ROOT/void",
    ),
];

/// Each recorded command, as (file name, command). A name ending in `.err`
/// records standard error and standard output together.
pub fn records() -> Vec<(String, String)> {
    let mut records = vec![
        ("version.txt".to_owned(), "zfs version".to_owned()),
        ("version.json".to_owned(), "zfs version -j".to_owned()),
        (
            "status.json".to_owned(),
            "zpool status -j -P fixture".to_owned(),
        ),
        (
            "status.txt".to_owned(),
            "zpool status -P fixture".to_owned(),
        ),
    ];
    for (stem, command) in LISTINGS {
        let command = command.replace("{DATASETS}", DATASET_PROPERTIES);
        for (extension, options) in [("json", "-j -p"), ("txt", "-H -p")] {
            records.push((
                format!("{stem}.{extension}"),
                command.replace("{}", options),
            ));
        }
    }
    let pools = format!("zpool list {{}} -o {POOL_PROPERTIES}");
    records.extend([
        (
            "missing.json.err".to_owned(),
            "zfs list -j -p fixture/none 2>&1".to_owned(),
        ),
        (
            "missing.txt.err".to_owned(),
            "zfs list -H fixture/none 2>&1".to_owned(),
        ),
        (
            "exported.txt".to_owned(),
            "zpool export fixture && zpool import".to_owned(),
        ),
        (
            "readonly-pools.json".to_owned(),
            format!(
                "zpool import -N -o readonly=on fixture && {}",
                pools.replace("{}", "-j -p")
            ),
        ),
        (
            "readonly-pools.txt".to_owned(),
            pools.replace("{}", "-H -p"),
        ),
    ]);
    records
}

/// The commands that end the recording; the disk is created afresh each time.
pub const TEARDOWN: &[&str] = &["zpool export fixture"];

/// The manifest that the harness runs, as JSON: the lists `setup` and
/// `teardown` of commands, and `records`, each with a `file` and a `command`.
pub fn manifest() -> String {
    let records: Vec<_> = records()
        .into_iter()
        .map(|(file, command)| json!({ "file": file, "command": command }))
        .collect();
    let value = json!({
        "setup": SETUP,
        "records": records,
        "teardown": TEARDOWN,
    });
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_lists_every_command_once() {
        let value: serde_json::Value = serde_json::from_str(&manifest()).unwrap();
        let listed = value["records"].as_array().unwrap();
        assert_eq!(listed.len(), records().len());
        let mut files: Vec<_> = listed.iter().map(|r| r["file"].as_str().unwrap()).collect();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), listed.len());
        assert_eq!(value["setup"].as_array().unwrap().len(), SETUP.len());
        assert!(value["teardown"][0].as_str().unwrap().contains("export"));
    }

    #[test]
    fn json_and_text_listings_come_in_pairs() {
        let files: Vec<_> = records().into_iter().map(|(file, _)| file).collect();
        for file in &files {
            if let Some(stem) = file.strip_suffix(".json") {
                assert!(files.contains(&format!("{stem}.txt")), "{file}");
            }
        }
        let commands: Vec<_> = records().into_iter().map(|(_, command)| command).collect();
        assert!(
            commands.contains(&"zpool list -j -p -o name,health,readonly,bootfs,guid".to_owned())
        );
        assert!(commands.iter().all(|command| !command.contains('{')));
    }
}
