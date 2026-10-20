#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
The procedures of docs/luks/vdev-configurations.rst and the pool sections of
docs/luks/administration.rst, run as written with ZFS 2.4.4.

The commands come from the documentation itself (see docs.py); the test only
supplies devices for the placeholder names (symbolic links in /dev/disk/by-id),
answers cryptsetup's passphrase prompts, and fills in values the documentation
cannot know, such as the UUIDs it abbreviates as "luks-5d0c3a1e-...". The LUKS
volumes use cryptsetup's default PBKDF, as in the documentation.

Disks: ata-DISK1..7 partitioned (esp, zfs), ata-DISK1..7 whole (for dRAID),
nvme-FAST1..3 with partitions 2-5 (special, log, cache, dedup), ata-NEWDISK and
nvme-SSD1..2 partitioned.

  procedures.py         run the procedures, then reboot and check the
                        documented verification step
"""

import re

from docs import commands
from zbmtest import (REPO, Test, VM, check_teardown, disk, follow_boot, partition, payload_disk,
                     zbm_image)

VDEV = REPO / "docs" / "luks" / "vdev-configurations.rst"
ADMIN = REPO / "docs" / "luks" / "administration.rst"

# placeholder name -> (disk serial, partition layout or None)
PARTED = [("esp", "64M"), ("zfs", None)]
FAST = [("esp", "32M"), ("special", "72M"), ("log", "72M"), ("cache", "72M"), ("dedup", None)]
DISKS = ([(f"ata-DISK{i}", f"w{i}", PARTED) for i in range(1, 8)]
         + [(f"ata-DISK{i}", f"x{i}", None) for i in range(1, 8)]
         + [(f"nvme-FAST{i}", f"f{i}", FAST) for i in range(1, 4)]
         + [("ata-NEWDISK", "n1", PARTED)]
         + [(f"nvme-SSD{i}", f"s{i}", PARTED) for i in range(1, 3)])


def links():
    """ln commands mapping placeholder names to the virtual disks."""
    cmds = []
    for name, serial, parts in DISKS:
        if parts is None:
            cmds.append(f"ln -sf /dev/disk/by-id/virtio-{serial} /dev/disk/by-id/{name}")
        else:
            for n in range(1, len(parts) + 1):
                cmds.append(f"ln -sf /dev/disk/by-id/virtio-{serial}-part{n} /dev/disk/by-id/{name}-part{n}")
    return cmds


class Run:
    def __init__(self, t, vm):
        self.t, self.vm = t, vm

    def doc(self, page, section, skip=(), subst=(), expect_rc=0):
        """Run the commands of a documentation section; returns their output."""
        out = []
        cmds = commands(page, section)
        self.t.step(f"--- {page.name}: {section} ({len(cmds)} commands)")
        for cmd in cmds:
            if any(re.search(p, cmd) for p in skip):
                self.t.step(f"  (not run here: {cmd.splitlines()[0][:90]})")
                continue
            for a, b in subst:
                cmd = cmd.replace(a, b)
            rc, o = self.vm.sh(cmd, t=3600)
            out.append(o)
            if rc != expect_rc:
                self.t.show(o)
                self.t.check(False, f"{section}: '{cmd.splitlines()[0][:100]}' exited {rc}")
                return "\n".join(out)
        self.t.check(True, f"{section}: all commands succeeded")
        return "\n".join(out)

    def status(self, pool):
        rc, out = self.vm.sh(f"zpool status -P {pool}")
        return out

    def reset(self, pool=None):
        """Destroy the pool and close every LUKS mapping (between examples)."""
        if pool:
            self.vm.sh(f"zpool destroy {pool}")
        self.vm.sh("for m in /dev/mapper/luks-*; do [ -e \"$m\" ] && cryptsetup close \"${m##*/}\"; done; true")


NEW_IN_24 = ("block_cloning_endian", "physical_rewrite", "dynamic_gang_header")


def feature_states(vm, pool):
    """{feature: state} of a pool."""
    rc, out = vm.sh(f"zpool get -H -o property,value all {pool} | grep feature@", quiet=True)
    return dict(l.replace("feature@", "").split()[:2] for l in out.splitlines() if l.startswith("feature@"))


def upgrade_pool(t, vm, r):
    """
    Two pools created with compatibility=openzfs-2.3-linux on LUKS volumes:
    zroot is upgraded with the commands of "Upgrading a Pool", zoff with the
    compatibility property turned off, which leaves dynamic_gang_header to an
    explicit zpool set.
    """
    vm.must("V=(); U=(); mkluks pw \"--pbkdf argon2id --pbkdf-memory 65536 --iter-time 500\" "
            "/dev/disk/by-id/ata-DISK1-part2 /dev/disk/by-id/ata-DISK2-part2")
    for pool, n in (("zroot", 0), ("zoff", 1)):
        vm.must(f"zpool create -f -o ashift=12 -o compatibility=openzfs-2.3-linux -m none {pool} \"${{V[{n}]}}\"")
        f = feature_states(vm, pool)
        t.check(all(f.get(x) == "disabled" for x in NEW_IN_24),
                f"{pool}: openzfs-2.3-linux leaves the 2.4 features disabled ({[f.get(x) for x in NEW_IN_24]})")
        t.check(f.get("raidz_expansion") == "enabled" and f.get("fast_dedup") == "enabled",
                f"{pool}: openzfs-2.3-linux enables the 2.3 features")

    vm.sh("zpool upgrade zroot", show=True)
    t.check(feature_states(vm, "zroot").get("block_cloning_endian") == "disabled",
            "zpool upgrade keeps to the compatibility set")
    out = r.doc(REPO / "docs" / "luks" / "zfs-versions.rst", "Upgrading a Pool")
    f = feature_states(vm, "zroot")
    t.check(all(f.get(x) == "enabled" for x in NEW_IN_24),
            f"Upgrading a Pool: the 2.4 features are enabled ({[f.get(x) for x in NEW_IN_24]})")
    t.check("feature@dynamic_gang_header" in out, "Upgrading a Pool: the check lists dynamic_gang_header")

    vm.sh("zpool set compatibility=off zoff && zpool upgrade zoff", show=True)
    f = feature_states(vm, "zoff")
    t.check(f.get("block_cloning_endian") == "enabled" and f.get("physical_rewrite") == "enabled",
            "compatibility=off: zpool upgrade enables block_cloning_endian and physical_rewrite")
    t.check(f.get("dynamic_gang_header") == "disabled",
            f"compatibility=off: zpool upgrade leaves dynamic_gang_header {f.get('dynamic_gang_header')}"
            " (documented: disabled)")
    vm.sh("zpool set feature@dynamic_gang_header=enabled zoff", show=True)
    t.check(feature_states(vm, "zoff").get("dynamic_gang_header") == "enabled",
            "zpool set feature@dynamic_gang_header=enabled enables it")
    vm.sh("zpool destroy zoff")
    r.reset("zroot")


def main():
    t = Test("procedures")
    vdisks = []
    for name, serial, parts in DISKS:
        d = disk("procedures", serial, fresh=True)
        if parts:
            partition(d, parts)
        vdisks.append((d, serial))
    img = zbm_image("auto")
    vm = VM(t, "procedures", disks=vdisks + [(payload_disk(), "payload")], kernel=img["kernel"],
            initrd=img["initrd"], append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    vm.send_functions()
    vm.must("; ".join(links()))
    r = Run(t, vm)

    # Conventions and preparation
    r.doc(VDEV, "Conventions")
    rc, out = vm.sh('printf "%s\\n" "${POOL_OPTS[@]}" | grep compatibility', show=True)
    t.check("openzfs-2.4-linux" in out, "POOL_OPTS uses the OpenZFS 2.4 compatibility set")
    r.doc(VDEV, "Step 2: Define a helper that creates and opens LUKS volumes")
    out = r.doc(VDEV, "Step 3: Create the LUKS volumes")
    t.check(len(re.findall(r"^/dev/mapper/luks-[0-9a-f-]{36}$", out, re.M)) == 2, "VDEVS holds two luks-<UUID> devices")
    rc, out = vm.sh("cryptsetup luksDump /dev/disk/by-id/ata-DISK1-part2 | grep -E '^Flags|PBKDF'", show=True)
    t.check("allow-discards" in out, "--allow-discards --persistent stored the TRIM flag")
    t.check("argon2id" in out, "default PBKDF argon2id")
    r.reset()

    # Data vdev layouts
    layouts = [("Example 1: Single disk", "zroot", []),
               ("Example 2: Two-way mirror", "zroot", ["mirror-0"]),
               ("Example 3: Three-way mirror", "zroot", ["mirror-0"]),
               ("Example 4: Striped mirrors", "zroot", ["mirror-0", "mirror-1"]),
               ("Example 5: RAIDZ", "tank", ["raidz2-0"]),
               ("Example 6: dRAID", "tank", ["draid2:4d:7c:1s-0", "draid2-0-0"])]
    for section, pool, vdevs in layouts:
        r.doc(VDEV, section)
        st = r.status(pool)
        t.check(f"state: ONLINE" in st and all(v in st for v in vdevs),
                f"{section}: {pool} ONLINE with {', '.join(vdevs) or 'a single disk'}")
        rc, f = vm.sh(f"zpool get -H -o value compatibility {pool}")
        t.check("openzfs-2.4-linux" in f, f"{section}: compatibility openzfs-2.4-linux")
        r.reset(pool)

    # Auxiliary vdevs on the RAIDZ2 pool of Example 5
    r.doc(VDEV, "Example 5: RAIDZ")
    rc, out = vm.sh('VDEVS=(); luks_create /dev/disk/by-id/nvme-FAST{1,2,3}-part2 && '
                    'zpool add tank special mirror "${VDEVS[@]}"')
    t.check(rc != 0 and "mismatched replication level" in out,
            "without -f, zpool add refuses a special mirror on RAIDZ (as documented)")
    vm.sh('for v in "${VDEVS[@]}"; do cryptsetup close "${v##*/}"; done')
    r.doc(VDEV, "Example 7: Special allocation class")
    r.doc(VDEV, "Example 8: Log (SLOG)")
    r.doc(VDEV, "Example 9: Cache (L2ARC)")
    r.doc(VDEV, "Example 10: Hot spare")
    st = r.status("tank")
    for sect in ("special", "dedup", "logs", "cache", "spares"):
        t.check(re.search(rf"^\s*{sect}\s*$", st, re.M) is not None, f"tank has its {sect} vdev")

    # Administration on that pool
    r.doc(ADMIN, "Administration")
    rc, out = vm.sh("pool_luks_devices tank | sort | tr '\\n' ' '", show=True)
    t.check(len(out.split()) == 16, f"pool_luks_devices lists the 16 LUKS devices behind tank ({len(out.split())})")
    cache = vm.sh("zpool status -P tank | awk '/^[[:space:]]*cache[[:space:]]*$/ { getline; print $1 }'")[1].strip()
    r.doc(ADMIN, "Removing an auxiliary vdev",
          subst=[("/dev/mapper/luks-9a72f853-...", cache),
                 ("luks-9a72f853-...", cache.replace("/dev/mapper/", ""))])
    t.check("cache" not in r.status("tank").split("config:")[-1], "cache vdev removed and its volume closed")
    # a failed disk: offline the sixth RAIDZ member, then the documented replacement
    failed = vm.sh("zpool status -P tank | awk '/raidz2-0/ { for (i = 0; i < 6; i++) { getline; d = $1 } print d; exit }'")[1].strip()
    vm.must(f"zpool offline tank {failed}")
    t.check("DEGRADED" in r.status("tank"), "tank DEGRADED after a member went offline")
    out = r.doc(ADMIN, "Replacing a failed disk",
                skip=[r"^zpool status"],
                subst=[("zpool replace zroot 12345678901234567890", f"zpool replace tank {failed}"),
                       ("zpool wait -t resilver zroot", "zpool wait -t resilver tank")])
    st = r.status("tank")
    t.check("DEGRADED" not in st and "state: ONLINE" in st, "replacement resilvered; tank ONLINE")
    r.reset("tank")

    # RAIDZ expansion (OpenZFS 2.3 and later)
    r.doc(VDEV, "Example 5: RAIDZ")
    r.doc(ADMIN, "Expanding a RAIDZ vdev")
    st = r.status("tank")
    members = len(re.findall(r"/dev/mapper/luks-\S+\s+ONLINE", st))
    t.check(members == 7, f"raidz2-0 has 7 members after expansion ({members})")
    rc, f = vm.sh("zpool get -H -o value feature@raidz_expansion tank")
    t.check("active" in f, "feature raidz_expansion active")
    r.reset("tank")

    # Single disk -> mirror -> striped mirrors
    r.doc(VDEV, "Example 1: Single disk")
    first = vm.sh("zpool status -P zroot | awk '$1 ~ \"^/dev/mapper/\" { print $1; exit }'")[1].strip()
    r.doc(ADMIN, "Turning a single disk into a mirror", subst=[("/dev/mapper/luks-5d0c3a1e-...", first)])
    r.doc(ADMIN, "Adding a vdev")
    st = r.status("zroot")
    t.check("mirror-0" in st and "mirror-1" in st and "state: ONLINE" in st,
            "single disk -> mirror (attach) -> striped mirrors (add)")
    r.reset("zroot")

    # Example 12: NVMe mirror without a crypttab (the nvme-cli and installed-system steps are not run here)
    r.doc(VDEV, "Example 12: A mirror of two NVMe SSDs, unlocked without a crypttab",
          skip=[r"^nvme ", r"^zfs set org\.zfsbootmenu", r"^install ", r"^dd ", r"^cat /etc/dracut",
                r"cryptsetup luksAddKey"])
    rc, out = vm.sh("cryptsetup luksDump /dev/disk/by-id/nvme-SSD1-part2 | grep -E '^Flags|sector'", show=True)
    t.check("4096" in out and "no-read-workqueue" in out and "allow-discards" in out,
            "Example 12: 4096-byte sectors and flags stored in the header")
    t.check("mirror-0" in r.status("zroot"), "Example 12: NVMe mirror created")
    r.reset("zroot")

    # Upgrading a pool from the OpenZFS 2.3 compatibility set (zfs-versions.rst)
    upgrade_pool(t, vm, r)

    # Example 11 and making the pool bootable
    r.doc(VDEV, "Example 11: A complete layout")
    r.doc(VDEV, "Step 1: Create a boot environment")
    out = r.doc(VDEV, "Step 2: Let the installed system unlock the volumes")
    rc, ct = vm.sh("cat /etc/crypttab", show=True)
    t.check(len(re.findall(r"^luks-[0-9a-f-]{36} UUID=[0-9a-f-]{36} none luks,discard$", ct, re.M)) == 11,
            "crypttab loop wrote 11 valid entries")
    vm.must("mkdir -p /be /pl; grep -q ' /pl ' /proc/mounts || mount -o ro /dev/disk/by-label/PAYLOAD /pl")
    vm.must("mount -t zfs -o zfsutil zroot/ROOT/os /be && cp -a /pl/rootfs/. /be/ && mkdir -p /be/boot && "
            "cp /pl/boot/vmlinuz-* /pl/boot/initramfs-* /be/boot/ && umount /be")
    vm.must("finish")
    vm.poweroff()

    # Step 4: verify after a reboot, with automatic unlocking
    vm = VM(t, "verify", disks=vdisks, kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.timeout=3").start()
    b = follow_boot(vm, be=False)
    t.check(b.prompts == ["luks"], f"one passphrase prompt for the 11 volumes ({b.prompts})")
    st = b.zbm_status
    t.check(b.pools().get("zroot") == "ONLINE", "Step 4: zroot ONLINE")
    t.check(re.search(r"cache\s*\n\s*/dev/mapper/luks-\S+\s+ONLINE", st) is not None, "Step 4: cache ONLINE")
    t.check(re.search(r"spares\s*\n\s*/dev/mapper/luks-\S+\s+AVAIL", st) is not None, "Step 4: spare AVAIL")
    t.check("UNAVAIL" not in st and "DEGRADED" not in st, "Step 4: nothing UNAVAIL")
    t.check(b.zluks.count("zroot") >= 11, "Step 4: zluks status maps the 11 volumes to zroot")
    check_teardown(t, b)
    vm.kill()
    t.finish()


if __name__ == "__main__":
    main()
