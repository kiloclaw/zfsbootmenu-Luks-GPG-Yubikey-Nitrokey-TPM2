#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
The default boot of an unencrypted pool, with each runtime (Phase 5 of
PORT-ROADMAP.md).

  runtime.py setup    create pool tplain on one disk, without LUKS, holding
                      the ZFS 2.4.4 test boot environment as its bootfs
  runtime.py legacy   boot it with the legacy runtime
  runtime.py rust     boot it with the Rust runtime (zfsbootmenu_runtime=rust)

Both boots must import the pool read-only, run the bundled and test hooks, and
boot the boot environment with kexec after the countdown. The Rust
boot must run under the Rust supervisor as process 1. The legacy boot is the
reference: it must pass before the Rust one counts (rule 2 of the roadmap).
"""

import re
import sys

from zbmtest import (Test, VM, check_be, check_teardown, disk, follow_boot, payload_disk,
                     zbm_image)

POOL = "tplain"
OPTS = ("-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 "
        "-O acltype=posixacl -O xattr=sa -m none")


def image(runtime):
    if runtime == "rust":
        return zbm_image("rust", conf='zfsbootmenu_runtime="rust"')
    return zbm_image("auto")


def setup():
    t = Test("runtime-setup")
    payload = (payload_disk(), "payload")
    img = image("legacy")
    vm = VM(t, "setup", disks=[(disk("runtime", 0, fresh=True), "d0"), payload],
            kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    vm.send_functions()
    rc, _ = vm.sh(f"zpool create -f {OPTS} {POOL} /dev/disk/by-id/virtio-d0")
    t.check(rc == 0, f"zpool create {POOL}")
    vm.must(f"mkbe {POOL}")
    vm.must(f"zfs set org.zfsbootmenu:commandline='console=ttyS0 loglevel=4' {POOL}/ROOT/be")
    vm.sh(f"zpool get -H -o value bootfs {POOL}", show=True)
    vm.must("finish")
    vm.poweroff()
    t.finish()


def boot(runtime):
    t = Test(f"runtime-{runtime}")
    img = image(runtime)
    vm = VM(t, "boot", disks=[(disk("runtime", 0), "d0")], kernel=img["kernel"],
            initrd=img["initrd"], append="console=ttyS0 zbm.timeout=3 loglevel=7").start()
    b = follow_boot(vm, timeout=1200)
    t.check(b.prompts == [], f"no prompts (prompts: {b.prompts})")
    t.check(b.pools().get(POOL) == "ONLINE", f"ZFSBootMenu imported {POOL}: {b.pools().get(POOL)}")
    log = open(t.dir / "serial-boot.log", errors="replace").read()
    m = re.search(r"ZBMTEST-RUNTIME pid1=\[(.*?)\] conf=\[(.*?)\] bootfs=\[(.*?)\]", log)
    t.check(m is not None, "the runtime report hook ran")
    if m:
        pid1, conf, bootfs = (g.strip() for g in m.groups())
        t.note(f"process 1: {pid1}; {conf}")
        t.check(conf == f'ZBM_RUNTIME="{runtime}"', f"the image records the {runtime} runtime")
        if runtime == "rust":
            t.check(pid1 == "/libexec/zbm zfsbootmenu-init --supervise",
                    "process 1 is the Rust supervisor")
        else:
            t.check("zfsbootmenu-init" in pid1 and "zbm" not in pid1,
                    "process 1 is the legacy start-up program")
        t.check(bootfs == f"{POOL}/ROOT/be", f"hooks see BOOTFS ({bootfs})")
    t.check("Booting" in log and f"{POOL}/ROOT/be" in log, "the countdown boots the default boot environment")
    # ZFSBootMenu exports only pools it imported read-write
    check_teardown(t, b, readonly_pools=[POOL])
    check_be(t, b, POOL, [POOL])
    t.finish()


def main():
    which = sys.argv[1]
    if which == "setup":
        setup()
    else:
        boot(which)


if __name__ == "__main__":
    main()
