#!/usr/bin/env python3
# vim: softtabstop=4 shiftwidth=4 expandtab
"""
UEFI boot from an NVMe SSD with 4 KiB sectors, without any crypttab.

The NVMe disk has an EFI system partition and one LUKS2 partition
(zroot-luks) with 4 KiB encryption sectors and the TRIM and work-queue flags
stored in its header. The ZFSBootMenu EFI image (built by the container) is
the firmware's fallback loader EFI/BOOT/BOOTX64.EFI; its command line names the
volume with zbm.luks=UUID=. The ZFS 2.4.4 boot environment unlocks the volume
with systemd's luks.uuid=, luks.key= and luks.crypttab=no parameters.

Scenarios (firmware.py <scenario>):
  setup     partition on the host, create LUKS and the pool in the recovery shell
  boot      firmware -> ZFSBootMenu (one passphrase) -> boot environment
  usbkey    zbm.luks_key= names a key file on a FAT USB stick: no prompt
  usblate   the stick is inserted while ZFSBootMenu waits for it: no prompt
  nostick   no stick: ZFSBootMenu waits, then asks for the passphrase
"""

import os
import re
import shutil
import sys
import time

from zbmtest import (FAST_PBKDF, GPT_ESP, GPT_LINUX, OVMF_VARS, PW, WORK, Test, VM, check_be,
                     check_teardown, follow_boot, gpt_disk, payload_disk, run, vfat_disk, zbm_image)

D = WORK / "disks" / "firmware"
NVME = D / "nvme.img"
UUIDF = D / "uuid"
STICK = D / "usb.img"
SECTOR = 4096
ESP_SECTORS = 32768          # 128 MiB in 4 KiB sectors
ESP_START = 256


def make_nvme():
    D.mkdir(parents=True, exist_ok=True)
    if NVME.exists():
        NVME.unlink()
    gpt_disk(NVME, 2 << 30, SECTOR, [("ESP", GPT_ESP, ESP_START, ESP_SECTORS),
                                     ("zroot-luks", GPT_LINUX, ESP_START + ESP_SECTORS, None)])


def write_esp(efi):
    esp = D / "esp.img"
    vfat_disk(esp, "EFI", {"EFI/BOOT/BOOTX64.EFI": efi.read_bytes()},
              size=f"{ESP_SECTORS * SECTOR // (1 << 20)}M", sector=SECTOR)
    with open(NVME, "r+b") as out, open(esp, "rb") as src:
        out.seek(ESP_START * SECTOR)
        shutil.copyfileobj(src, out)
    esp.unlink()


def ovmf():
    path = D / "OVMF_VARS.fd"
    shutil.copy(OVMF_VARS, path)
    return path


def setup(t):
    make_nvme()
    img = zbm_image("auto")
    vm = VM(t, "setup", disks=[(payload_disk(), "payload")], kernel=img["kernel"], initrd=img["initrd"],
            append="console=ttyS0 zbm.luks=off zbm.retry_delay=3", nvme=(NVME, SECTOR)).start()
    vm.recovery_shell()
    vm.send_functions()
    rc, out = vm.sh("cat /sys/block/nvme0n1/queue/logical_block_size", show=True)
    t.check(out.strip().endswith("4096"), "NVMe namespace has 4096-byte logical blocks")
    vm.must(f"printf '%s' {PW} | cryptsetup luksFormat -q --type luks2 --sector-size 4096 {FAST_PBKDF} "
            "--key-file=- /dev/disk/by-partlabel/zroot-luks")
    vm.must("UUID=$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)")
    vm.must(f"printf '%s' {PW} | cryptsetup open --allow-discards --perf-no_read_workqueue "
            "--perf-no_write_workqueue --persistent --key-file=- /dev/disk/by-partlabel/zroot-luks luks-$UUID")
    rc, out = vm.sh("cryptsetup luksDump /dev/disk/by-partlabel/zroot-luks | grep -E 'Flags|sector'", show=True)
    t.check("allow-discards" in out and "no-read-workqueue" in out and "no-write-workqueue" in out,
            "activation flags stored in the LUKS2 header")
    vm.must("zpool create -f -o ashift=12 -o autotrim=on -o compatibility=openzfs-2.4-linux "
            "-O compression=lz4 -O acltype=posixacl -O xattr=sa -m none zroot /dev/mapper/luks-$UUID")
    vm.must("mkbe zroot")
    vm.must('zfs set org.zfsbootmenu:commandline="console=ttyS0 loglevel=4 luks.crypttab=no luks.uuid=$UUID '
            'luks.key=$UUID=/etc/zbmtest/be.key luks.options=$UUID=discard,no-read-workqueue,no-write-workqueue" '
            'zroot/ROOT/be')
    rc, out = vm.sh("echo UUID=$UUID", show=True)
    m = re.search(r"UUID=([0-9a-f-]{36})", out)
    t.check(m is not None, "volume UUID recorded")
    UUIDF.write_text(m.group(1) if m else "")
    vm.must("finish")
    vm.poweroff()


def efi_image(uuid, usb):
    cmd = f"console=ttyS0 loglevel=4 zbm.timeout=3 zbm.luks=UUID={uuid}"
    if usb:
        cmd += " zbm.luks_key=/keys/zroot.key:LABEL=ZBMKEY"
    return zbm_image("firmware-usb" if usb else "firmware", efi=True, cmdline=cmd)["efi"]


def checks(t, b, uuid):
    pools = b.pools()
    t.check(pools.get("zroot") == "ONLINE", f"ZFSBootMenu imported zroot: {pools.get('zroot')}")
    check_teardown(t, b)
    check_be(t, b, "zroot", ["zroot"])
    cmd = b.be_lines.get("BE-CMDLINE", [""])[0]
    t.check("luks.crypttab=no" in cmd and f"luks.uuid={uuid}" in cmd and "root=zfs:zroot/ROOT/be" in cmd,
            "boot environment command line has the luks.* parameters and root=zfs:")
    dm = "\n".join(l for l in b.be.splitlines() if "BE-DM| " in l)
    t.check("sector_size:4096" in dm, "boot environment: 4096-byte encryption sectors")
    t.check("allow_discards" in dm and "no_read_workqueue" in dm and "no_write_workqueue" in dm,
            "boot environment: TRIM and no-workqueue flags active")
    t.check("be-luks-prompt" not in b.prompts, "the boot environment did not ask for a passphrase")


def main():
    scenario = sys.argv[1]
    t = Test(f"firmware-{scenario}")
    if scenario == "setup":
        setup(t)
        t.finish()
    uuid = UUIDF.read_text().strip() if UUIDF.exists() else ""
    if not uuid:
        t.check(False, "run 'firmware.py setup' first")
        t.finish()
    usb = scenario in ("usbkey", "usblate", "nostick")
    write_esp(efi_image(uuid, usb))
    vfat_disk(STICK, "ZBMKEY", {"keys/zroot.key": PW.encode()})
    sticks = [(STICK, "usb0")] if scenario == "usbkey" else []
    vm = VM(t, scenario, nvme=(NVME, SECTOR), ovmf_vars=ovmf(), usb=sticks, mem=4096).start()

    if scenario == "boot":
        b = follow_boot(vm)
        t.check(b.prompts == ["luks"], f"one passphrase prompt in ZFSBootMenu ({b.prompts})")
    elif scenario == "usbkey":
        b = follow_boot(vm)
        t.check("luks" not in b.prompts, f"no passphrase prompt: the key file on the stick was used ({b.prompts})")
    elif scenario == "usblate":
        vm.expect(r"Insert the key device", timeout=2400)
        t.step("ZFSBootMenu waits for the key device")
        time.sleep(1)
        vm.hotplug_usb(STICK, "usb0")
        b = follow_boot(vm)
        t.check("luks" not in b.prompts, f"no passphrase prompt after the stick arrived ({b.prompts})")
    elif scenario == "nostick":
        vm.expect(r"Insert the key device", timeout=2400)
        t.step("ZFSBootMenu waits for the key device, which never arrives")
        b = follow_boot(vm)
        t.check(b.prompts == ["luks"], f"after the wait, one passphrase prompt ({b.prompts})")
    else:
        t.check(False, f"unknown scenario {scenario}")
        t.finish()
    checks(t, b, uuid)
    t.finish()


if __name__ == "__main__":
    main()
