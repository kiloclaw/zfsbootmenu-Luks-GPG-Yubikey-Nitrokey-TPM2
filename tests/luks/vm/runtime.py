#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Start-up and pool import with each runtime, without LUKS (Phase 5 and package
A of Phase 6 in PORT-ROADMAP.md).

  runtime.py setup    create two pools, each holding the ZFS 2.4.4 test boot
                      environment as its bootfs: tplain on disk d0, exported
                      by this host, and tforeign on disk d1, left imported by
                      another host (host ID deadbeef)

  runtime.py RUNTIME[-SCENARIO]
                      boot with RUNTIME, legacy or rust (zfsbootmenu_runtime=
                      "rust"); the scenarios:

    (none)    tplain: the countdown boots its boot environment; process 1,
              the runtime and BOOTFS as hooks see them
    hostid    tforeign alone: host ID matching imports it with the other
              host's ID, which the boot environment gets as spl_hostid
    strict    tforeign alone with zbm.import_policy=strict: no import; the
              retry prompt appears
    force     tforeign alone with zbm.import_policy=force: imported at once
    prefer    both pools with zbm.prefer=tplain!!: only tplain is imported,
              and its boot environment boots
    late      d0 is plugged in while the import retry loop waits
    waitfor   zbm.wait_for names d0, which is plugged in while ZFSBootMenu
              waits for it
    hooks     zbm.hookroot adds a hook from a FAT disk, zbm.skip_hooks
              disables the runtime report hook
    signals   the signal dispositions of a hook, fzf (legacy only: the
              menu) and the recovery shell; the Rust run compares with the
              legacy run's

Every scenario must pass with the legacy image first (rule 2 of the roadmap).
"""

import json
import re
import sys
import time

from zbmtest import (Test, VM, check_be, clean, check_teardown, disk, follow_boot, payload_disk,
                     vfat_disk, WORK, zbm_image)

OPTS = ("-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 "
        "-O acltype=posixacl -O xattr=sa -m none")
BE_CMDLINE = "console=ttyS0 loglevel=4"
FOREIGN = b"\xef\xbe\xad\xde"   # deadbeef, little-endian

HOOK = b"""#!/bin/bash
echo "ZBMTEST-HOOKROOT ran from $0"
"""

# An early-setup hook that reports its own signal dispositions and records
# those of fzf once the menu runs
SIGNALS_HOOK = b"""#!/bin/bash
sig() { grep -E '^Sig(Blk|Ign|Cgt):' "/proc/$1/status" | tr -s ' \\t' ' ' | tr '\\n' ' '; }
echo "ZBMTEST-SIG hook=[$(sig $$)]"
(
  n=0
  while [ "${n}" -lt 1800 ]; do
    for f in /proc/[0-9]*/comm; do
      read -r c < "$f" 2>/dev/null || continue
      [ "${c}" = fzf ] || continue
      p="${f#/proc/}"; p="${p%/comm}"
      echo "fzf=[$(sig "$p")]" > /zfsbootmenu/zbmtest-fzf-sig
      exit 0
    done
    sleep 1
    n=$((n + 1))
  done
) </dev/null >/dev/null 2>&1 &
"""
SIG_KEYS = ("SigBlk", "SigIgn", "SigCgt")


def image(runtime, signals=False):
    files = {"hooks/early-setup.d/95-zbmtest-signals": SIGNALS_HOOK} if signals else None
    suffix = "-signals" if signals else ""
    if runtime == "rust":
        return zbm_image("rust" + suffix, conf='zfsbootmenu_runtime="rust"', files=files)
    return zbm_image("auto" + suffix, files=files)


def masks(text):
    """{SigBlk, SigIgn, SigCgt: hex mask} from /proc/<pid>/status lines."""
    return {k: v for k, v in re.findall(r"(Sig(?:Blk|Ign|Cgt)):\s*([0-9a-f]+)", text)}


def signals(runtime):
    """
    Signal dispositions of an early-setup hook, of fzf in the menu and of the
    recovery shell, on the console. The legacy run records them; the Rust run
    must reproduce those of the hook and the shell (the menu is package E).
    """
    t = Test(f"runtime-{runtime}-signals")
    img = image(runtime, signals=True)
    vm = VM(t, "boot", disks=[d(0)], kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.timeout=30 loglevel=7").start()
    vm.expect(r"ZBMTEST-SIG hook=\[(.*?)\]", timeout=1200)
    hook = masks(vm.match.group(1))
    vm.expect(r"Booting \S*tplain/ROOT/be\S* in", timeout=1200)
    time.sleep(2)
    vm.send("\x1b")
    if runtime == "legacy":
        t.step("menu: waiting for fzf, then CTRL+R for the recovery shell")
        time.sleep(15)
        vm.send("\x12")
    vm.shell_ready()
    rc, out = vm.sh("read -r l < /zfsbootmenu/zbmtest-fzf-sig 2>/dev/null; echo \"${l}\"; grep -E '^Sig(Blk|Ign|Cgt)' /proc/$$/status",
                    show=True)
    fzf_text = re.search(r"fzf=\[(.*?)\]", out)
    fzf = masks(fzf_text.group(1)) if fzf_text else {}
    shell = masks(out.split("fzf=[")[-1] if not fzf_text else out[fzf_text.end():])
    vm.kill()
    result = {"hook": hook, "fzf": fzf, "shell": shell}
    (t.dir / "signals.json").write_text(json.dumps(result, indent=2) + "\n")
    t.note(f"hook {hook}; fzf {fzf}; shell {shell}")
    t.check(sorted(hook) == sorted(SIG_KEYS), "the hook reported its dispositions")
    t.check(sorted(shell) == sorted(SIG_KEYS), "the recovery shell reported its dispositions")
    if runtime == "legacy":
        t.check(sorted(fzf) == sorted(SIG_KEYS), "fzf's dispositions were recorded")
        t.finish()
    ref = WORK / "tests" / "runtime-legacy-signals" / "signals.json"
    t.check(ref.exists(), "the legacy run recorded the reference")
    if ref.exists():
        legacy = json.loads(ref.read_text())
        for what in ("hook", "shell"):
            for key in SIG_KEYS:
                t.check(result[what].get(key) == legacy[what].get(key),
                        f"{what} {key}: {result[what].get(key)} (legacy {legacy[what].get(key)})")
        t.note("fzf runs in the menu, which arrives with package E")
    t.finish()


def d(i):
    return (disk("runtime", i), f"d{i}")


def setup():
    t = Test("runtime-setup")
    payload = (payload_disk(), "payload")
    img = image("legacy")
    disks = [(disk("runtime", i, fresh=True), f"d{i}") for i in range(2)]
    vm = VM(t, "setup", disks=disks + [payload], kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    vm.send_functions()
    for i, pool in enumerate(("tplain", "tforeign")):
        rc, _ = vm.sh(f"zpool create -f {OPTS} {pool} /dev/disk/by-id/virtio-d{i}")
        t.check(rc == 0, f"zpool create {pool}")
        vm.must(f"mkbe {pool}")
        vm.must(f"zfs set org.zfsbootmenu:commandline='{BE_CMDLINE}' {pool}/ROOT/be")
    vm.must("finish")
    # tforeign belongs to another host and stays imported, as after a crash
    hostid = "".join(f"\\x{b:02x}" for b in FOREIGN)
    vm.must(f"printf '{hostid}' > /etc/hostid && zpool import -N tforeign")
    rc, out = vm.sh("od -An -tx4 /etc/hostid; zpool list -H -o name", show=True)
    t.check(rc == 0 and "deadbeef" in out, "tforeign imported by host deadbeef")
    vm.sh("sync")
    vm.poweroff()
    t.finish()


def report(t, log):
    """process 1, the runtime and BOOTFS from the runtime report hook."""
    m = re.search(r"ZBMTEST-RUNTIME pid1=\[(.*?)\] conf=\[(.*?)\] bootfs=\[(.*?)\]", log)
    return tuple(g.strip() for g in m.groups()) if m else None


def check_runtime(t, runtime, log, bootfs):
    r = report(t, log)
    t.check(r is not None, "the runtime report hook ran")
    if not r:
        return
    pid1, conf, seen = r
    t.note(f"process 1: {pid1}; {conf}")
    t.check(conf == f'ZBM_RUNTIME="{runtime}"', f"the image records the {runtime} runtime")
    if runtime == "rust":
        t.check(pid1 == "/libexec/zbm zfsbootmenu-init --supervise", "process 1 is the Rust supervisor")
    else:
        t.check("zfsbootmenu-init" in pid1 and "zbm" not in pid1, "process 1 is the legacy start-up program")
    t.check(seen == bootfs, f"hooks see BOOTFS ({seen})")


def boot(runtime, scenario):
    name = f"runtime-{runtime}" + (f"-{scenario}" if scenario else "")
    t = Test(name)
    img = image(runtime)
    append = "console=ttyS0 zbm.timeout=3 loglevel=7"
    disks = [d(0)]
    if scenario in ("hostid", "strict", "force"):
        disks = [d(1)]
    elif scenario == "prefer":
        disks = [d(0), d(1)]
    elif scenario in ("late", "waitfor"):
        disks = []
    elif scenario == "hooks":
        stick = vfat_disk(WORK / "disks" / "runtime" / "hooks.img", "HOOKS",
                          {"zbm/teardown.d/50-hookroot": HOOK})
        disks = [d(0), (stick, "hooks")]
    extra = {
        "strict": " zbm.import_policy=strict zbm.retry_delay=5",
        "force": " zbm.import_policy=force",
        "prefer": " zbm.prefer=tplain!!",
        "late": " zbm.retry_delay=10",
        "waitfor": " zbm.wait_for=/dev/disk/by-id/virtio-d0 zbm.retry_delay=10",
        "hooks": " zbm.hookroot=/dev/disk/by-id/virtio-hooks//zbm zbm.skip_hooks=20-runtime.sh",
    }.get(scenario, "")
    vm = VM(t, "boot", disks=disks, kernel=img["kernel"], initrd=img["initrd"],
            append=append + extra).start()

    if scenario == "strict":
        i = vm.expect([r"Unable to import", r"ZBMTEST-STATUS-BEGIN"], timeout=1200)
        t.check(i == 0, "the strict policy does not import a pool of another host")
        vm.kill()
        log = clean(open(t.dir / "serial-boot.log", errors="replace").read())
        t.check("Booting tforeign" not in log and " for tforeign/" not in log, "nothing is booted")
        t.finish()

    on = {}
    if scenario == "late":
        on["retry"] = lambda v: v.hotplug_disk(disk("runtime", 0), "d0")
    if scenario == "waitfor":
        vm.expect(r"required devices are missing", timeout=1200)
        t.step("ZFSBootMenu waits for d0; plugging it in")
        time.sleep(2)
        vm.hotplug_disk(disk("runtime", 0), "d0")
    b = follow_boot(vm, on=on, be=scenario != "force", timeout=1200)
    log = clean(open(t.dir / "serial-boot.log", errors="replace").read())
    t.check(b.prompts == [], f"no prompts (prompts: {b.prompts})")
    pools = b.pools()

    if scenario in ("hostid", "force"):
        t.check(pools.get("tforeign") == "ONLINE", f"ZFSBootMenu imported tforeign: {pools.get('tforeign')}")
        if scenario == "hostid":
            check_runtime(t, runtime, log, "tforeign/ROOT/be")
            check_teardown(t, b, readonly_pools=["tforeign"])
            spl = b.be_lines.get("BE-CMDLINE", [""])[0]
            t.check("spl_hostid=0xdeadbeef" in spl or "spl_hostid=deadbeef" in spl,
                    f"the boot environment gets the matched host ID ({spl})")
            check_be(t, b, "tforeign", ["tforeign"])
        else:
            check_teardown(t, b, readonly_pools=["tforeign"])
            vm.kill()
        t.finish()

    t.check(pools.get("tplain") == "ONLINE", f"ZFSBootMenu imported tplain: {pools.get('tplain')}")
    if scenario == "prefer":
        t.check("tforeign" not in pools, f"zbm.prefer=tplain!! imports only tplain ({sorted(pools)})")
    if scenario == "hooks":
        t.check("ZBMTEST-HOOKROOT ran from /libexec/hooks/teardown.d/50-hookroot" in log,
                "zbm.hookroot added a teardown hook")
        t.check(report(t, log) is None, "zbm.skip_hooks disabled the runtime report hook")
    else:
        check_runtime(t, runtime, log, "tplain/ROOT/be")
    t.check(re.search(r"Booting \S+ for tplain/ROOT/be", log) is not None,
            "the countdown boots the default boot environment")
    # ZFSBootMenu exports only pools it imported read-write
    check_teardown(t, b, readonly_pools=["tplain"])
    check_be(t, b, "tplain", ["tplain"])
    t.finish()


def main():
    which = sys.argv[1]
    if which == "setup":
        setup()
    runtime, _, scenario = which.partition("-")
    if scenario == "signals":
        signals(runtime)
    boot(runtime, scenario)


if __name__ == "__main__":
    main()
