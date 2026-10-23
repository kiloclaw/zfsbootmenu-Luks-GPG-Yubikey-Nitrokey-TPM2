//! `runtime-setup`: the pools `tplain` (d0, exported by this host) and
//! `tforeign` (d1, left imported by the host `deadbeef`), each holding the
//! test boot environment as its bootfs.

use std::fmt::Write as _;

use crate::error::Result;
use crate::qemu::Machine;
use crate::record::Test;
use crate::settings::ZbmRuntime;
use crate::vm::Vm;

/// The options of the pools.
const OPTS: &str = "-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 \
                    -O acltype=posixacl -O xattr=sa -m none";

/// The command line of the boot environments.
const BE_CMDLINE: &str = "console=ttyS0 loglevel=4";

/// The host ID of the other host, `deadbeef`, as `/etc/hostid` holds it.
const FOREIGN: [u8; 4] = [0xef, 0xbe, 0xad, 0xde];

/// The pools, in the order of their disks.
const POOLS: [&str; 2] = ["tplain", "tforeign"];

/// Creates the pools in the recovery shell.
///
/// # Errors
///
/// When a step of the test cannot be done.
pub(super) fn run(test: &Test) -> Result<()> {
    let settings = test.settings();
    let payload = super::super::payload(settings)?;
    let image = super::image(settings, "auto", ZbmRuntime::Legacy, None)?;
    let mut disks = super::super::disks(settings, "runtime", 0..2, true)?;
    disks.push(payload);
    let machine = Machine::boot(&image, super::SHELL_APPEND).with_disks(disks);
    let mut vm = Vm::start(test, "setup", &machine)?;
    vm.recovery_shell()?;
    vm.send_functions()?;
    for (index, pool) in POOLS.iter().enumerate() {
        let (rc, _) = vm.sh(&format!(
            "zpool create -f {OPTS} {pool} /dev/disk/by-id/virtio-d{index}"
        ))?;
        test.check(rc == 0, &format!("zpool create {pool}"));
        vm.must(&format!("mkbe {pool}"))?;
        vm.must(&format!(
            "zfs set org.zfsbootmenu:commandline='{BE_CMDLINE}' {pool}/ROOT/be"
        ))?;
    }
    vm.must("finish")?;
    // tforeign belongs to another host and stays imported, as after a crash
    vm.must(&foreign_import())?;
    let (rc, out) = vm.sh_show("od -An -tx4 /etc/hostid; zpool list -H -o name")?;
    test.check(
        rc == 0 && out.contains("deadbeef"),
        "tforeign imported by host deadbeef",
    );
    vm.sh("sync")?;
    vm.poweroff()
}

/// The command that writes the other host's ID and imports `tforeign`.
fn foreign_import() -> String {
    let hostid = FOREIGN.iter().fold(String::new(), |mut out, byte| {
        write!(out, "\\x{byte:02x}").ok();
        out
    });
    format!("printf '{hostid}' > /etc/hostid && zpool import -N tforeign")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_other_host_id_is_written_little_endian() {
        assert_eq!(
            foreign_import(),
            r"printf '\xef\xbe\xad\xde' > /etc/hostid && zpool import -N tforeign"
        );
    }
}
