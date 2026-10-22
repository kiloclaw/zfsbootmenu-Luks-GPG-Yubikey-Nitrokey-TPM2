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

import pexpect

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
    return zbm_image("auto" + suffix, files=files, runtime=runtime)


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


PILOT_KEY = "zbm-pilot-key-1234"
STATS = r"(\d+) frames, mean ([\d.]+) ms, slowest ([\d.]+) ms\r*\n"


def serial_bytes(vm):
    """The size of the serial log so far, in bytes, after reading what the VM
    sent in the last second (the log grows only as output is read)."""
    vm.c.expect([pexpect.TIMEOUT], timeout=1)
    vm.c.logfile_read.flush()
    return (vm.t.dir / f"serial-{vm.label}.log").stat().st_size


def pilot_menu(vm, t, term, size):
    """The menu pilot on the serial console with TERM=term; types 'be' and
    chooses the match. Returns the frame statistics and the serial bytes the
    filter's frames took."""
    option = f" --size {size}" if size else ""
    vm.sendline(f"TERM={term} /libexec/zbm zbm-menu-pilot --stats{option}")
    # Ratatui moves the cursor over blank cells instead of writing spaces, so
    # the screen is matched word by word, and compared without spaces.
    i = vm.expect([r"environments", r"zbm-menu-pilot: [^\r\n]*"], timeout=120)
    if i == 1:
        t.check(False, f"TERM={term}: the menu starts ({vm.match.group(0)})")
        return None, 0
    vm.expect(r"\[Ctrl-U\]", timeout=60)
    screen = vm.before.replace(" ", "")
    t.check("tplain/ROOT/be" in screen, f"TERM={term}: the boot environment is listed")
    drawn_ascii = "+Bootenvironments" in screen
    t.check(drawn_ascii == (term == "vt100"),
            f"TERM={term}: {'ASCII' if term == 'vt100' else 'line-drawing'} borders")
    time.sleep(1)
    before = serial_bytes(vm)
    for key in "be":
        vm.send(key)
        time.sleep(1)
    typed = serial_bytes(vm) - before
    start = time.time()
    vm.send("\r")
    vm.expect(r"chosen: (\S+)\r*\n", timeout=60)
    chosen = vm.match.group(1)
    vm.expect(STATS, timeout=60)
    stats = tuple(float(x) for x in vm.match.groups())
    t.check(chosen == "tplain/ROOT/be", f"TERM={term}: Enter chooses tplain/ROOT/be ({chosen})")
    t.note(f"TERM={term}: {int(stats[0])} frames, mean {stats[1]} ms, slowest {stats[2]} ms; "
           f"two filter keys took {typed} serial bytes; Enter to answer {time.time() - start:.2f} s")
    vm.sh("true", quiet=True)
    return stats, typed


def pilot_unlock(vm, t):
    """zluks --pilot open on a LUKS volume formatted in the VM."""
    rc, out = vm.sh("command -v losetup && command -v cryptsetup", show=True)
    if rc != 0:
        t.note(f"pilot 1 skipped: the image lacks losetup or cryptsetup ({out})")
        return
    # The image has no dd; make the 32 MiB file with whichever tool it has.
    rc, out = vm.sh(
        "if command -v truncate >/dev/null; then truncate -s 32M /tmp/pilot.img; "
        "elif command -v fallocate >/dev/null; then fallocate -l 32M /tmp/pilot.img; "
        "else head -c 33554432 /dev/zero > /tmp/pilot.img; fi && "
        "dev=$(losetup -f --show /tmp/pilot.img) && "
        f"printf %s {PILOT_KEY} | cryptsetup luksFormat --batch-mode --type luks2 --pbkdf pbkdf2 "
        "--pbkdf-force-iterations 1000 --key-file=- \"$dev\" && "
        f"printf %s {PILOT_KEY} > /tmp/pilot.key && printf wrong > /tmp/wrong.key && echo DEV=$dev",
        show=True)
    t.check(rc == 0, "a LUKS volume was formatted on a loop device")
    if rc != 0:
        return
    rc, out = vm.sh('zluks --pilot open "$dev" zbm-pilot --key-file /tmp/wrong.key '
                    "--journal /tmp/pilot.jsonl", show=True)
    t.check(rc == 1 and "no key slot of" in out, "a wrong key file is refused, with the reason")
    rc, out = vm.sh('zluks --pilot open "$dev" zbm-pilot --key-file /tmp/pilot.key '
                    "--journal /tmp/pilot.jsonl", show=True)
    t.check(rc == 0 and "as /dev/mapper/zbm-pilot (key file)" in out,
            "zluks --pilot open opens the volume with the key file")
    rc, _ = vm.sh("test -b /dev/mapper/zbm-pilot")
    t.check(rc == 0, "/dev/mapper/zbm-pilot exists")
    rc, out = vm.sh("cat /tmp/pilot.jsonl", show=True)
    t.check('"stdin":"secret"' in out, "the journal records that the key went to standard input")
    t.check(PILOT_KEY not in out, "the journal does not contain the key")
    t.check('"message":"refused"' in out and '"message":"opened"' in out,
            "the journal records the refused and the successful attempt")
    vm.sh("cryptsetup close zbm-pilot; losetup -d \"$dev\"")


def pilot():
    """
    The pilots of REFACTOR-ROADMAP.md (R0) in the Rust image, on the serial
    console of the recovery shell: the menu with TERM=vt100 and TERM=linux,
    and zluks --pilot open on a LUKS volume formatted in the VM.
    """
    t = Test("runtime-rust-pilot")
    img = image("rust")
    vm = VM(t, "boot", disks=[d(0)], kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.timeout=30 loglevel=7").start()
    vm.expect(r"Booting \S*tplain/ROOT/be\S* in", timeout=1200)
    time.sleep(2)
    vm.send("\x1b")
    vm.shell_ready()
    pilot_menu(vm, t, "vt100", "80x24")
    pilot_menu(vm, t, "linux", None)
    pilot_unlock(vm, t)
    vm.kill()
    t.finish()


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
    if which == "rust-pilot":
        pilot()
    boot(runtime, scenario)


if __name__ == "__main__":
    main()
