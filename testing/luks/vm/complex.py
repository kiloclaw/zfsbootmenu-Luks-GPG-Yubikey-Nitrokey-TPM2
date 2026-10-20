#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Complex pools on LUKS, created and used with ZFS 2.4.4.

Disks (16 plus the payload disk):
  d0-d3    RAIDZ2 data vdev of tank          (LUKS, passphrase pw)
  d4-d5    special mirror of tank            (LUKS)
  d6-d7    dedup mirror of tank              (LUKS)
  d8-d9    log mirror of tank                (LUKS)
  d10      cache of tank                     (LUKS)
  d11      hot spare of tank                 (LUKS)
  d12      LUKS half of the mirror "mixed"   (LUKS, passphrase other)
  d13      plain half of the mirror "mixed"  (no encryption)
  d14      disk added to raidz2-0 by RAIDZ expansion (LUKS)
  d15      partition zfsA (LUKS) and partition plainB (plain), mirror "parts"

tank/ROOT/be (the boot environment) uses ZFS native encryption on top of LUKS.
tank/data activates the newer pool features: fast_dedup, block_cloning and
block_cloning_endian, longname, physical_rewrite (zfs rewrite -P),
dynamic_gang_header (forced gang blocks), zstd_compress, blake3,
raidz_expansion and allocation_classes.

Scenarios (complex.py <scenario>):
  setup         create everything in the recovery shell (zbm.luks=off)
  healthy       automatic unlocking; two passphrases, ZFS key in ZFSBootMenu
                and in the boot environment; every pool ONLINE
  crypttab      the same through an embedded crypttab mixing UUID=, ID= and
                PARTLABEL= specifications
  degraded      a RAIDZ2 member and the plain mirror half are missing
  hotplug       zbm.luks= lists every volume; d0 appears while ZFSBootMenu
                waits for it
  hotplug-auto  only the special, dedup, log, cache and spare disks are present;
                the rest appear during the import retry loop
  badkey        wrong ZFS key: recovery shell with volumes and pools available
  skip-recover  every LUKS prompt skipped; the volumes are unlocked with zluks
                in the recovery shell and the boot continues
  oldzfs        an image with an older ZFS (ZBMTEST_OLD_KERNEL and
                ZBMTEST_OLD_INITRD) refuses the ZFS 2.4.4 pools; skipped when
                not configured
"""

import json
import os
import re
import sys
import time

import pexpect

from zbmtest import (FAST_PBKDF, PW, PW_OTHER, WORK, ZFSKEY, Test, VM, be_features,
                     check_be, check_teardown, disk, follow_boot, hostid_arg, partition,
                     payload_disk, run, zbm_image)

N = 16
OPTS = ("-o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux "
        "-O compression=lz4 -O acltype=posixacl -O xattr=sa -O relatime=on -m none")
POOLS = ["tank", "mixed", "parts"]
NEW_FEATURES = ["raidz_expansion", "fast_dedup", "block_cloning", "block_cloning_endian", "longname",
                "physical_rewrite", "dynamic_gang_header", "zstd_compress", "blake3",
                "allocation_classes", "encryption"]
CRYPTTAB = WORK / "disks" / "complex" / "crypttab"


def disks(skip=()):
    return [(None if i in skip else disk("complex", i), f"d{i}") for i in range(N)]


def luks_uuid(path, part=None):
    """UUID of the LUKS volume on a disk image, or on its partition part."""
    off = 0
    if part:
        info = json.loads(run(["sfdisk", "--json", str(path)]))["partitiontable"]
        off = info["partitions"][part - 1]["start"] * info.get("sectorsize", 512)
    with open(path, "rb") as f:
        f.seek(off)
        hdr = f.read(208)
    assert hdr[:6] == b"LUKS\xba\xbe", f"no LUKS header on {path}"
    return hdr[168:208].split(b"\0")[0].decode()


def be_cmdline(skip=()):
    """
    The boot environment's command line as setup sets it (becmdline), without
    the volumes of the disks in skip: dracut waits for every rd.luks.uuid=
    volume and then drops to its emergency shell.
    """
    cl = ["console=ttyS0", "loglevel=4", "rd.luks.key=/etc/zbmtest/be.key"]
    cl += [f"rd.luks.uuid={luks_uuid(disk('complex', i))}" for i in list(range(12)) + [14] if i not in skip]
    if 12 not in skip:
        u = luks_uuid(disk("complex", 12))
        cl += [f"rd.luks.key={u}=/etc/zbmtest/be-other.key", f"rd.luks.uuid={u}"]
    if 15 not in skip:
        cl.append(f"rd.luks.uuid={luks_uuid(disk('complex', 15), 1)}")
    return " ".join(cl)


def setup(t):
    for i in range(N):
        disk("complex", i, fresh=True)
    partition(disk("complex", 15), [("zfsA", "180M"), ("plainB", None)])
    img = zbm_image("auto")
    vm = VM(t, "setup", disks=disks() + [(payload_disk(), "payload")], kernel=img["kernel"],
            initrd=img["initrd"], append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    vm.send_functions()
    byid = lambda *n: " ".join(f"/dev/disk/by-id/virtio-d{i}" for i in n)
    vm.must(f"V=(); U=(); mkluks {PW} '{FAST_PBKDF}' {byid(*range(12), 14)}", t=3600)
    vm.must(f"mkluks {PW_OTHER} '{FAST_PBKDF}' {byid(12)}")
    vm.must(f"mkluks {PW} '{FAST_PBKDF}' /dev/disk/by-partlabel/zfsA")
    # V[0..11] = d0..d11, V[12] = d14, V[13] = d12, V[14] = zfsA
    rc, _ = vm.sh(f"zpool create -f {OPTS} tank raidz2 ${{V[@]:0:4}} special mirror ${{V[4]}} ${{V[5]}} "
                  f"dedup mirror ${{V[6]}} ${{V[7]}} log mirror ${{V[8]}} ${{V[9]}} cache ${{V[10]}} spare ${{V[11]}}")
    t.check(rc == 0, "tank: RAIDZ2 + special, dedup and log mirrors + cache + spare")
    rc, _ = vm.sh("zpool attach tank raidz2-0 ${V[12]} && zpool wait -t raidz_expand tank", t=3600)
    t.check(rc == 0, "RAIDZ expansion: zpool attach tank raidz2-0 <LUKS volume>")
    rc, _ = vm.sh(f"zpool create -f {OPTS} mixed mirror ${{V[13]}} /dev/disk/by-id/virtio-d13")
    t.check(rc == 0, "mixed: mirror of a LUKS volume and a plain disk")
    rc, _ = vm.sh(f"zpool create -f {OPTS} parts mirror ${{V[14]}} /dev/disk/by-partlabel/plainB")
    t.check(rc == 0, "parts: mirror of a LUKS partition and a plain partition")

    # newer features
    vm.must("zfs create -o mountpoint=/data/t -o dedup=on -o compression=zstd -o checksum=blake3 "
            "-o dnodesize=auto -o longname=on tank/data")
    vm.must("head -c 8388608 /dev/urandom > /data/t/a && cp /data/t/a /data/t/dup && zpool sync tank")
    # deduplicated blocks are cloned through the dedup table: clone in a
    # dataset without dedup
    rc, out = vm.sh("zfs create -o mountpoint=/data/bc tank/bc && head -c 4194304 /dev/urandom > /data/bc/a && "
                    "cp --reflink=always /data/bc/a /data/bc/clone && zpool sync tank && "
                    "zpool get -H -o value bcloneused tank",
                    show=True)
    t.check(rc == 0, "cp --reflink=always clones blocks (block cloning)")
    vm.must("n=; for i in {1..300}; do n=${n}n; done; : > \"/data/t/${n}\" && zpool sync tank")
    rc, _ = vm.sh("zfs rewrite -P /data/t/a && zpool sync tank")
    t.check(rc == 0, "zfs rewrite -P (physical rewrite, new in OpenZFS 2.4)")
    # gang blocks: ZFS 2.4 activates dynamic_gang_header when the children of
    # a gang block do not fit (zio_write_gang_block). Forcing every block of
    # 32 KiB or more to gang makes 128 KiB records gang and their ~43 KiB
    # children gang again; their ~15 KiB grandchildren are allocated normally.
    # Use a dataset without dedup and compression.
    try:
        rc, _ = vm.sh("zfs create -o mountpoint=/data/g -o dedup=off -o compression=off -o recordsize=128k tank/gang && "
                      "P=/sys/module/zfs/parameters; echo 32769 > $P/metaslab_force_ganging; "
                      "echo 100 > $P/metaslab_force_ganging_pct; "
                      "head -c 1048576 /dev/urandom > /data/g/gang; zpool sync tank; "
                      "echo 16777217 > $P/metaslab_force_ganging; zpool sync tank", t=900)
        t.check(rc == 0, "forced gang blocks written")
    except pexpect.TIMEOUT:
        t.check(False, "writing forced gang blocks did not finish within 15 minutes")
        vm.kill()
        t.finish()
    vm.must("zfs snapshot tank/data@s1 && zfs bookmark tank/data@s1 tank/data#b1")
    vm.must("zfs create -o mountpoint=/data/m mixed/data && zfs create -o mountpoint=/data/p parts/data && "
            "head -c 4194304 /dev/urandom > /data/m/f && head -c 4194304 /dev/urandom > /data/p/f && "
            "zpool sync mixed parts")

    # boot environment: encrypted root on tank
    vm.must(f"printf '{ZFSKEY}\\n{ZFSKEY}\\n' | mkbe tank -o encryption=on -o keyformat=passphrase "
            "-o keylocation=prompt")
    rc, out = vm.sh("zpool sync tank && features tank", show=True)
    for f in NEW_FEATURES:
        t.check(f in out.split(), f"tank: feature {f} active")
    vm.must('becmdline tank/ROOT/be "${U[@]:0:13}" "${U[13]}=/etc/zbmtest/be-other.key" "${U[14]}"')
    rc, out = vm.sh("zpool status -P", show=True)
    # crypttab for the crypttab scenario: the names the pool was created with
    # and a mix of device specifications
    rc, out = vm.sh(
        'for i in {0..11} 14; do j=$i; [ $i -eq 14 ] && j=12; '
        'case $i in 0|4) s="UUID=${U[$j]}";; *) s="ID=virtio-d$i";; esac; '
        'echo "luks-${U[$j]} $s none discard"; done; '
        'echo "luks-${U[13]} UUID=${U[13]^^} none"; echo "luks-${U[14]} PARTLABEL=zfsA none"')
    lines = [l for l in out.splitlines() if l.startswith("luks-")]
    t.check(len(lines) == 15, f"crypttab with {len(lines)} entries for the crypttab scenario")
    CRYPTTAB.write_text("\n".join(lines) + "\n")
    vm.must("finish")
    vm.poweroff()


def expect_healthy(t, b, states=None, prompts_ok=None, be_states=None, readonly_pools=()):
    states = states or {}
    be_states = states if be_states is None else be_states
    pools = b.pools()
    for p in POOLS:
        want = states.get(p, "ONLINE")
        t.check(pools.get(p) == want, f"ZFSBootMenu imported {p}: {pools.get(p)} (expected {want})")
    st = b.zbm_status
    for sect in ("special", "dedup", "logs", "cache", "spares"):
        t.check(re.search(rf"^\s*{sect}\s*$", st, re.M) is not None, f"tank shows its {sect} section")
    if "tank" not in states:
        t.check(re.search(r"cache\s*\n\s*/dev/mapper/\S+\s+ONLINE", st) is not None, "cache vdev ONLINE")
        t.check(re.search(r"spares\s*\n\s*/dev/mapper/\S+\s+AVAIL", st) is not None, "hot spare AVAIL")
        t.check(len(re.findall(r"raidz2-0.*\n(?:\s+/dev/mapper/\S+\s+ONLINE.*\n){5}", st)) == 1,
                "raidz2-0 has five members after RAIDZ expansion")
    check_teardown(t, b, readonly_pools)
    t.check("be-zfskey" in b.prompts, "boot environment asked for the ZFS key of its encrypted root")
    check_be(t, b, "tank", POOLS, be_states)
    feats = be_features(b)
    for f in NEW_FEATURES:
        t.check(feats.get(("tank", f)) == "active", f"boot environment sees tank feature {f} active")


def main():
    scenario = sys.argv[1]
    t = Test(f"complex-{scenario}")
    if scenario == "setup":
        setup(t)
        t.finish()
    if not CRYPTTAB.exists():
        t.check(False, "run 'complex.py setup' first")
        t.finish()
    auto = zbm_image("auto")
    base = "console=ttyS0 zbm.timeout=3 zbm.retry_delay=4"

    if scenario == "healthy":
        vm = VM(t, scenario, disks=disks(), kernel=auto["kernel"], initrd=auto["initrd"], append=base).start()
        b = follow_boot(vm, answers={"luks": [PW, PW_OTHER]})
        t.check(b.prompts.count("luks") == 2,
                f"two LUKS prompts: one passphrase for 14 volumes, one for the volume with its own ({b.prompts})")
        t.check(b.prompts.count("zfskey") == 1, "ZFSBootMenu asked once for the ZFS key")
        expect_healthy(t, b)

    elif scenario == "crypttab":
        img = zbm_image("complex-crypttab", crypttab=CRYPTTAB.read_text())
        vm = VM(t, scenario, disks=disks(), kernel=img["kernel"], initrd=img["initrd"], append=base).start()
        b = follow_boot(vm, answers={"luks": [PW, PW_OTHER]})
        t.check(b.prompts.count("luks") == 2, f"two LUKS prompts in crypttab mode ({b.prompts})")
        expect_healthy(t, b)

    elif scenario == "degraded":
        # the boot environment's initramfs must not wait for the missing
        # volume: its command line for this boot leaves it out, as MOD+E in
        # ZFSBootMenu would (docs/luks/administration.rst)
        kcl = be_cmdline(skip=(1, 13))
        vm = VM(t, scenario, disks=disks(skip=(1, 13)), kernel=auto["kernel"], initrd=auto["initrd"],
                append=f'{base} zbm.kcl_override="{kcl}"').start()
        b = follow_boot(vm, answers={"luks": [PW, PW_OTHER]})
        expect_healthy(t, b, states={"tank": "DEGRADED", "mixed": "DEGRADED"})

    elif scenario == "hotplug":
        specs = ",".join([f"ID=virtio-d{i}" for i in list(range(13)) + [14]] + ["PARTLABEL=zfsA"])
        vm = VM(t, scenario, disks=disks(skip=(0,)), kernel=auto["kernel"], initrd=auto["initrd"],
                append=f"{base} zbm.luks={specs}").start()
        b = follow_boot(vm, answers={"luks": [PW, PW_OTHER]},
                        on={"missing": lambda v: v.hotplug_disk(disk("complex", 0), "d0")})
        t.check("luks" in b.prompts, f"prompts: {b.prompts}")
        expect_healthy(t, b)

    elif scenario == "hotplug-auto":
        # All late disks, LUKS volumes and the unencrypted halves of mixed
        # and parts, are plugged in within one retry interval: the retry must
        # unlock the late volumes before it imports. Disks that arrive after a
        # retry has imported the pools DEGRADED are not used, as the loop ends
        # once a pool imports (see LATE DEVICES in zfsbootmenu-luks(7)).
        late = [0, 1, 2, 3, 12, 13, 14, 15]
        vm = VM(t, scenario, disks=disks(skip=late), kernel=auto["kernel"], initrd=auto["initrd"],
                append="console=ttyS0 zbm.timeout=3 zbm.retry_delay=60").start()

        def plug(v):
            t.step("ZFSBootMenu is in its import retry loop; adding the remaining disks")
            for i in late:
                v.hotplug_disk(disk("complex", i), f"d{i}")
                time.sleep(1)
        b = follow_boot(vm, answers={"luks": [PW, PW, PW_OTHER]}, on={"retry": plug})
        t.check(b.prompts.count("luks") >= 2, f"late volumes were unlocked during the retry loop ({b.prompts})")
        expect_healthy(t, b)

    elif scenario == "badkey":
        vm = VM(t, scenario, disks=disks(), kernel=auto["kernel"], initrd=auto["initrd"], append=base).start()
        b = follow_boot(vm, answers={"luks": [PW, PW_OTHER], "zfskey": ["wrong1", "wrong2", "wrong3"]}, be=False)
        t.check("keyload-failed" in b.prompts, "a wrong ZFS key ends in the recovery shell (root mount failure)")
        vm.shell_ready()
        rc, out = vm.sh("zluks status", show=True)
        t.check(out.count("open") >= 15, "all LUKS volumes stay open in the recovery shell")
        rc, out = vm.sh("zpool list -H -o name,health", show=True)
        t.check(all(p in out for p in POOLS), "pools stay imported for recovery")
        rc, out = vm.sh(f"printf '{ZFSKEY}' | zfs load-key -L prompt tank/ROOT/be && "
                        "zfs get -H -o value keystatus tank/ROOT/be", show=True)
        t.check(rc == 0 and "available" in out, "the key can be loaded from the recovery shell")
        vm.kill()

    elif scenario == "skip-recover":
        vm = VM(t, scenario, disks=disks(), kernel=auto["kernel"], initrd=auto["initrd"], append=base).start()
        skipped = 0
        while True:
            i = vm.expect([r"Passphrase \(leave empty to skip\): ", r"Unable to import",
                           r"Dropping to an emergency shell"], timeout=1800)
            if i > 0:
                break
            skipped += 1
            vm.sendline("")
        # mixed and parts import DEGRADED through their unencrypted halves, so
        # ZFSBootMenu finds no boot environment and opens its emergency shell
        # (with no importable pool it would stay in the import retry loop)
        t.check(skipped >= 2 and i == 2,
                f"{skipped} LUKS prompts skipped; no boot environment without tank, emergency shell")
        vm.shell_ready()
        rc, out = vm.sh("zpool list -H -o name,health", show=True)
        t.check("tank" not in out and "DEGRADED" in out, "only the pools with unencrypted halves are imported")
        rc, out = vm.sh("zluks status", show=True)
        t.check(rc == 0, "zluks status runs")
        vm.sendline("zluks unlock; echo RC=$?")
        prompts = 0
        while True:
            i = vm.expect([r"Passphrase \(leave empty to skip\): ", r"RC=(\d+)"], timeout=1800)
            if i == 1:
                break
            prompts += 1
            vm.sendline(PW if prompts == 1 else PW_OTHER)
        t.check(vm.match.group(1) == "0", f"zluks unlock opened the volumes ({prompts} prompts)")
        rc, out = vm.sh("zluks status | grep -c open", show=True)
        t.step("leaving the shell; ZFSBootMenu imports tank and shows its menu")
        vm.sendline("exit")
        b = follow_boot(vm, menu=True)
        t.check("menu" in b.prompts, "the boot menu lists tank's boot environment after the shell")
        # mixed and parts were imported, read-only, through their unencrypted
        # halves before zluks opened the LUKS halves; ZFSBootMenu leaves such
        # pools imported at kexec
        expect_healthy(t, b, states={"mixed": "DEGRADED", "parts": "DEGRADED"}, be_states={},
                       readonly_pools=("mixed", "parts"))

    elif scenario == "oldzfs":
        k, i = os.environ.get("ZBMTEST_OLD_KERNEL"), os.environ.get("ZBMTEST_OLD_INITRD")
        if not (k and i):
            t.note("ZBMTEST_OLD_KERNEL/ZBMTEST_OLD_INITRD not set; skipped")
            t.finish()
        vm = VM(t, scenario, disks=disks(), kernel=k, initrd=i, append=f"{base} {hostid_arg()}").start()
        answers = [PW, PW_OTHER]
        while True:
            j = vm.expect([r"Passphrase \(leave empty to skip\): ", r"Unable to import",
                           r"Dropping to an emergency shell"], timeout=1800)
            if j > 0:
                break
            vm.sendline(answers.pop(0) if answers else PW)
        # mixed and parts have no active feature the older ZFS lacks, so it
        # imports them; tank, with raidz_expansion active, is refused, and so
        # is its boot environment
        if j == 1:
            t.step("no pool imported; opening the recovery shell")
            vm.send("\x1b")
        else:
            t.step("no boot environment found; ZFSBootMenu dropped to its emergency shell")
        t.check(True, "the older image unlocked the volumes and booted nothing")
        vm.shell_ready()
        rc, out = vm.sh("zfs version; zpool list -H -o name,health,readonly", show=True)
        t.check(not re.search(r"^tank\s", out, re.M), "the older ZFS did not import tank")
        imported = sorted(re.findall(r"^(mixed|parts)\s+ONLINE", out, re.M))
        t.note(f"pools without unsupported active features imported by the older ZFS: {imported}")
        rc, out = vm.sh("zpool import 2>&1 | head -60", show=True)
        t.check("tank" in out and ("raidz_expansion" in out or "unsupported feature" in out.lower()),
                "zpool import names the unsupported feature of tank")
        t.check("cannot be imported" in out, "zpool import says tank cannot be imported")
        vm.kill()
    else:
        t.check(False, f"unknown scenario {scenario}")
    t.finish()


if __name__ == "__main__":
    main()
