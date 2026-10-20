#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
Key management with a software TPM and GnuPG, on ZFS 2.4.4 pools.

Disks: four, one GPT partition each (zfs0-zfs3). zfs0 and zfs1 are enrolled
with systemd-cryptenroll for TPM2 with the PIN 1234 (no PCRs). zfs2 has two
tokens: TPM2 with another PIN (5678) as token 0 and a clevis TPM2 binding as
token 1. zfs3 has a key file encrypted with gpg --symmetric (passphrase
gpgpass). Every volume also keeps the passphrase pw. The crypttab names the mappings zroot-a, zroot-b (mirror
zroot, root of the boot environment), tank-a and tank-b (mirror tank).

Scenarios (keys.py <scenario>):
  setup    LUKS formatting and enrolment on the host (loop devices, swtpm),
           then pool creation with ZFS 2.4.4 in the recovery shell
  plugin   systemd's TPM2 token plugin: a wrong PIN, the right PIN (reused
           for zfs1); zfs2 is unlocked silently by its second token (clevis
           needs no interaction and is tried before PIN tokens); the GnuPG
           passphrase for zfs3; boot to the boot environment. When only the
           first token were seen, zfs2 would ask for its PIN and passphrase.
  native   the same with zfsbootmenu_luks_token_plugins="no" (tpm2-tools)
  lockout  the TPM allows one wrong PIN: after it, ZFSBootMenu explains the
           lockout and falls back to the passphrase
  notpm    no TPM device: the TPM2 volumes fall back to the passphrase
"""

import os
import re
import subprocess
import sys

from zbmtest import (FAST_PBKDF, PW, WORK, Swtpm, Test, VM, check_be, check_teardown, disk,
                     follow_boot, loop_partition, partition, payload_disk, run, zbm_image)

CRYPTTAB = """zroot-a  PARTLABEL=zfs0  none                                discard
zroot-b  PARTLABEL=zfs1  none                                discard
tank-a   PARTLABEL=zfs2  none                                -
tank-b   PARTLABEL=zfs3  /etc/zfsbootmenu/keys/tank.key.gpg  -
"""
TPM = WORK / "disks" / "keys" / "tpm"
GPGFILE = WORK / "disks" / "keys" / "tank.key.gpg"
RC = "mkdir -p /etc/zfsbootmenu/keys && cp /build/keys/tank.key.gpg /etc/zfsbootmenu/keys/"


def disks():
    return [(disk("keys", i), f"d{i}") for i in range(4)]


def host_prepare(t):
    """Format and enrol the four volumes on loop devices."""
    if TPM.exists():
        run(["rm", "-rf", str(TPM)])
    TPM.mkdir(parents=True)
    run(["swtpm_setup", "--tpm2", "--tpmstate", str(TPM), "--overwrite"])
    port = 2500 + os.getpid() % 500
    pidfile = WORK / "disks" / "keys" / "swtpm.pid"
    run(["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={TPM}", "--server", f"type=tcp,port={port}",
         "--ctrl", f"type=tcp,port={port + 1}", "--flags", "not-need-init,startup-clear", "-d",
         "--pid", f"file={pidfile}"])
    tcti = f"swtpm:host=127.0.0.1,port={port}"
    loops = []
    try:
        for i in range(4):
            d = disk("keys", i, fresh=True)
            partition(d, [(f"zfs{i}", None)])
            part = loop_partition(d, 1)
            loops.append(part)
            subprocess.run(["udevadm", "settle"], check=False)
            run(f"printf '%s' {PW} | cryptsetup luksFormat -q --type luks2 {FAST_PBKDF} --key-file=- {part}",
                shell=True)
            if i < 3:
                env = dict(os.environ, PASSWORD=PW, NEWPIN="5678" if i == 2 else "1234")
                r = subprocess.run(["systemd-cryptenroll", f"--tpm2-device={tcti}", "--tpm2-pcrs=",
                                    "--tpm2-with-pin=yes", part], env=env, capture_output=True, text=True)
                t.check(r.returncode == 0, f"systemd-cryptenroll TPM2 + PIN on zfs{i}")
            if i == 2:
                env = dict(os.environ, TPM2TOOLS_TCTI=tcti)
                r = subprocess.run(f"printf '%s' {PW} | clevis luks bind -y -k - -d {part} tpm2 '{{}}'",
                                   shell=True, env=env, capture_output=True, text=True)
                t.check(r.returncode == 0, f"clevis luks bind tpm2 on zfs2 (second token) {r.stderr.strip()[:80]}")
            if i == 3:
                key = WORK / "disks" / "keys" / "tank.key"
                key.write_bytes(os.urandom(64))
                run(f"printf '%s' {PW} | cryptsetup luksAddKey -q {FAST_PBKDF} --key-file=- {part} {key}",
                    shell=True)
                gnupg = WORK / "disks" / "keys" / "gnupg"
                run(["rm", "-rf", str(gnupg)])
                gnupg.mkdir(mode=0o700)
                if GPGFILE.exists():
                    GPGFILE.unlink()
                run(["gpg", "--homedir", str(gnupg), "-q", "--batch", "--pinentry-mode", "loopback",
                     "--passphrase", "gpgpass", "--symmetric", "-o", str(GPGFILE), str(key)])
                key.unlink()
                t.check(GPGFILE.exists(), "key file of zfs3 encrypted with gpg --symmetric")
            dump = run(["cryptsetup", "luksDump", "--dump-json-metadata", part])
            types = re.findall(r'"type":"(systemd-tpm2|clevis)"', dump.replace(" ", ""))
            want = {0: ["systemd-tpm2"], 1: ["systemd-tpm2"], 2: ["systemd-tpm2", "clevis"], 3: []}[i]
            t.check(types == want, f"zfs{i} tokens: {types}")
    finally:
        for loop in loops:
            subprocess.run(["losetup", "-d", loop], check=False)
        subprocess.run(["kill", pidfile.read_text().strip()], check=False)


def setup(t):
    host_prepare(t)
    img = zbm_image("auto")
    vm = VM(t, "setup", disks=disks() + [(payload_disk(), "payload")], kernel=img["kernel"],
            initrd=img["initrd"], append="console=ttyS0 zbm.luks=off zbm.retry_delay=3").start()
    vm.recovery_shell()
    vm.send_functions()
    for i, name in enumerate(["zroot-a", "zroot-b", "tank-a", "tank-b"]):
        vm.must(f"printf '%s' {PW} | cryptsetup open --key-file=- /dev/disk/by-partlabel/zfs{i} {name}")
    opts = "-o ashift=12 -o compatibility=openzfs-2.4-linux -O compression=lz4 -m none"
    vm.must(f"zpool create -f {opts} zroot mirror /dev/mapper/zroot-a /dev/mapper/zroot-b")
    vm.must(f"zpool create -f {opts} tank mirror /dev/mapper/tank-a /dev/mapper/tank-b")
    vm.must("mkbe zroot")
    vm.must("becmdline zroot/ROOT/be $(for i in 0 1 2 3; do cryptsetup luksUUID /dev/disk/by-partlabel/zfs$i; done)")
    vm.must("zpool export -a && for m in zroot-a zroot-b tank-a tank-b; do cryptsetup close $m; done; "
            "grep -q ' /pl ' /proc/mounts && umount /pl; true")
    vm.poweroff()


def image(native):
    return zbm_image("keys-native" if native else "keys-plugin", crypttab=CRYPTTAB,
                     files={"keys/tank.key.gpg": GPGFILE.read_bytes()}, rc=RC,
                     conf='zfsbootmenu_luks_token_plugins="no"' if native else "")


def boot_checks(t, b):
    pools = b.pools()
    t.check(pools.get("zroot") == "ONLINE" and pools.get("tank") == "ONLINE",
            f"ZFSBootMenu imported zroot and tank: {pools}")
    check_teardown(t, b)
    check_be(t, b, "zroot", ["zroot", "tank"])


def main():
    scenario = sys.argv[1]
    t = Test(f"keys-{scenario}")
    if scenario == "setup":
        setup(t)
        t.finish()
    if not GPGFILE.exists():
        t.check(False, "run 'keys.py setup' first")
        t.finish()
    tpm = Swtpm(TPM)
    tpm.clear_lockout()
    base = "console=ttyS0 zbm.timeout=3"

    if scenario in ("plugin", "native"):
        img = image(scenario == "native")
        vm = VM(t, scenario, disks=disks(), kernel=img["kernel"], initrd=img["initrd"], append=base,
                tpm=TPM).start()
        b = follow_boot(vm, answers={"tpm2pin": ["0000", "1234", ""], "gpg": ["gpgpass"]})
        t.check(b.prompts == ["tpm2pin", "tpm2pin", "gpg"],
                f"wrong PIN, right PIN (reused for zfs1), no prompt for zfs2, GnuPG passphrase: {b.prompts}")
        t.check("luks" not in b.prompts,
                "no fallback to the LUKS passphrase: zfs2 was unlocked by its second token (clevis)")
        boot_checks(t, b)

    elif scenario == "lockout":
        port = 2600 + os.getpid() % 300
        pid = WORK / "disks" / "keys" / "swtpm-lock.pid"
        run(["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={TPM}", "--server", f"type=tcp,port={port}",
             "--ctrl", f"type=tcp,port={port + 1}", "--flags", "startup-clear", "-d", "--pid", f"file={pid}"])
        tcti = f"swtpm:host=127.0.0.1,port={port}"
        try:
            run(["tpm2_dictionarylockout", "-T", tcti, "-s", "-n", "1", "-t", "3600", "-l", "3600"])
        finally:
            subprocess.run(["kill", pid.read_text().strip()], check=False)
        img = image(False)
        vm = VM(t, scenario, disks=disks(), kernel=img["kernel"], initrd=img["initrd"], append=base,
                tpm=TPM).start()
        b = follow_boot(vm, answers={"tpm2pin": ["0000", "9999"], "luks": [PW, PW, PW], "gpg": ["gpgpass"]})
        log = open(vm.t.dir / f"serial-{scenario}.log").read()
        t.check("locked against PIN guessing" in log, "ZFSBootMenu explains the TPM lockout")
        t.check("luks" in b.prompts, f"falls back to the passphrase after the lockout ({b.prompts})")
        boot_checks(t, b)
        tpm.clear_lockout()
        run(["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={TPM}", "--server", f"type=tcp,port={port}",
             "--ctrl", f"type=tcp,port={port + 1}", "--flags", "startup-clear", "-d", "--pid", f"file={pid}"])
        try:
            run(["tpm2_dictionarylockout", "-T", tcti, "-s", "-n", "32", "-t", "600", "-l", "600"])
        finally:
            subprocess.run(["kill", pid.read_text().strip()], check=False)

    elif scenario == "notpm":
        img = image(False)
        vm = VM(t, scenario, disks=disks(), kernel=img["kernel"], initrd=img["initrd"], append=base).start()
        b = follow_boot(vm, answers={"luks": [PW, PW, PW], "gpg": ["gpgpass"]})
        t.check("tpm2pin" not in b.prompts, "no TPM2 PIN prompt without a TPM")
        t.check("luks" in b.prompts, f"TPM2 volumes fall back to the passphrase ({b.prompts})")
        boot_checks(t, b)
    else:
        t.check(False, f"unknown scenario {scenario}")
    t.finish()


if __name__ == "__main__":
    main()
