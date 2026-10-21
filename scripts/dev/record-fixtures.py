#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Records the output of zfs, zpool and /proc/self/mounts of OpenZFS 2.4.4 as
fixtures for the Rust unit tests, so that the parsers are tested against real
output and not only against examples written by hand.

The commands run in ZFSBootMenu's recovery shell in a QEMU virtual machine,
booted from an image that the build container builds from this tree (Linux
7.3, OpenZFS 2.4.4), with the harness of the VM tests in tests/luks/vm: see
docs/luks/testing.rst for its requirements and environment variables. The
machine gets a pool with boot environments, an inherited and a local
org.zfsbootmenu:commandline, an encrypted dataset and a snapshot.

Each fixture in crates/zbm-zfs/tests/fixtures holds the exact standard output
of one command, and commands.txt lists the command and exit status of each. Output travels over
the serial console in base64, so tabs and trailing newlines arrive unchanged.

Usage: scripts/dev/record-fixtures.py
"""

import base64
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tests" / "luks" / "vm"))

from zbmtest import ZFSKEY, Test, VM, disk, zbm_image  # noqa: E402

FIXTURES = REPO / "crates" / "zbm-zfs" / "tests" / "fixtures"

SETUP = [
    "zpool create -f -o ashift=12 -O compression=lz4 -O acltype=posixacl -O xattr=sa "
    "-m none tfix /dev/disk/by-id/virtio-d0",
    "zfs create -o mountpoint=none -o canmount=off tfix/ROOT",
    "zfs set org.zfsbootmenu:commandline='quiet loglevel=4' tfix/ROOT",
    "zfs create -o mountpoint=/ -o canmount=noauto tfix/ROOT/be",
    "zfs create -o mountpoint=/ -o canmount=noauto tfix/ROOT/other",
    "zfs set org.zfsbootmenu:commandline='%{parent} rw' tfix/ROOT/other",
    "zfs create -o mountpoint=/child -o canmount=noauto tfix/ROOT/other/child",
    f"echo {ZFSKEY} | zfs create -o encryption=on -o keyformat=passphrase "
    "-o mountpoint=/secret -o canmount=noauto tfix/secret",
    "zfs snapshot tfix/ROOT/be@one",
    "zpool set bootfs=tfix/ROOT/be tfix",
    "mkdir -p /mnt/be && mount -o zfsutil -t zfs tfix/ROOT/be /mnt/be",
]

# The commands of zbm-zfs (crates/zbm-zfs/src/lib.rs) and a few listings
RECORD = {
    "zfs-list.txt": "zfs list -H -p -o name,mountpoint,canmount,encroot,keystatus,"
                    "org.zfsbootmenu:commandline",
    "zfs-list-snapshots.txt": "zfs list -H -p -t snapshot -o name,used",
    "zfs-list-name.txt": "zfs list -o name -H tfix/ROOT/be",
    "zfs-list-filesystem.txt": "zfs list -H -o name -t filesystem tfix/ROOT/be",
    "zfs-list-filesystem-snapshot.txt": "zfs list -H -o name -t filesystem tfix/ROOT/be@one",
    "zfs-get-value-local.txt": "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT/other",
    "zfs-get-value-inherited.txt": "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT/be",
    "zfs-get-value-root.txt": "zfs get -H -o value org.zfsbootmenu:commandline tfix/ROOT",
    "zfs-get-value-child.txt": "zfs get -H -o value org.zfsbootmenu:commandline "
                               "tfix/ROOT/other/child",
    "zfs-get-value-unset.txt": "zfs get -H -o value org.zfsbootmenu:commandline tfix",
    "zfs-get-source-inherited.txt": "zfs get -H -o source -s inherited "
                                    "org.zfsbootmenu:commandline tfix/ROOT/be",
    "zfs-get-source-local.txt": "zfs get -H -o source -s inherited "
                                "org.zfsbootmenu:commandline tfix/ROOT/other",
    "zfs-get-source-child.txt": "zfs get -H -o source -s inherited "
                                "org.zfsbootmenu:commandline tfix/ROOT/other/child",
    "zpool-list-name.txt": "zpool list -H -o name",
    "zpool-list.txt": "zpool list -H -p -o name,health,size,allocated",
    "zpool-get-bootfs.txt": "zpool get -H -o value bootfs tfix",
    "mounts.txt": "cat /proc/self/mounts",
}


def main():
    t = Test("record-fixtures")
    img = zbm_image("auto")
    vm = VM(t, "record", disks=[(disk("fixtures", 0, fresh=True), "d0")],
            kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    rc, version = vm.sh("zfs version", show=True)
    t.check("zfs-2.4.4" in version, "ZFSBootMenu runs ZFS 2.4.4")
    for command in SETUP:
        vm.must(command)

    FIXTURES.mkdir(parents=True, exist_ok=True)
    listing = []
    for name, command in RECORD.items():
        rc, out = vm.sh(f"{command} 2>/dev/null > /tmp/fixture; status=$?; "
                        "base64 -w0 /tmp/fixture; echo; echo STATUS=$status")
        encoded, _, status = out.rpartition("STATUS=")
        (FIXTURES / name).write_bytes(base64.b64decode("".join(encoded.split())))
        listing.append(f"{name} (exit status {status.strip()}): {command}")
    versions = " ".join(line.strip() for line in version.splitlines() if line.strip())
    (FIXTURES / "commands.txt").write_text(
        f"Recorded with {versions} by scripts/dev/record-fixtures.py\n\n" + "\n".join(listing) + "\n")
    t.check(True, f"recorded {len(RECORD)} fixtures in {FIXTURES}")
    vm.poweroff()
    t.finish()


if __name__ == "__main__":
    main()
