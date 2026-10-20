#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Main vdev layouts on LUKS, created with ZFS 2.4.4.

  layouts.py main     single disk, 2-way mirror, 3-way mirror, striped
                      mirrors, RAIDZ1 (13 disks; root on the 2-way mirror)
  layouts.py parity   RAIDZ2, RAIDZ3 with all default features, dRAID
                      draid2:3d:1s (15 disks; root on dRAID)

Pool names are the layout names prefixed with "t" (tsingle, tmirror2, ...):
ZFS reserves names that start with mirror, raidz, draid or spare.

Each run creates the pools in ZFSBootMenu's recovery shell (zbm.luks=off),
then boots again with the default automatic unlocking and checks that one
passphrase opens every volume, that every pool is imported with its layout,
that the teardown closes everything, and that the ZFS 2.4.4 boot environment
unlocks, imports, writes to and scrubs every pool.

In the parity group, traidz2 also gets the read-only compatible features
fast_dedup, block_cloning_endian and physical_rewrite activated.

  layouts.py parity-oldzfs
                      boots the parity pools with an image that has an older
                      ZFS (ZBMTEST_OLD_KERNEL, ZBMTEST_OLD_INITRD; skipped
                      when not set): pools whose new features are only enabled
                      import normally, traidz2 imports read-only with a warning,
                      and the ZFS 2.4.4 boot environment still boots
"""

import os

import re
import sys

from zbmtest import (FAST_PBKDF, PW, Test, VM, be_features, check_be, check_teardown,
                     disk, follow_boot, hostid_arg, payload_disk, zbm_image)

OPTS = ("-o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux "
        "-O compression=lz4 -O acltype=posixacl -O xattr=sa -O relatime=on -m none")

GROUPS = {
    "main": {
        "disks": 13,
        "root": "tmirror2",
        "pools": [
            ("tsingle", f"zpool create -f {OPTS} tsingle ${{V[0]}}", ["tsingle "]),
            ("tmirror2", f"zpool create -f {OPTS} tmirror2 mirror ${{V[1]}} ${{V[2]}}", ["mirror-0"]),
            ("tmirror3", f"zpool create -f {OPTS} tmirror3 mirror ${{V[3]}} ${{V[4]}} ${{V[5]}}", ["mirror-0"]),
            ("traid10", f"zpool create -f {OPTS} traid10 mirror ${{V[6]}} ${{V[7]}} mirror ${{V[8]}} ${{V[9]}}",
             ["mirror-0", "mirror-1"]),
            ("traidz1", f"zpool create -f {OPTS} traidz1 raidz1 ${{V[10]}} ${{V[11]}} ${{V[12]}}", ["raidz1-0"]),
        ],
    },
    "parity": {
        "disks": 15,
        "root": "tdraid",
        "pools": [
            ("traidz2", f"zpool create -f {OPTS} traidz2 raidz2 ${{V[@]:0:4}}", ["raidz2-0"]),
            # no compatibility property: every feature ZFS 2.4.4 enables by default
            ("traidz3", "zpool create -f -o ashift=12 -O compression=zstd -O checksum=blake3 -m none "
                       "traidz3 raidz3 ${V[@]:4:5}", ["raidz3-0"]),
            ("tdraid", f"zpool create -f {OPTS} tdraid draid2:3d:1s ${{V[@]:9:6}}",
             ["draid2:3d:6c:1s-0", "draid2-0-0"]),
        ],
    },
}


def main():
    group = sys.argv[1]
    if group.endswith("-oldzfs"):
        oldzfs(group[:-len("-oldzfs")])
    g = GROUPS[group]
    t = Test(f"layouts-{group}")
    n = g["disks"]
    disks = [(disk(group, i, fresh=True), f"d{i}") for i in range(n)]
    payload = (payload_disk(), "payload")
    img = zbm_image("auto")

    # ---------------------------------------------------------------- setup
    vm = VM(t, "setup", disks=disks + [payload], kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    vm.send_functions()
    rc, out = vm.sh("zfs version", show=True)
    t.check("zfs-2.4.4" in out and "zfs-kmod-2.4.4" in out, "ZFSBootMenu runs ZFS 2.4.4 (userland and module)")
    devs = " ".join(f"/dev/disk/by-id/virtio-d{i}" for i in range(n))
    vm.must(f"V=(); U=(); mkluks {PW} '{FAST_PBKDF}' {devs}", t=3600)
    for name, cmd, _ in g["pools"]:
        rc, out = vm.sh(cmd)
        t.check(rc == 0, f"zpool create {name}")
        vm.must(f"zfs create -o mountpoint=/data/{name} {name}/data && "
                f"head -c 8388608 /dev/urandom > /data/{name}/blob && zpool sync {name}")
    if group == "parity":
        # read-only compatible features only (see docs/luks/zfs-versions.rst)
        # deduplicated blocks are cloned through the dedup table, so block
        # cloning needs a dataset without dedup
        vm.must("zfs create -o mountpoint=/data/dd -o dedup=on traidz2/dd && "
                "head -c 4194304 /dev/urandom > /data/dd/a && cp /data/dd/a /data/dd/b && "
                "zfs rewrite -P /data/dd/a && zfs create -o mountpoint=/data/bc traidz2/bc && "
                "head -c 4194304 /dev/urandom > /data/bc/a && cp --reflink=always /data/bc/a /data/bc/clone && "
                "zpool sync traidz2")
        rc, out = vm.sh("features traidz2", show=True)
        for f in ("fast_dedup", "block_cloning", "block_cloning_endian", "physical_rewrite"):
            t.check(f in out.split(), f"traidz2: {f} active")
    root = g["root"]
    vm.must(f"mkbe {root}")
    vm.must(f'becmdline {root}/ROOT/be "${{U[@]}}"')
    rc, out = vm.sh("zpool status -P", show=True)
    for name, _, _ in g["pools"]:
        rc, f = vm.sh(f"features {name}", show=True)
    vm.must("finish")
    vm.poweroff()

    # --------------------------------------------------------------- verify
    vm = VM(t, "boot", disks=disks, kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.timeout=3").start()
    b = follow_boot(vm)
    t.check(b.prompts == ["luks"], f"one passphrase prompt unlocked all {n} volumes (prompts: {b.prompts})")
    pools = b.pools()
    for name, _, vdevs in g["pools"]:
        t.check(pools.get(name) == "ONLINE", f"ZFSBootMenu imported {name}: {pools.get(name)}")
        block = re.search(rf"pool: {name}\n(.*?)(?=\n\s*pool: |\Z)", b.zbm_status, re.S)
        text = block.group(1) if block else ""
        for v in vdevs:
            t.check(v in text, f"{name} has vdev {v.strip()}")
        t.check(len(re.findall(r"/dev/mapper/luks-\S+\s+(ONLINE|AVAIL)", text)) > 0,
                f"{name} vdevs are LUKS mappings")
    check_teardown(t, b)
    check_be(t, b, root, [p for p, _, _ in g["pools"]])
    feats = be_features(b)
    for name, cmd, _ in g["pools"]:
        # zpool create enables it, with or without the openzfs-2.4-linux set;
        # zpool upgrade does not (see procedures.py)
        dgh = feats.get((name, "dynamic_gang_header"))
        t.check(dgh in ("enabled", "active"), f"{name}: dynamic_gang_header {dgh} after zpool create")
    if group == "parity":
        t.check(feats.get(("tdraid", "draid")) == "active", "draid feature active")
        t.check(feats.get(("traidz3", "zstd_compress")) == "active", "zstd_compress active on raidz3")
        t.check(feats.get(("traidz3", "blake3")) == "active", "blake3 active on raidz3")
    t.finish()


def oldzfs(group):
    t = Test(f"layouts-{group}-oldzfs")
    k, i = os.environ.get("ZBMTEST_OLD_KERNEL"), os.environ.get("ZBMTEST_OLD_INITRD")
    if not (k and i):
        t.note("ZBMTEST_OLD_KERNEL/ZBMTEST_OLD_INITRD not set; skipped")
        t.finish()
    g = GROUPS[group]
    disks = [(disk(group, n), f"d{n}") for n in range(g["disks"])]
    vm = VM(t, "oldzfs", disks=disks, kernel=k, initrd=i,
            append=f"console=ttyS0 zbm.timeout=3 {hostid_arg()}").start()
    b = follow_boot(vm)
    if "emergency" in b.prompts:
        # record why nothing could be booted
        vm.shell_ready()
        for cmd in ("zfs version", "zpool list -H -o name,health,readonly", "zpool get -H -o name,value bootfs",
                    "cat /zfsbootmenu/degraded", "zpool import", "zfs list -o name,mountpoint,canmount,mounted",
                    "zpool get -H -o name,property,value all | grep -E 'unsupported@|feature@.*active'"):
            vm.sh(cmd, show=True)
        vm.kill()
    log = open(t.dir / "serial-oldzfs.log").read()
    t.check("nsupported features" in log, "the older ZFS warns about unsupported features")
    pools = b.pools()
    for name, _, _ in g["pools"]:
        t.check(pools.get(name) == "ONLINE", f"older ZFS imported {name}: {pools.get(name)}")
    check_teardown(t, b)
    check_be(t, b, g["root"], [p for p, _, _ in g["pools"]])
    t.finish()


if __name__ == "__main__":
    main()
