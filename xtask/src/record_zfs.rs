//! `cargo xtask record-zfs`: recording what `zfs` and `zpool` print, for the
//! fixture tests of `zbm-zfs`.
//!
//! The recording runs in a virtual machine of the VM test harness, whose
//! ZFSBootMenu image has the ZFS of the build container: the scenario
//! `runtime.py record-zfs` (`run-all.sh runtime-record-zfs`) boots it to
//! the recovery shell, asks this command for the [`manifest`]
//! (`--manifest`), runs the setup commands on an empty disk, runs each
//! recorded command and saves its standard output in
//! `crates/zbm-zfs/tests/fixtures/<major.minor>/<name>`.
//! The commands are defined here, in Rust; the harness only runs them.
//!
//! The pool `fixture` has what ZFSBootMenu looks at: boot environments below
//! `ROOT` with a default (`bootfs`), user properties, snapshots, a dataset that
//! is not a boot environment, and an encryption root whose key is not loaded.
//! Each listing is recorded as JSON (`-j`, OpenZFS 2.3 and later) and as the
//! `-H -p` text that older versions print, so that both parsers are tested
//! against the same pool.

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

/// Each recorded command, as (file name, command). A name ending in `.err`
/// records standard error and standard output together.
pub fn records() -> Vec<(&'static str, String)> {
    let filesystems = format!("-t filesystem -o {DATASET_PROPERTIES} -r fixture");
    vec![
        ("version.txt", "zfs version".to_owned()),
        ("version.json", "zfs version -j".to_owned()),
        ("pools.json", format!("zpool list -j -o {POOL_PROPERTIES}")),
        (
            "pools.txt",
            format!("zpool list -H -p -o {POOL_PROPERTIES}"),
        ),
        (
            "pool-properties.json",
            "zpool get -j -o name,property,value,source readonly,bootfs,guid fixture".to_owned(),
        ),
        (
            "pool-properties.txt",
            "zpool get -H -p -o name,property,value,source readonly,bootfs,guid fixture".to_owned(),
        ),
        ("status.json", "zpool status -j -P fixture".to_owned()),
        ("status.txt", "zpool status -P fixture".to_owned()),
        ("filesystems.json", format!("zfs list -j {filesystems}")),
        ("filesystems.txt", format!("zfs list -H -p {filesystems}")),
        (
            "snapshots.json",
            "zfs list -j -t snapshot -o name,creation -r fixture".to_owned(),
        ),
        (
            "snapshots.txt",
            "zfs list -H -p -t snapshot -o name,creation -r fixture".to_owned(),
        ),
        (
            "properties.json",
            "zfs get -j -o name,property,value,source \
             mountpoint,canmount,org.zfsbootmenu:commandline fixture/ROOT/void"
                .to_owned(),
        ),
        (
            "properties.txt",
            "zfs get -H -p -o name,property,value,source \
             mountpoint,canmount,org.zfsbootmenu:commandline fixture/ROOT/void"
                .to_owned(),
        ),
        (
            "missing.json.err",
            "zfs list -j fixture/none 2>&1".to_owned(),
        ),
        (
            "missing.txt.err",
            "zfs list -H fixture/none 2>&1".to_owned(),
        ),
        (
            "exported.txt",
            "zpool export fixture && zpool import".to_owned(),
        ),
        (
            "readonly-pools.json",
            format!("zpool import -N -o readonly=on fixture && zpool list -j -o {POOL_PROPERTIES}"),
        ),
        (
            "readonly-pools.txt",
            format!("zpool list -H -p -o {POOL_PROPERTIES}"),
        ),
    ]
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
                assert!(files.contains(&format!("{stem}.txt").as_str()), "{file}");
            }
        }
    }
}
