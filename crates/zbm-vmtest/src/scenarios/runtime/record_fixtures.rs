//! `runtime-record-fixtures`: not a test. Records what `zfs`, `zpool` and
//! `/proc/self/mounts` print in ZFSBootMenu's recovery shell, as the
//! fixtures of the legacy-compatible commands of `zbm-zfs`
//! (`crates/zbm-zfs/tests/fixtures`, read by `tests/recorded.rs`).
//!
//! The machine gets a pool with boot environments, an inherited and a local
//! `org.zfsbootmenu:commandline`, an encrypted dataset and a snapshot. Each
//! fixture holds the exact standard output of one command, and
//! `commands.txt` lists the command and exit status of each.

use std::fs;
use std::path::Path;

use crate::error::{Error, Result};
use crate::qemu::{Disk, Machine};
use crate::record::Test;
use crate::settings::{ZFSKEY, ZbmRuntime};
use crate::vm::{Sh, Vm};

use super::record_zfs::from_hex;

/// The commands that build the pool `tfix` on the disk `virtio-d0`.
fn setup() -> Vec<String> {
    let mut commands: Vec<String> = [
        "zpool create -f -o ashift=12 -O compression=lz4 -O acltype=posixacl -O xattr=sa \
         -m none tfix /dev/disk/by-id/virtio-d0",
        "zfs create -o mountpoint=none -o canmount=off tfix/ROOT",
        "zfs set org.zfsbootmenu:commandline='quiet loglevel=4' tfix/ROOT",
        "zfs create -o mountpoint=/ -o canmount=noauto tfix/ROOT/be",
        "zfs create -o mountpoint=/ -o canmount=noauto tfix/ROOT/other",
        "zfs set org.zfsbootmenu:commandline='%{parent} rw' tfix/ROOT/other",
        "zfs create -o mountpoint=/child -o canmount=noauto tfix/ROOT/other/child",
    ]
    .map(str::to_owned)
    .to_vec();
    commands.push(format!(
        "echo {ZFSKEY} | zfs create -o encryption=on -o keyformat=passphrase \
         -o mountpoint=/secret -o canmount=noauto tfix/secret"
    ));
    commands.extend(
        [
            "zfs snapshot tfix/ROOT/be@one",
            "zpool set bootfs=tfix/ROOT/be tfix",
            "mkdir -p /mnt/be && mount -o zfsutil -t zfs tfix/ROOT/be /mnt/be",
        ]
        .map(str::to_owned),
    );
    commands
}

/// The recorded commands, by fixture: those of `zbm-zfs` and a few
/// listings.
const RECORD: &[(&str, &str)] = &[
    (
        "zfs-list.txt",
        "zfs list -H -p -o name,mountpoint,canmount,encroot,keystatus,\
         org.zfsbootmenu:commandline",
    ),
    (
        "zfs-list-snapshots.txt",
        "zfs list -H -p -t snapshot -o name,used",
    ),
    ("zfs-list-name.txt", "zfs list -o name -H tfix/ROOT/be"),
    (
        "zfs-list-filesystem.txt",
        "zfs list -H -o name -t filesystem tfix/ROOT/be",
    ),
    (
        "zfs-list-filesystem-snapshot.txt",
        "zfs list -H -o name -t filesystem tfix/ROOT/be@one",
    ),
    (
        "zfs-get-value-local.txt",
        "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT/other",
    ),
    (
        "zfs-get-value-inherited.txt",
        "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT/be",
    ),
    (
        "zfs-get-value-root.txt",
        "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT",
    ),
    (
        "zfs-get-value-child.txt",
        "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT/other/child",
    ),
    (
        "zfs-get-value-unset.txt",
        "zfs get -H -o value org.zfsbootmenu:commandline tfix",
    ),
    (
        "zfs-get-source-inherited.txt",
        "zfs get -H -o source -s inherited org.zfsbootmenu:commandline tfix/ROOT/be",
    ),
    (
        "zfs-get-source-local.txt",
        "zfs get -H -o source -s inherited org.zfsbootmenu:commandline tfix/ROOT/other",
    ),
    (
        "zfs-get-source-child.txt",
        "zfs get -H -o source -s inherited org.zfsbootmenu:commandline \
         tfix/ROOT/other/child",
    ),
    ("zpool-list-name.txt", "zpool list -H -o name"),
    (
        "zpool-list.txt",
        "zpool list -H -p -o name,health,size,allocated",
    ),
    ("zpool-get-bootfs.txt", "zpool get -H -o value bootfs tfix"),
    ("mounts.txt", "cat /proc/self/mounts"),
];

/// The output and exit status of a command run by [`recorded_command`].
fn parse_recorded(out: &str) -> Option<(Vec<u8>, String)> {
    let (hex, status) = out.rsplit_once("STATUS=")?;
    Some((from_hex(hex)?, status.trim().to_owned()))
}

/// `command` with its output in hexadecimal, which survives the serial
/// console byte for byte, and its exit status.
fn recorded_command(command: &str) -> String {
    format!(
        "{command} 2>/dev/null > /tmp/fixture; status=$?; od -An -v -tx1 /tmp/fixture; \
         echo STATUS=$status"
    )
}

/// `commands.txt`: the versions, then each fixture's command and status.
fn listing(version: &str, statuses: &[(&str, &str, String)]) -> String {
    let versions: Vec<&str> = version.split_whitespace().collect();
    let mut text = format!(
        "Recorded with {} by zbm-vmtest run runtime-record-fixtures\n\n",
        versions.join(" ")
    );
    for (name, command, status) in statuses {
        text.push_str(&format!("{name} (exit status {status}): {command}\n"));
    }
    text
}

/// Records the fixtures.
///
/// # Errors
///
/// When a step cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let image = super::image(settings, "auto", ZbmRuntime::Legacy, None)?;
    let disk = crate::disks::disk(settings, "fixtures", 0, "384M", true)?;
    let machine = Machine::boot(&image, super::SHELL_APPEND).with_disks([Disk::new(disk, "d0")]);
    let mut vm = Vm::start(test, "record", &machine)?;
    vm.recovery_shell()?;
    let (_, version) = vm.sh_show("zfs version")?;
    test.check(version.contains("zfs-2.4.4"), "ZFSBootMenu runs ZFS 2.4.4");
    for command in setup() {
        vm.must(&command)?;
    }
    let target = settings.repo.join("crates/zbm-zfs/tests/fixtures");
    fs::create_dir_all(&target).map_err(Error::io(format!("creating {}", target.display())))?;
    let mut statuses = Vec::new();
    for &(name, command) in RECORD {
        let (_, out) = vm.sh_with(&recorded_command(command), Sh::default().quiet())?;
        let (data, status) = parse_recorded(&out)
            .ok_or_else(|| Error::Setup(format!("{name}: unreadable output: {out}")))?;
        write(&target.join(name), &data)?;
        statuses.push((name, command, status));
    }
    write(
        &target.join("commands.txt"),
        listing(&version, &statuses).as_bytes(),
    )?;
    test.check(
        true,
        &format!("recorded {} fixtures in {}", RECORD.len(), target.display()),
    );
    vm.poweroff()
}

/// Writes a fixture.
fn write(path: &Path, data: &[u8]) -> Result<()> {
    fs::write(path, data).map_err(Error::io(format!("writing {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn output_and_status_are_read_back() {
        let out = " 74 66 69 78 0a\nSTATUS=0";

        assert_eq!(
            parse_recorded(out),
            Some((b"tfix\n".to_vec(), "0".to_owned()))
        );
        assert_eq!(
            parse_recorded("\nSTATUS=1"),
            Some((Vec::new(), "1".to_owned()))
        );
        assert_eq!(parse_recorded("74 6"), None);
    }

    #[test]
    fn the_listing_names_the_versions_and_each_command() {
        let statuses = [(
            "zpool-list-name.txt",
            "zpool list -H -o name",
            "0".to_owned(),
        )];

        let text = listing("zfs-2.4.4-1\nzfs-kmod-2.4.4-1\n", &statuses);

        assert_eq!(
            text,
            "Recorded with zfs-2.4.4-1 zfs-kmod-2.4.4-1 by zbm-vmtest run \
             runtime-record-fixtures\n\nzpool-list-name.txt (exit status 0): \
             zpool list -H -o name\n"
        );
        assert!(recorded_command("zfs list").starts_with("zfs list 2>/dev/null"));
        assert_eq!(setup().len(), 11);
    }
}
