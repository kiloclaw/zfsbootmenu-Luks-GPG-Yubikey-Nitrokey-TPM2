# Gentoo on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install Gentoo Linux from scratch on a **single NVMe SSD** with a **ZFS root pool**. The
system boots with the ZFSBootMenu build from this repository (branch `claude/zbm-refactor`). It covers both the
**systemd** and the **OpenRC** profile, with Gentoo's distribution kernel and an initramfs built by dracut.

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Gentoo's initramfs then unlocks the pool again
with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from the documentation of Gentoo and of this repository, and from the sources of the
> Gentoo packages it uses. The project has not run it from start to finish. `claude/zbm-refactor` is a development
> branch, not a release.
> The project's VM tests boot ZFSBootMenu images built with dracut and a test system whose initramfs is built with
> dracut and systemd, with unencrypted pools, ZFS native encryption and LUKS; they do not install Gentoo. A dracut
> initramfs without systemd, which the OpenRC profile uses, is not covered by them.
> Keep a live medium at hand until the system has booted several times, and read
> [What the project has tested](#what-the-project-has-tested) before you start.

## Contents

- [Overview](#overview)
  - [Choosing an option](#choosing-an-option)
  - [Boot process](#boot-process)
  - [Where the keys are](#where-the-keys-are)
  - [What the project has tested](#what-the-project-has-tested)
- [Requirements](#requirements)
  - [Hardware](#hardware)
  - [Installation media](#installation-media)
  - [Conventions](#conventions)
- [Preparing the NVMe SSD](#preparing-the-nvme-ssd)
  - [Identifying the SSD](#identifying-the-ssd)
  - [Setting variables](#setting-variables)
  - [Optional: switching to 4 KiB sectors](#optional-switching-to-4-kib-sectors)
  - [Erasing the SSD](#erasing-the-ssd)
  - [Partitioning with gdisk](#partitioning-with-gdisk)
  - [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container)
  - [Creating the ZFS pool](#creating-the-zfs-pool)
  - [Creating the datasets](#creating-the-datasets)
  - [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties)
  - [Mounting the pool below /mnt](#mounting-the-pool-below-mnt)
- [Installing the base system](#installing-the-base-system)
  - [Downloading and unpacking the stage file](#downloading-and-unpacking-the-stage-file)
  - [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file)
  - [Chrooting](#chrooting)
  - [Configuring Portage](#configuring-portage)
- [Kernel and ZFS](#kernel-and-zfs)
- [Option C only: creating the LUKS key file](#option-c-only-creating-the-luks-key-file)
- [Configuring the initramfs](#configuring-the-initramfs)
- [Configuring the system](#configuring-the-system)
- [Installing ZFSBootMenu](#installing-zfsbootmenu)
  - [Dependencies](#dependencies)
  - [Building and installing](#building-and-installing)
  - [Configuring generate-zbm](#configuring-generate-zbm)
  - [Choosing the runtime](#choosing-the-runtime)
  - [Building and checking the image](#building-and-checking-the-image)
  - [Creating the firmware boot entries](#creating-the-firmware-boot-entries)
- [Finalizing the installation](#finalizing-the-installation)
- [First boot](#first-boot)
- [Usage](#usage)
  - [Updating the system](#updating-the-system)
  - [Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image)
  - [Boot environments, snapshots and rollback](#boot-environments-snapshots-and-rollback)
  - [Changing the passphrase](#changing-the-passphrase)
  - [Replacing the key file](#replacing-the-key-file)
  - [Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header)
  - [Scrubbing and trimming](#scrubbing-and-trimming)
  - [Updating ZFSBootMenu](#updating-zfsbootmenu)
  - [Relaxing the pool compatibility](#relaxing-the-pool-compatibility)
- [Troubleshooting](#troubleshooting)
  - [The firmware does not find ZFSBootMenu](#the-firmware-does-not-find-zfsbootmenu)
  - [ZFSBootMenu finds no pool](#zfsbootmenu-finds-no-pool)
  - [Option C: ZFSBootMenu never asks for the passphrase (the image lacks cryptsetup)][no-cryptsetup]
  - [The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock]
  - [ZFSBootMenu lists no kernels, or no boot environment](#zfsbootmenu-lists-no-kernels-or-no-boot-environment)
  - [The pool is refused: wrong host ID, or last used by another system][hostid-refused]
  - [The recovery shell and its tools](#the-recovery-shell-and-its-tools)
  - [Repairing the system from the live medium](#repairing-the-system-from-the-live-medium)
  - [Continuing after a reboot of the live system](#continuing-after-a-reboot-of-the-live-system)
  - [installkernel stops inside the chroot](#installkernel-stops-inside-the-chroot)
  - [zfs.ko is missing after a kernel update](#zfsko-is-missing-after-a-kernel-update)
  - [generate-zbm builds the image from the wrong kernel](#generate-zbm-builds-the-image-from-the-wrong-kernel)
- [Security notes](#security-notes)
- [See also](#see-also)

[no-cryptsetup]: #option-c-zfsbootmenu-never-asks-for-the-passphrase-the-image-lacks-cryptsetup
[second-unlock]: #the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs
[hostid-refused]: #the-pool-is-refused-wrong-host-id-or-last-used-by-another-system

## Overview

### Choosing an option

Two choices are made at the start and kept to the end: the **option** (A, B or C) and the Gentoo **profile**
(systemd or OpenRC).

| | Option A | Option B | Option C |
|---|---|---|---|
| Pool | not encrypted | ZFS native encryption (`aes-256-gcm`) | inside a LUKS2 container |
| Passphrase typed | never | once, in ZFSBootMenu | once, in ZFSBootMenu |
| Gentoo's initramfs unlocks with | nothing | key file `/etc/zfs/zroot.key` | `/etc/cryptsetup-keys.d/zroot.key` |
| Partition 2 type | `BF00` | `BF00` | `8309` |
| Readable without the passphrase | everything | names, sizes, properties (see below) | the size and LUKS header |
| Extra software | none | none | `cryptsetup` (live system, Gentoo, ZFSBootMenu's image) |

Option B leaves the names, sizes and properties of datasets and snapshots readable, but not file contents or file
names; [Security notes](#security-notes) has the details.

Option B stores the **passphrase itself** in its key file: OpenZFS has one key per encryption root, and ZFSBootMenu
asks for it whenever the file named by `keylocation` is not in its own image. Option C stores a **different, random**
key in a second LUKS key slot. The two can be combined; this guide does not.

The profile changes a few steps. Where it does, the text is marked **systemd.** or **OpenRC.**:

| | systemd profile | OpenRC profile |
|---|---|---|
| Stage file | `stage3-amd64-systemd-*.tar.xz` | `stage3-amd64-openrc-*.tar.xz` |
| Kernel file in `/boot` | `kernel-<version>` | `vmlinuz-<version>` |
| Kernel installer | systemd's `kernel-install`, through `sys-kernel/installkernel` | Gentoo's `installkernel` script |
| EFI stub for ZFSBootMenu | `sys-apps/systemd[boot]` | `sys-apps/systemd-utils[boot,kernel-install]` |
| Option C, second unlock | `luks.*` kernel parameters, no crypttab | `rd.luks.uuid=` and a line in `/etc/crypttab` |

### Boot process

1. The UEFI firmware starts **ZFSBootMenu**, one EFI file on the EFI system partition (ESP).
2. **Option C**: ZFSBootMenu reads `zbm.luks=` from its kernel command line, asks for the **LUKS passphrase** and
   opens the container.
3. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/gentoo` with a countdown of 10 seconds.
   When it ends, the pool's `bootfs` is booted; a key press opens the menu instead.
4. **Option B**: the `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in
   ZFSBootMenu's image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime this
   prompt comes **after** the countdown.
5. ZFSBootMenu loads Gentoo's kernel and initramfs from `/boot` **inside the boot environment** and starts the kernel
   with `kexec`. It passes the command line from the ZFS property `org.zfsbootmenu:commandline`, the root argument
   `root=ZFS=zroot/ROOT/gentoo` and the host ID. **Option C**: before `kexec` it exports the pool and closes the
   container, which wipes the volume key from memory.
6. Gentoo's initramfs (dracut) starts in the new kernel:
   - **Option A**: it imports the pool and mounts the root file system.
   - **Option B**: it imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the initramfs**,
     without asking.
   - **Option C**: it unlocks the container again with the key file `/etc/cryptsetup-keys.d/zroot.key` **inside the
     initramfs**, without asking, then imports the pool.

Why a second unlock (Options B and C): `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. Gentoo's own initramfs must unlock the pool again, exactly as it would without ZFSBootMenu.
ZFSBootMenu does not replace Gentoo's initramfs.

### Where the keys are

Option A: there are none.

| Key | Option | Stored where | Used by |
|---|---|---|---|
| ZFS passphrase | B | your memory | ZFSBootMenu at every boot; a live system for repairs |
| The same passphrase in `/etc/zfs/zroot.key` | B | in the pool (see below) | Gentoo's initramfs |
| LUKS passphrase (key slot 0) | C | your memory | ZFSBootMenu at every boot; a live system for repairs |
| Random key file `/etc/cryptsetup-keys.d/zroot.key` (slot 1) | C | in the pool (see below) | Gentoo's initramfs |

"In the pool" means twice: the file itself in `/etc`, and a copy of it in Gentoo's initramfs in `/boot`. Both
directories are inside the encrypted pool. Gentoo's initramfs uses the key file after `kexec`.

The key file in Gentoo's initramfs is safe **only because** the file and the initramfs both live inside the encrypted
pool (`/etc` and `/boot`); they cannot be read before the passphrase has been typed. ZFSBootMenu's own image lives on
the unencrypted ESP and never contains a key.

### What the project has tested

From [Testing](../../luks/testing.rst):

- The VM tests boot ZFSBootMenu images built **with dracut** by the project's build container (Ubuntu 26.10,
  OpenZFS 2.4.4), with `hostonly="no"`, as this guide builds them on Gentoo.
- Their boot environment is a test system, not an installed distribution: the Ubuntu kernel and a **dracut
  initramfs with systemd**, the ZFS `zfs` module and systemd-cryptsetup, which unlocks with key files and
  `luks.*` or `rd.luks.*` parameters, without a crypttab. That is the arrangement of Option C on the **systemd**
  profile; the test group `firmware` boots it from an NVMe disk with 4 KiB sectors, one passphrase prompt and no
  second prompt.
- The suite runs with the **Rust runtime** by default. This guide's default, the **shell runtime**, is not part of
  the regular runs; every test was last run with it on 20 October 2026.
- **Option A**: the group `runtime` boots unencrypted pools. In the regular suite only its Rust runtime tests run;
  its shell runtime tests (`runtime-legacy-*`) run only when named, last on 20 October 2026.
- **Option B is tested in part**: the group `complex` has a root with ZFS native encryption, and checks that the key
  is asked by ZFSBootMenu **and by the boot environment**: the test system's initramfs holds no key file. The single
  prompt of Option B (the key file in Gentoo's initramfs) is upstream ZFSBootMenu's documented design and is not
  tested by the project.
- **Not tested**: a dracut initramfs **without systemd** (the OpenRC profile, and Option C's crypttab way on it);
  Gentoo's kernels, ZFS packages and dracut version; an image built on an installed distribution; Secure Boot; real
  hardware (the tests run in QEMU).

## Requirements

### Hardware

- An x86_64 (amd64) computer with an **NVMe SSD** that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware: the ZFSBootMenu image built here is not signed. How to check is in
  [Preparing the NVMe SSD](#preparing-the-nvme-ssd).
- An internet connection.
- Room on the SSD for the build: about 1 GB for the clone of this repository, plus Cargo's build directory, whose
  size was not measured for this guide.

### Installation media

Use the **Gentoo minimal installation CD** for amd64, `install-amd64-minimal-<date>.iso`, from
<https://www.gentoo.org/downloads/>. Since spring 2025 its build specification adds `sys-fs/zfs` to the live
kernel (Gentoo's releng repository, file `releases/specs/amd64/installcd-stage2-minimal.spec`: commit 6cbab2c,
"Enable ZFS on new installcd for amd64", March 2025, and the fix 07b7d1e, merged in April 2025). Its stage 1
specification adds `gdisk` (sys-apps/gptfdisk), `cryptsetup`, `nvme-cli`, `dosfstools`, `efibootmgr`, `wget` and
`gpg`. Older guides in this repository, among them [zfs-luks-v4.md](zfs-luks-v4.md), say that the minimal CD has no
ZFS; that was true of the images built before these changes.

| Program | Needed for | Options |
|---|---|---|
| ZFS kernel module and `zpool`, `zfs`, `zgenhostid` | creating the pool | A B C |
| `gdisk` | partitioning | A B C |
| `mkfs.vfat` (dosfstools) | formatting the ESP | A B C |
| `wipefs`, `blkdiscard`, `lsblk`, `blockdev` (util-linux) | erasing and checking | A B C |
| `cryptsetup` 2.4 or later | the LUKS container | C |
| `nvme` (nvme-cli) | optional: identifying the SSD, 4 KiB sectors | A B C |
| `wget`, `gpg`, `tar` | downloading and checking the stage file | A B C |

> **Note**
> The Gentoo **LiveGUI** image dropped ZFS in January 2026 (releng commit 71faf2c, "LiveGUI: Disable zfs"); do not
> use it for this guide. That the current minimal CD's ZFS works was read from its build specification, not tried:
> the check below confirms it.
>
> If `modprobe zfs && zfs version` fails, use the **Ubuntu Desktop live ISO**, 24.04 or later, instead: choose
> *Try Ubuntu*, open a terminal, and run
>
> ```
> ubuntu@ubuntu:~$ sudo -i
> root # apt update
> root # apt install zfsutils-linux gdisk cryptsetup nvme-cli
> ```
>
> `wget`, `gpg`, `tar` and `mkfs.vfat` are on it already. Then continue with the same commands. Any other UEFI live
> system with OpenZFS 2.2 or later, `gdisk`, `wget`, `gpg` and (Option C) `cryptsetup` works as well.

The live system's OpenZFS must be **2.2 or later**: the pool is created with `compatibility=openzfs-2.2-linux`, and
only an OpenZFS that knows that set can create it.

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new Gentoo system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |
| `<Enter>` | in a transcript: press Enter without typing anything |
| **Option A.** / **Option B.** / **Option C.** | the next paragraph and commands are for that option only |
| **All options.** | the text is for every option again |
| **Both profiles.** | the text is for both profiles again |
| **systemd.** / **OpenRC.** | the next paragraph and commands are for that profile only |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Preparing the NVMe SSD

Boot the minimal CD in **UEFI** mode (choose its UEFI entry in the firmware's boot menu). It starts a root shell and
configures wired networking with DHCP; for wireless networks see the Gentoo Handbook, *Configuring the network*.

**Check:**

```
root # ls /sys/firmware/efi
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
root # modprobe zfs && zfs version
root # command -v gdisk mkfs.vfat blkdiscard wipefs cryptsetup nvme wget gpg
root # findmnt -R /mnt
root # ping -c 2 gentoo.org
```

- `ls` lists files; otherwise the live system booted in legacy BIOS mode.
- The last number of `od` is `0`: Secure Boot is disabled. `1` means enabled; disable it in the firmware setup.
- `zfs version` prints two lines, `zfs-` (the tools) and `zfs-kmod-` (the kernel module), with version 2.2 or later.
  If `modprobe` or `zfs` fails, switch to the live system named in the Note of
  [Installation media](#installation-media).
- `command -v` prints a path for every program (`cryptsetup` is needed for Option C only). gdisk prints its version
  on its first line when it starts; version 1.0.9 or later is assumed below.
- `findmnt -R /mnt` prints nothing: the live system uses nothing below `/mnt`, where the new system will be mounted.
  (An empty directory `/mnt/gentoo` is harmless; the root dataset is mounted over it.)
- `ping` gets answers.

### Identifying the SSD

Why: the next steps destroy everything on the SSD you name; it must be the right one.

```
root # nvme list
```

Each line shows a namespace such as `/dev/nvme0n1` with its model, serial number and size. `nvme0` is the
controller and `n1` its first namespace; partitions are `/dev/nvme0n1p1`, `/dev/nvme0n1p2`. Without `nvme-cli`,
`lsblk -d -o NAME,SIZE,MODEL,SERIAL,TRAN` shows the same (`TRAN` is `nvme`).

This guide uses the kernel name `/dev/nvme0n1`: `gdisk`, `blkdiscard` and `efibootmgr -d` take it, and a computer
with one NVMe SSD has only `nvme0n1`. Kernel names can change between boots when a computer has **several** NVMe
SSDs. In that case, compare the serial number with `ls -l /dev/disk/by-id/ | grep nvme | grep -v -- -part` before
every destructive step and after every reboot of the live system. ZFS finds its disks by their labels, not by the
name used at `zpool create`, so a changed name does not stop the pool from importing later.

### Setting variables

Why: every later command uses these names, so that they can be copied as they are.

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
root # export POOL=zroot
root # export BE=gentoo
```

**systemd.**

```
root # export INIT=systemd
```

**OpenRC.**

```
root # export INIT=openrc
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the ESP (NVMe partitions have a `p` before the number) |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `gentoo` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}`; the name is free |
| `INIT` | `systemd` or `openrc` | A B C | the Gentoo profile |
| `POOL_DEV` | A, B: `${POOL_PART}`; C: `/dev/mapper/luks-${LUKS_UUID}` | A B C | the device `zpool create` gets |
| `LUKS_UUID` | the output of `cryptsetup luksUUID` | C | the UUID of the LUKS container |

`POOL_DEV` is set later, in [Creating the ZFS pool](#creating-the-zfs-pool) (Options A and B) or in
[Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container), which also sets
`LUKS_UUID` after `luksFormat`.

A few paths are written literally, because **FILE** blocks cannot expand variables. If you change `POOL`, change
`zroot` in these paths too:

| Path | Option | What |
|---|---|---|
| `/etc/zfs/zroot.key` | B | the ZFS passphrase file; `keylocation=file:///etc/zfs/zroot.key` |
| `/etc/cryptsetup-keys.d/zroot.key` | C | the random LUKS key file for the second unlock |
| `/dev/mapper/luks-<LUKS UUID>` | C | the opened container; the name systemd, dracut and ZFSBootMenu use |
| `/etc/hostid` | A B C | the host ID, the same on the live system, in Gentoo and in both initramfs images |
| `/boot/efi` | A B C | mount point of the ESP in Gentoo |
| `/boot/efi/EFI/zbm/vmlinuz.EFI`, `vmlinuz-backup.EFI` | A B C | ZFSBootMenu's image and its backup |
| `/usr/src/zbm` | A B C | the clone of this repository |

> **Tip**
> If the terminal is closed, set these variables again before continuing; for Option C, set `LUKS_UUID` and
> `POOL_DEV` again as well (see
> [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container)). If the **live system
> is rebooted** once the pool exists, more is lost: its host ID and, for Option B, the passphrase file lived only in
> its memory. Follow
> [Continuing after a reboot of the live system](#continuing-after-a-reboot-of-the-live-system)
> before continuing.

**Check:**

```
root # lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"
root # echo "${INIT}"
```

The model and serial number are those of the SSD to erase, and `INIT` is `systemd` or `openrc`.

### Optional: switching to 4 KiB sectors

Why: many NVMe SSDs ship with 512-byte logical sectors and also support 4096-byte sectors, which can be slightly
faster. Show the supported formats:

```
root # nvme id-ns -H "${DISK}" | grep -E '^LBA Format'
```

Example output:

```
LBA Format  0 : Metadata Size: 0   bytes - Data Size: 512 bytes - Relative Performance: 0x2 Good (in use)
LBA Format  1 : Metadata Size: 0   bytes - Data Size: 4096 bytes - Relative Performance: 0x1 Better
```

If a `4096 bytes` format with `Metadata Size: 0` is listed and not `(in use)`, switch to it. Use its number with
`--lbaf` (here `1`):

> **Warning**
> `nvme format` erases the whole SSD immediately.

```
root # nvme format "${DISK}" --lbaf=1 --force
root # nvme id-ns -H "${DISK}" | grep 'in use'
```

If only 512-byte formats are listed, skip this step. The rest of the guide works either way: ZFS gets `ashift=12`
and, for Option C, LUKS gets `--sector-size 4096`, so both use 4 KiB blocks on either format. On a 4 KiB namespace
gdisk counts in 4096-byte sectors: its sector numbers are about eight times smaller than in the transcript below,
and it prints `Partitions will be aligned on 256-sector boundaries` (1 MiB, as with `2048-sector` on 512-byte
sectors). The answers you type are the same.

**Check:** the `grep 'in use'` line shows `Data Size: 4096 bytes`, or you skipped this step.

### Erasing the SSD

Why: old ZFS labels, LUKS headers and file system signatures confuse `zpool create`, `cryptsetup` and the live
system; discarding also tells the SSD that every block is free.

> **Warning**
> The commands in this section destroy everything on `${DISK}` at once and cannot be undone. Check the output of
> `lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"` one more time.

```
root # lsblk -o NAME,SIZE,FSTYPE,MOUNTPOINTS "${DISK}"
root # swapoff -a
root # zpool export -a
root # for part in "${DISK}"p*; do [ -b "${part}" ] && zpool labelclear -f "${part}"; done
root # for part in "${DISK}"p*; do [ -b "${part}" ] && wipefs -a "${part}"; done
root # blkdiscard -f "${DISK}"
root # wipefs -a "${DISK}"
```

- The first command must show no mount point; unmount anything the live system mounted from the SSD. `swapoff -a`
  and `zpool export -a` release a swap partition or an old pool that the live system took into use; "no pools
  available to export" is not an error.
- The two loops clear signatures **inside** old partitions: ZFS keeps four labels per disk and LUKS2 two headers,
  and wiping only the partition table would leave them where a new partition may find them. On an SSD without
  partitions the loops do nothing. `zpool labelclear` prints an error for a partition that holds no ZFS label; that
  is expected.
- `blkdiscard -f` discards every block of the namespace. If it fails with *Operation not supported*, the SSD or the
  controller does not pass discards; continue with the next command.
- If you ran `nvme format` in the optional step, the SSD is already empty; the commands are harmless.

**Check:**

```
root # wipefs "${DISK}"
root # lsblk "${DISK}"
```

`wipefs` prints nothing, and `lsblk` shows the SSD without partitions.

### Partitioning with gdisk

Why: the firmware needs a FAT partition for ZFSBootMenu (the ESP); everything else is one partition for the pool or
the LUKS container.

| Partition | Device | Size | gdisk type | Name | Use |
|---|---|---|---|---|---|
| 1 | `${ESP_PART}` (`nvme0n1p1`) | 1 GiB | `EF00` EFI system partition | `EFI` | the ESP: FAT32, at `/boot/efi` |
| 2, Options A and B | `${POOL_PART}` (`nvme0n1p2`) | the rest | `BF00` Solaris root | `zroot` | the ZFS pool |
| 2, Option C | `${POOL_PART}` (`nvme0n1p2`) | the rest | `8309` Linux LUKS | `zroot-luks` | the LUKS2 container |

The ESP holds only ZFSBootMenu: its image, the backup image and a copy on the fallback path. Gentoo's kernels and
initramfs images are **never** stored on it; they live in `/boot` inside the pool. 1 GiB leaves room for several
images. ZFS ignores the type code `BF00`; it is a label for people and tools.

The keystrokes:

| Step | Command | Answers |
|---|---|---|
| New, empty GPT | `o` | `y` |
| Partition 1, the ESP | `n` | `<Enter>`, `<Enter>`, `+1G`, `ef00` |
| Partition 2, the rest | `n` | `<Enter>`, `<Enter>`, `<Enter>`, then `bf00` (Options A, B) or `8309` (Option C) |
| Name partition 1 | `c` | `1`, `EFI` |
| Name partition 2 | `c` | `2`, then `zroot` (Options A, B) or `zroot-luks` (Option C) |
| Print and compare with the table above | `p` | |
| Write and leave | `w` | `y` |

gdisk changes nothing on the SSD until `w`; `q` leaves without writing. The transcript is from a 1 TB SSD with
512-byte sectors; the numbers on your SSD differ. **Never type the sector numbers shown**: press Enter for the
defaults. Since gdisk 1.0.9, the default end of a new partition is aligned, so partition 2 gets a size that is a
whole number of MiB, which Option C's 4 KiB encryption sectors need.

**Options A and B:**

```
root # gdisk "${DISK}"
GPT fdisk (gdisk) version 1.0.10

Partition table scan:
  MBR: not present
  BSD: not present
  APM: not present
  GPT: not present

Creating new GPT entries in memory.

Command (? for help): o
This option deletes all partitions and creates a new protective MBR.
Proceed? (Y/N): y

Command (? for help): n
Partition number (1-128, default 1): <Enter>
First sector (34-1953525134, default = 2048) or {+-}size{KMGTP}: <Enter>
Last sector (2048-1953525134, default = 1953523711) or {+-}size{KMGTP}: +1G
Current type is 8300 (Linux filesystem)
Hex code or GUID (L to show codes, Enter = 8300): ef00
Changed type of partition to 'EFI system partition'

Command (? for help): n
Partition number (2-128, default 2): <Enter>
First sector (34-1953525134, default = 2099200) or {+-}size{KMGTP}: <Enter>
Last sector (2099200-1953525134, default = 1953523711) or {+-}size{KMGTP}: <Enter>
Current type is 8300 (Linux filesystem)
Hex code or GUID (L to show codes, Enter = 8300): bf00
Changed type of partition to 'Solaris root'

Command (? for help): c
Partition number (1-2): 1
Enter name: EFI

Command (? for help): c
Partition number (1-2): 2
Enter name: zroot

Command (? for help): p
Disk /dev/nvme0n1: 1953525168 sectors, 931.5 GiB
Model: YOUR SSD MODEL
Sector size (logical/physical): 512/512 bytes
Disk identifier (GUID): 01234567-89AB-CDEF-0123-456789ABCDEF
Partition table holds up to 128 entries
Main partition table begins at sector 2 and ends at sector 33
First usable sector is 34, last usable sector is 1953525134
Partitions will be aligned on 2048-sector boundaries
Total free space is 3437 sectors (1.7 MiB)

Number  Start (sector)    End (sector)  Size       Code  Name
   1            2048         2099199   1024.0 MiB  EF00  EFI
   2         2099200      1953523711   930.5 GiB   BF00  zroot

Command (? for help): w

Final checks complete. About to write GPT data. THIS WILL OVERWRITE EXISTING
PARTITIONS!!

Do you want to proceed? (Y/N): y
OK; writing new GUID partition table (GPT) to /dev/nvme0n1.
The operation has completed successfully.
```

**Option C** types `8309` instead of `bf00` for partition 2:

```
Hex code or GUID (L to show codes, Enter = 8300): 8309
Changed type of partition to 'Linux LUKS'
```

and `zroot-luks` instead of `zroot` as its name:

```
Command (? for help): c
Partition number (1-2): 2
Enter name: zroot-luks
```

The `p` listing then ends with `8309  zroot-luks`.

What you may see that the transcript does not show:

- The opening lines differ when an old table is still found (`MBR: protective`, `GPT: present` or `GPT: damaged`);
  `o` replaces whatever is there.
- If gdisk ends with "Warning: The kernel is still using the old partition table", run `partprobe "${DISK}"`, or
  reboot the live system and set the variables again.
- A mistake before `w` costs nothing: `q` quits without saving, `d` deletes a partition.

**All options.** Make the kernel see the partitions, and format the ESP:

```
root # udevadm settle
root # mkfs.vfat -F 32 -n EFI "${ESP_PART}"
```

**Check:**

```
root # gdisk -l "${DISK}"
root # lsblk -o NAME,SIZE,FSTYPE,PARTLABEL "${DISK}"
root # echo $(( $(blockdev --getsize64 "${POOL_PART}") % 1048576 ))
root # wipefs "${POOL_PART}"
```

- `gdisk -l` prints the same table as `p` did: partition 1 with code `EF00` and 1024.0 MiB, partition 2 with code
  `BF00` (Options A and B) or `8309` (Option C).
- `lsblk` lists `nvme0n1p1` (1G, `vfat`, `EFI`) and `nvme0n1p2` (no file system, `zroot` or `zroot-luks`).
- The `echo` prints `0`: the size of partition 2 is a whole number of MiB. If it prints another number, your gdisk is
  older than 1.0.9. For Options A and B that does no harm. For Option C, start `gdisk "${DISK}"` again, delete
  partition 2 (`d`, `2`) and create it again (`n`, `<Enter>`, `<Enter>`), answering the `Last sector` prompt with
  the number that `echo $(( (LAST + 1) / 2048 * 2048 - 1 ))` prints, where `LAST` is the larger number in the
  prompt's range (on a 4 KiB namespace, use `256` instead of `2048`). Then type `8309`, name it with `c`, `2`,
  `zroot-luks`, write with `w`, `y`, run `udevadm settle` and repeat this Check.
- `wipefs` prints nothing. If it prints a line, run `wipefs -a "${POOL_PART}"`.

### Option C only: creating the LUKS container

Skip this section for Options A and B.

Why: the container encrypts everything in partition 2; the pool is created inside it.

Create the container with 4 KiB encryption sectors. Type `YES` in capitals when asked, then the passphrase twice:

```
root # cryptsetup luksFormat --type luks2 --sector-size 4096 "${POOL_PART}"
```

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it
> before any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

If `luksFormat` prints "Device size is not aligned to requested sector size.", go back to the Check of
[Partitioning with gdisk](#partitioning-with-gdisk).

Open the container as `luks-<UUID>`, the name systemd, dracut and ZFSBootMenu use, and store the TRIM flag in the
LUKS2 header with `--persistent`, so that TRIM passes through wherever the container is opened (ZFSBootMenu,
Gentoo's initramfs, a live system), without any crypttab option:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open --allow-discards --persistent "${POOL_PART}" "luks-${LUKS_UUID}"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
```

> **Note**
> Passing TRIM through reveals which blocks are unused, but not their content. This is the usual trade-off for
> SSDs; leave out `--allow-discards` if that matters to you, and set `autotrim=off` on the pool.

Optionally, add `--perf-no_read_workqueue --perf-no_write_workqueue` to the `cryptsetup open` line: they process
encryption directly instead of in kernel work queues, which lowers latency on fast NVMe SSDs, and `--persistent`
stores them as well.

After a reboot of the live system, open the container again like this; the stored flags are applied by themselves:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

**Check:**

```
root # cryptsetup luksDump "${POOL_PART}" | grep -E '^Flags|sector'
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
root # echo "${POOL_DEV}"
```

The header shows `Flags: allow-discards` and `sector: 4096 [bytes]`; the status shows `type: LUKS2`,
`sector size: 4096` and `flags: discards` (and the work-queue flags if you added them); `POOL_DEV` is
`/dev/mapper/luks-` followed by the UUID.

### Creating the ZFS pool

Why: the pool holds the whole system. Three steps: the host ID, for Option B the passphrase file, then the pool.

**All options: the host ID.** ZFS records in the pool the host ID of the system that imported it, and refuses a
normal import by another host while the pool was not exported. ZFSBootMenu, Gentoo's initramfs and the running
Gentoo all import this pool, so they must agree. Write a fixed host ID now; it is copied into Gentoo later, and from
there into both initramfs images:

```
root # zgenhostid -f 0x00bab10c
```

**Option B: the passphrase file.** `zpool create` reads the passphrase from the file named by `keylocation`, and the
same file later lets Gentoo's initramfs load the key without asking. Type the passphrase twice; it is not shown:

```
root # mkdir -p /etc/zfs
root # bash -c 'umask 077; IFS= read -r -s -p "ZFS passphrase: " p; echo; IFS= read -r -s -p "Again: " q; echo
  [ "$p" = "$q" ] && [ ${#p} -ge 8 ] && [ ${#p} -le 512 ] && printf "%s\n" "$p" > /etc/zfs/zroot.key ||
  { echo "The passphrases differ, or are not 8 to 512 characters long; nothing written." >&2; exit 1; }'
root # chmod 000 /etc/zfs/zroot.key
```

The `bash -c` command reads the passphrase twice without showing it, and writes the file only if both are the same
and 8 to 512 characters long (OpenZFS's limits); if it prints the error, run it again. The three lines are one
command; copy them together. `IFS=` keeps spaces at the start and end of the passphrase exactly as typed.

> **Important**
> Choose a strong passphrase of 8 to 512 characters that uses only characters found on a US keyboard layout:
> ZFSBootMenu asks for it before any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

The file must stay on the live system until the installation is finished; a copy goes into Gentoo. You type the
passphrase again in [Mounting the pool below /mnt](#mounting-the-pool-below-mnt), which catches a typing mistake
before anything is installed.

**Check (Option B):**

```
root # ls -l /etc/zfs/zroot.key
root # wc -l < /etc/zfs/zroot.key
```

The file is `----------` and owned by root, and `wc` prints exactly `1`.

**Options A and B** set `POOL_DEV` (Option C did when it opened the container):

```
root # export POOL_DEV="${POOL_PART}"
```

**Options A and C** create the pool:

```
root # zpool create -f \
  -o ashift=12 \
  -o autotrim=on \
  -o compatibility=openzfs-2.2-linux \
  -O compression=lz4 \
  -O acltype=posixacl \
  -O xattr=sa \
  -O relatime=on \
  -m none \
  -R /mnt \
  "${POOL}" "${POOL_DEV}"
```

**Option B** creates the pool with three more `-O` lines:

```
root # zpool create -f \
  -o ashift=12 \
  -o autotrim=on \
  -o compatibility=openzfs-2.2-linux \
  -O compression=lz4 \
  -O acltype=posixacl \
  -O xattr=sa \
  -O relatime=on \
  -O encryption=aes-256-gcm \
  -O keyformat=passphrase \
  -O keylocation=file:///etc/zfs/zroot.key \
  -m none \
  -R /mnt \
  "${POOL}" "${POOL_DEV}"
```

| Option | Meaning |
|---|---|
| `ashift=12` | 4 KiB blocks, matching the SSD (and the LUKS sector size in Option C) |
| `autotrim=on` | sends TRIM for freed space continuously |
| `compatibility=openzfs-2.2-linux` | only the features OpenZFS 2.2 knows, so live systems with ZFS 2.2 can import it |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | everything the pool mounts during the installation appears below `/mnt` |
| `encryption=aes-256-gcm` (B) | likely the fastest cipher on modern x86_64 hardware |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key; required |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file just written; Gentoo's initramfs will carry a copy |

The compatibility can be relaxed later: [Relaxing the pool compatibility](#relaxing-the-pool-compatibility).

In Option B the encryption root is the pool's root dataset, so every dataset created below it inherits the
encryption, including the boot environment with its `/boot`.

**Check:**

```
root # hostid
root # zpool status "${POOL}"
root # zpool get ashift,autotrim,compatibility "${POOL}"
root # zfs get -o property,value encryption,keyformat,keylocation,keystatus "${POOL}"
```

- `hostid` prints `00bab10c`.
- The pool is `ONLINE` on `nvme0n1p2` (Options A and B) or on `luks-<UUID>` (Option C).
- `ashift` is `12`, `autotrim` is `on`, `compatibility` is `openzfs-2.2-linux`.
- Options A and C: `encryption` is `off`. Option B: `aes-256-gcm`, `passphrase`, `file:///etc/zfs/zroot.key` and
  `available`.

### Creating the datasets

Why: ZFSBootMenu treats every file system with `mountpoint=/` that has a kernel and an initramfs in its `/boot` as a
boot environment. User data lives outside it, so that a rollback of the system does not roll back `/home`.

```
root # zfs create -o mountpoint=none "${POOL}/ROOT"
root # zfs create -o mountpoint=/ -o canmount=noauto "${POOL}/ROOT/${BE}"
root # zfs create -o mountpoint=/home "${POOL}/home"
root # zpool set bootfs="${POOL}/ROOT/${BE}" "${POOL}"
```

| Dataset | Properties | Purpose |
|---|---|---|
| `zroot/ROOT` | `mountpoint=none` | container for boot environments; carries the properties they inherit |
| `zroot/ROOT/gentoo` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, including `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/gentoo` | the boot environment ZFSBootMenu boots after the countdown |

`canmount=noauto` is required on every file system with `mountpoint=/`, and it is not inherited: each boot
environment created later needs it set again. Keep `/usr`, `/etc` and `/var` in the boot environment; do not give
them datasets of their own, or a rollback would separate Portage's database from the files it describes.

> **Important**
> `/boot` is a directory of the root dataset `zroot/ROOT/gentoo`, not a file system of its own and not the ESP.
> ZFSBootMenu reads Gentoo's kernel and initramfs from there, and in Options B and C the initramfs with the key
> file is then inside the encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/gentoo`.

### Setting the ZFSBootMenu properties

Why: ZFSBootMenu passes the value of the ZFS property `org.zfsbootmenu:commandline` to Gentoo's kernel. It is set
on `zroot/ROOT`, so every future boot environment inherits it. ZFSBootMenu adds the root argument itself: for a boot
environment whose `os-release` says `ID=gentoo` it is `root=ZFS=zroot/ROOT/gentoo`, which dracut's ZFS module
accepts. Never put `root=`, `spl_hostid=` or `zbm.*` options into this property.

**Options A and B:**

```
root # zfs set org.zfsbootmenu:commandline="quiet" "${POOL}/ROOT"
```

**Option B** also names the boot environment as the key source:

```
root # zfs set org.zfsbootmenu:keysource="${POOL}/ROOT/${BE}" "${POOL}"
```

When you choose a boot environment **in the menu**, ZFSBootMenu then mounts `zroot/ROOT/gentoo` after the
passphrase was typed, finds `etc/zfs/zroot.key` in it, and keeps a copy in memory until it boots, so that menu
operations that export and re-import the pool (setting the default, cloning a snapshot, switching to read/write) do
not ask again. It does not remove the first prompt, and a boot from the countdown does not use it. It is a
convenience, not a requirement.

**Option C, systemd.** Gentoo's initramfs runs systemd, whose `systemd-cryptsetup-generator` reads three `luks.*`
parameters that replace a crypttab:

```
root # KCL="quiet luks.crypttab=no luks.uuid=${LUKS_UUID}"
root # KCL="${KCL} luks.key=${LUKS_UUID}=/etc/cryptsetup-keys.d/zroot.key"
root # zfs set org.zfsbootmenu:commandline="${KCL}" "${POOL}/ROOT"
```

| Parameter | Effect |
|---|---|
| `luks.crypttab=no` | ignore `/etc/crypttab` completely |
| `luks.uuid=<UUID>` | unlock the container with this LUKS UUID, as `luks-<UUID>` |
| `luks.key=<UUID>=<path>` | use this key file for it; the file is created later, inside the chroot |

The running system reads the `luks.*` forms too. If you later add other LUKS volumes to `/etc/crypttab` (swap, a
data disk), switch to the forms only the initramfs reads: `rd.luks.crypttab=no`, `rd.luks.uuid=` and `rd.luks.key=`.

**Option C, OpenRC.** Without systemd in the initramfs, the `luks.*` parameters do nothing. dracut's `crypt` module
opens only the containers named by `rd.luks.uuid=`, and takes the key file from a line in `/etc/crypttab` inside the
initramfs (written in [Configuring the initramfs](#configuring-the-initramfs)):

```
root # zfs set org.zfsbootmenu:commandline="quiet rd.luks.uuid=${LUKS_UUID}" "${POOL}/ROOT"
```

**Check:**

```
root # zfs get -H -o value org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"
root # zfs get -H -o value org.zfsbootmenu:keysource "${POOL}"
```

- Options A and B: the command line is `quiet`. Option C with systemd: it contains your LUKS UUID twice. Option C
  with OpenRC: `quiet rd.luks.uuid=` and your LUKS UUID.
- The key source is `zroot/ROOT/gentoo` for Option B and `-` for Options A and C.

### Mounting the pool below /mnt

Why: an export and a fresh import prove that the pool can be opened again, and mount the datasets in the right
order: the root first.

```
root # zpool export "${POOL}"
root # zpool import -N -R /mnt "${POOL}"
```

**Option B** loads the key by typing the passphrase:

```
root # zfs load-key -L prompt "${POOL}"
```

If it refuses the passphrase, the file holds something else than you believe (`cat /etc/zfs/zroot.key` shows it).
Nothing is installed yet: run `zfs load-key "${POOL}"` (which reads the file), `zpool destroy "${POOL}"`, write the
file again and repeat from [Creating the ZFS pool](#creating-the-zfs-pool).

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

`-N` imports without mounting; the root dataset is mounted by hand because it is `canmount=noauto`. The cache file
is copied into Gentoo later; dracut's ZFS module puts it into the initramfs, and the `zfs-import-cache` service of
the systemd profile reads it.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot.
> The ZFS tools inside Gentoo may not match the ZFS kernel module that the live system has loaded.

**Check:**

```
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`/mnt` is `zroot/ROOT/gentoo` and `/mnt/home` is `zroot/home`. Option B: the key status is `available` (Options A
and C print `-`).

## Installing the base system

### Downloading and unpacking the stage file

Why: a stage3 archive is Gentoo's minimal base system; Portage builds the rest on top of it.

Download the latest stage3 of your profile and its signature into the new root dataset:

```
root # cd /mnt
root # BASE=https://distfiles.gentoo.org/releases/amd64/autobuilds
root # LATEST="$(wget -qO- "${BASE}/latest-stage3-amd64-${INIT}.txt" | awk '/^[0-9]+T[0-9]+Z\/stage3-/ { print $1 }')"
root # echo "${LATEST}"
root # wget "${BASE}/${LATEST}" "${BASE}/${LATEST}.asc"
```

`echo` prints a path such as `20261115T170104Z/stage3-amd64-systemd-20261115T170104Z.tar.xz` (`openrc` instead of
`systemd` for the OpenRC profile). If it prints nothing, download the newest `stage3-amd64-systemd-*.tar.xz` or
`stage3-amd64-openrc-*.tar.xz` and its `.asc` file by hand from
<https://distfiles.gentoo.org/releases/amd64/autobuilds/>, in the directory `current-stage3-amd64-systemd/` or
`current-stage3-amd64-openrc/`.

Verify the signature:

```
root # wget -O - https://qa-reports.gentoo.org/output/service-keys.gpg | gpg --import
root # gpg --verify stage3-amd64-*.tar.xz.asc
```

The output must include `Good signature from "Gentoo Linux Release Engineering (Automated Weekly Release Key)
<releng@gentoo.org>"`. A warning that the key is not certified with a trusted signature is normal.

Unpack it and remove the archive:

```
root # tar xpf stage3-amd64-*.tar.xz --xattrs-include='*.*' --numeric-owner -C /mnt
root # rm stage3-amd64-*.tar.xz*
root # cd /
```

**Check:**

```
root # ls /mnt
root # cat /mnt/etc/os-release
```

`/mnt` holds `bin`, `etc`, `usr`, `var` and the other directories of a root file system, and `os-release` contains
`ID=gentoo`.

### Copying the host ID and the key file

Why: Gentoo must use the same host ID as the live system, and (Option B) the same passphrase file; the ESP is
mounted where the kernel installer and `generate-zbm` expect it.

```
root # mkdir -p /mnt/boot/efi
root # mount "${ESP_PART}" /mnt/boot/efi
root # cp /etc/hostid /mnt/etc/hostid
root # mkdir -p /mnt/etc/zfs
root # cp /etc/zfs/zpool.cache /mnt/etc/zfs/zpool.cache
```

**Option B** copies the passphrase file, keeping its mode:

```
root # cp -p /etc/zfs/zroot.key /mnt/etc/zfs/zroot.key
```

**All options.** Save the variables for the chroot (for Options A and B the `LUKS_UUID` line is empty, which is
harmless):

```
root # cat > /mnt/root/install.env <<EOF
export DISK="${DISK}"
export ESP_PART="${ESP_PART}"
export POOL_PART="${POOL_PART}"
export POOL="${POOL}"
export BE="${BE}"
export INIT="${INIT}"
export POOL_DEV="${POOL_DEV}"
export LUKS_UUID="${LUKS_UUID}"
EOF
```

**Check:**

```
root # findmnt /mnt/boot/efi
root # od -A n -t x4 /mnt/etc/hostid
root # ls -l /mnt/etc/zfs/
root # cat /mnt/root/install.env
```

The ESP is mounted, the host ID prints `00bab10c`, `/mnt/etc/zfs` holds `zpool.cache` and, for Option B,
`zroot.key` with mode `----------`, and every variable except `LUKS_UUID` (Options A and B) has a value.

### Chrooting

Why: the rest of the installation runs inside the new system, with the live system's kernel.

```
root # cp --dereference /etc/resolv.conf /mnt/etc/
root # mount --types proc /proc /mnt/proc
root # mount --rbind /sys /mnt/sys && mount --make-rslave /mnt/sys
root # mount --rbind /dev /mnt/dev && mount --make-rslave /mnt/dev
root # mount --bind /run /mnt/run && mount --make-slave /mnt/run
root # chroot /mnt /bin/bash
```

```
(chroot) root # source /etc/profile
(chroot) root # source /root/install.env
(chroot) root # export PS1="(chroot) ${PS1}"
```

**Check:**

```
(chroot) root # ping -c 2 gentoo.org
(chroot) root # echo "${INIT} ${BE} ${LUKS_UUID}"
(chroot) root # ls /sys/firmware/efi/efivars | head -n 3
```

`ping` gets answers, the variables have their values (no `LUKS_UUID` for Options A and B), and the EFI variables
are visible.

### Configuring Portage

Why: Portage needs the ebuild repository, the right profile, and the settings that make kernel and ZFS updates
rebuild each other.

```
(chroot) root # emerge-webrsync
(chroot) root # eselect profile show
```

**systemd.** The profile must be `default/linux/amd64/23.0/systemd` (or a `desktop/.../systemd` variant).

**OpenRC.** The profile must be `default/linux/amd64/23.0` (or a `desktop` variant without `systemd`).

**Both profiles.** If it is not, select one with `eselect profile list` and `eselect profile set <number>`.

Add these lines to make.conf, replacing `8` with the number of CPU threads (`nproc`). If make.conf already has a
`USE=` line, add `dist-kernel` to it instead of writing a second one:

**FILE** `/etc/portage/make.conf` — *additions*

```
MAKEOPTS="-j8 -l8"

# Distribution kernels; ZFS is rebuilt and the initramfs reinstalled for each new kernel
USE="dist-kernel"

ACCEPT_LICENSE="-* @FREE @BINARY-REDISTRIBUTABLE"
```

`@BINARY-REDISTRIBUTABLE` covers the licenses of `sys-kernel/linux-firmware` and `sys-firmware/intel-microcode`.

> **Tip**
> To save hours of compiling, use Gentoo's binary packages: add
> `FEATURES="${FEATURES} getbinpkg binpkg-request-signature"` to make.conf and run `getuto`.

Set the USE flags this setup needs. Write the lines of your profile and option:

**FILE** `/etc/portage/package.use/zfsroot`

```
# Build an initramfs with dracut for every installed kernel, in the "compat" layout (/boot);
# never a layout that puts kernels and initramfs images onto the ESP
sys-kernel/installkernel dracut -efistub -grub -refind -systemd-boot -ugrd -uki -ukify

# systemd profile, Options A and B: the EFI stub for the ZFSBootMenu image
#sys-apps/systemd boot
# systemd profile, Option C: also systemd-cryptsetup, which reads the luks.* kernel parameters
#sys-apps/systemd boot cryptsetup

# OpenRC profile, all options: the EFI stub (boot needs kernel-install on systemd-utils)
#sys-apps/systemd-utils boot kernel-install
```

Remove the `#` from exactly one of the three commented lines.

| Package | USE flags | Why |
|---|---|---|
| `sys-kernel/installkernel` | `dracut` | runs dracut when a kernel is installed |
| `sys-kernel/installkernel` | `-efistub`, `-grub`, ... | the kernels stay in `/boot` (see below) |
| `sys-apps/systemd` (systemd) | `boot` | the EFI stub that `generate-zbm` needs |
| `sys-apps/systemd` (systemd, C) | `cryptsetup` | systemd-cryptsetup and its generator, for the second unlock |
| `sys-apps/systemd-utils` (OpenRC) | `boot`, `kernel-install` | the same EFI stub |

The EFI stub is `/usr/lib/systemd/boot/efi/linuxx64.efi.stub`. Both ebuilds require `kernel-install` together with
`boot` (`REQUIRED_USE="boot? ( kernel-install )"`); `sys-apps/systemd` enables it by default, `sys-apps/systemd-utils`
does not, hence the second flag on OpenRC.

`sys-fs/zfs` needs no entry: its defaults `rootfs`, `initramfs`, `modules` and `dist-kernel-cap` are what this guide
needs. Do not disable `initramfs`: without it, the ZFS module is left out of every dracut initramfs.

The installkernel line disables every flag that installs kernels for a boot loader. Do not enable `grub`,
`systemd-boot`, `uki` or `efistub` on `sys-kernel/installkernel`, even when it later prints that the `systemd-boot`
USE flag *should* be enabled: those layouts would put kernels and initramfs images onto the unencrypted ESP.
`/etc/kernel/install.conf`, written in [Kernel and ZFS](#kernel-and-zfs), fixes the layout as well.

Set the time zone, the locale and the host name (replace the values with your own), then rebuild the system with
the new settings:

```
(chroot) root # ln -sf ../usr/share/zoneinfo/Europe/London /etc/localtime
(chroot) root # echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen
(chroot) root # locale-gen
(chroot) root # eselect locale set en_US.utf8
(chroot) root # echo "gentoo" > /etc/hostname
(chroot) root # env-update && source /etc/profile && export PS1="(chroot) ${PS1}"
(chroot) root # emerge --ask --verbose --update --deep --newuse @world
```

**systemd.** The keyboard layout of the console is set in `/etc/vconsole.conf`, for example `KEYMAP=us`.

**OpenRC.** It is set in `/etc/conf.d/keymaps`, `keymap="us"` by default. OpenRC's `hostname` service reads
`/etc/hostname` as well.

**Check:**

```
(chroot) root # emerge --info | grep -E '^USE='
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
```

`USE` includes `dist-kernel`, and the EFI stub exists. If the stub is missing, the `sys-apps/systemd` or
`sys-apps/systemd-utils` line in `/etc/portage/package.use/zfsroot` is still commented out; fix it and run the
`emerge --update` command again.

## Kernel and ZFS

Why: Gentoo's distribution kernel comes prebuilt; ZFS is built for it, and its installer puts kernel and initramfs
into `/boot` of the root dataset, where ZFSBootMenu looks for them.

**All options.** Tell the kernel installer to keep kernels in `/boot`:

**FILE** `/etc/kernel/install.conf`

```
# Kernels and initramfs images go to /boot on ZFS, never onto the ESP: the initramfs may hold a key file
layout=compat
initrd_generator=dracut
uki_generator=none
BOOT_ROOT=/boot
```

This file replaces the package's `/usr/lib/kernel/install.conf` as a whole, for both installer backends, so it
repeats `initrd_generator` and `uki_generator`. With `layout=compat`, Gentoo's `90-compat.install` plugin installs
into `/boot` whatever directory systemd's `kernel-install` chooses, even the ESP. `BOOT_ROOT=/boot` is harmless:
`kernel-install` ignores it unless it is the root of a FAT file system, and the plugin does not read it.

Write the base configuration of Gentoo's initramfs **before** the kernel is installed:

```
(chroot) root # mkdir -p /etc/dracut.conf.d
```

**FILE** `/etc/dracut.conf.d/10-zfs.conf`

```
# A generic initramfs: built in a chroot, host-only detection would describe the live system
hostonly="no"
# Do not store detected kernel parameters; ZFSBootMenu passes the command line
hostonly_cmdline="no"
nofsck="yes"
# The ZFS root
add_dracutmodules+=" zfs "
# The NVMe driver, if the kernel builds it as a module
add_drivers+=" nvme "
```

| Setting | Why |
|---|---|
| `hostonly="no"` | a generic initramfs, which works on this computer from the first boot (see below) |
| `hostonly_cmdline="no"` | no kernel parameters are stored in the initramfs (see below) |
| `add_dracutmodules+=" zfs "` | OpenZFS's dracut module: imports the pool, loads keys, mounts `root=ZFS=...` |
| `add_drivers+=" nvme "` | harmless where the kernel has the driver built in |

dracut's host-only mode picks drivers by looking at the running system, which in the chroot is the live system.
`installkernel` also refuses to run dracut inside a chroot unless a file in `/etc/dracut.conf.d/` sets
`hostonly_cmdline="no"`, `hostonly="no"` or `kernel_cmdline=`; this file sets the first two.

> **Warning**
> Put dracut settings for Gentoo's initramfs only into `/etc/dracut.conf.d/`, never into `/etc/dracut.conf`.
> `generate-zbm` builds the ZFSBootMenu image with its own configuration directory,
> `/etc/zfsbootmenu/dracut.conf.d`, instead of `/etc/dracut.conf.d`, but dracut still reads the main file
> `/etc/dracut.conf` and the packages' directory `/usr/lib/dracut/dracut.conf.d/`. A key file named in either would
> end up in the ZFSBootMenu image on the unencrypted ESP.

Install the kernel, its firmware and ZFS. **Option C** adds `sys-fs/cryptsetup`; on an **Intel** CPU add
`sys-firmware/intel-microcode` (AMD microcode comes with `sys-kernel/linux-firmware`):

```
(chroot) root # emerge --ask sys-kernel/linux-firmware sys-kernel/gentoo-kernel-bin sys-fs/zfs
```

For example, Option C on an Intel computer:

```
(chroot) root # emerge --ask sys-kernel/linux-firmware sys-kernel/gentoo-kernel-bin sys-fs/zfs \
  sys-fs/cryptsetup sys-firmware/intel-microcode
```

| Package | Purpose |
|---|---|
| `sys-kernel/gentoo-kernel-bin` | the prebuilt distribution kernel (`sys-kernel/gentoo-kernel` compiles it) |
| `sys-fs/zfs` | ZFS tools and, from ZFS 2.4 on, the kernel modules; rebuilt for every new kernel |
| `sys-kernel/linux-firmware` | firmware, and AMD CPU microcode |
| `sys-firmware/intel-microcode` | Intel CPU microcode |
| `sys-fs/cryptsetup` (C) | LUKS tools, for Gentoo and for the ZFSBootMenu image |

What happens, so that the messages make sense: Portage installs the kernel first. Its installer sees that the root
is ZFS and that no ZFS module exists yet for the new kernel, and skips the installation into `/boot` for the moment.
When `sys-fs/zfs` has been built, it installs the kernel again, now with an initramfs that contains the ZFS module.
`USE=dist-kernel` and `dist-kernel-cap` keep the kernel within the range the ZFS version supports. The microcode
packages, with their default USE flag `initramfs`, tell dracut to put the microcode into the initramfs, and with
`USE=dist-kernel` they write no separate microcode file into `/boot`: ZFSBootMenu loads only one initramfs file per
kernel.

**Check:**

```
(chroot) root # ls /lib/modules/
(chroot) root # find /lib/modules -name 'zfs.ko*'
(chroot) root # ls -l /boot
(chroot) root # ls /boot/efi
(chroot) root # tail -n 3 /var/log/installkernel.log
```

- One kernel version is listed, and `zfs.ko` exists for it.
- **systemd.** `/boot` holds `kernel-<version>` and `initramfs-<version>.img`.
- **OpenRC.** `/boot` holds `vmlinuz-<version>`, `initramfs-<version>.img`, `System.map-<version>` and
  `config-<version>`. It may also hold the same names with `.old` appended: Gentoo's `installkernel` renames the
  files of a version to `….old` whenever it installs that version again, for example in the `emerge --config` loop
  below.
- `/boot/efi` is empty: no kernel and no initramfs went onto the ESP.
- The log's lines name the layout `compat` and `/boot`.

If `/boot` holds no `initramfs-<version>.img`, see
[installkernel stops inside the chroot](#installkernel-stops-inside-the-chroot) in Troubleshooting. This first
initramfs cannot use a key file yet; that is done in [Configuring the initramfs](#configuring-the-initramfs).

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Why: Gentoo's initramfs must unlock the container again after `kexec`; a random key in a second key slot lets it do
so without asking.

Create a random 64-byte key file and add it to the container. `cryptsetup` asks for the passphrase:

```
(chroot) root # install -d -m 0700 /etc/cryptsetup-keys.d
(chroot) root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot.key
(chroot) root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  "${POOL_PART}" /etc/cryptsetup-keys.d/zroot.key
```

A random key needs no slow key derivation, so `--pbkdf pbkdf2 --pbkdf-force-iterations 1000` (the smallest count
cryptsetup accepts) makes the second unlock almost instant. The passphrase slot keeps the slow, memory-hard default
(Argon2id).

**Check:**

```
(chroot) root # ls -l /etc/cryptsetup-keys.d/zroot.key
(chroot) root # cryptsetup open --test-passphrase --verbose --key-file /etc/cryptsetup-keys.d/zroot.key "${POOL_PART}"
(chroot) root # cryptsetup luksDump "${POOL_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, cryptsetup reports `Key slot 1 unlocked.`, and two key slots (`0` and
`1`) are listed.

## Configuring the initramfs

Why: Gentoo's initramfs must import the pool and, for Options B and C, unlock it with the key file it carries.

| | Option A | Option B | Option C |
|---|---|---|---|
| ZFS module and tools, `/etc/hostid`, `zpool.cache` | yes | yes | yes |
| `/etc/zfs/zroot.key` | | yes | |
| dracut's `crypt` module and `cryptsetup` | | | yes |
| `/etc/cryptsetup-keys.d/zroot.key` | | | yes |
| `/etc/crypttab` | | | OpenRC only |
| readable by root only | not needed | **required** | **required** |

**Option A** needs nothing more than `/etc/dracut.conf.d/10-zfs.conf` from [Kernel and ZFS](#kernel-and-zfs).

**Option B.** Put the passphrase file into the initramfs. OpenZFS's dracut module loads the key of an encryption
root whose `keylocation` is a file with `zfs load-key`, from the file inside the initramfs, without asking:

**FILE** `/etc/dracut.conf.d/20-zfs-key.conf`

```
# The ZFS passphrase file named by keylocation; the initramfs lives inside the encrypted pool
install_items+=" /etc/zfs/zroot.key "
```

**Option C, systemd.** Add the `crypt` module, which brings systemd-cryptsetup, and the key file named by
`luks.key=`:

**FILE** `/etc/dracut.conf.d/20-zfs-luks.conf`

```
# systemd-cryptsetup for the luks.* kernel parameters
add_dracutmodules+=" crypt "
# The key file named by luks.key=
install_items+=" /etc/cryptsetup-keys.d/zroot.key "
```

Make sure no crypttab is in the way:

```
(chroot) root # [ -e /etc/crypttab ] && mv /etc/crypttab /etc/crypttab.unused
```

**Option C, OpenRC.** dracut's unlock script looks up the container's line in `/etc/crypttab` for the mapping name
and the key file. Write the line with the device as `UUID=` and the LUKS UUID, the form the script matches:

```
(chroot) root # printf '%s UUID=%s %s luks\n' "luks-${LUKS_UUID}" "${LUKS_UUID}" \
  /etc/cryptsetup-keys.d/zroot.key >> /etc/crypttab
```

**FILE** `/etc/dracut.conf.d/20-zfs-luks.conf`

```
# The crypt module unlocks the containers named by rd.luks.uuid=
add_dracutmodules+=" crypt "
# The crypttab and the key file it names; a generic initramfs does not copy the crypttab by itself
install_items+=" /etc/cryptsetup-keys.d/zroot.key /etc/crypttab "
```

OpenRC does not read `/etc/crypttab` on the running system (it uses `/etc/conf.d/dmcrypt`), so the line only affects
the initramfs.

**Options B and C.** Gentoo's kernel installers copy the initramfs into `/boot` with mode `0644`, readable by every
user, and it now contains the key. A plugin makes every initramfs in `/boot` readable by root only, after every
kernel installation, including the reinstallations Portage triggers after ZFS updates.

The plugin's file name depends on the profile:

- **systemd.** **FILE** `/etc/kernel/install.d/95-initramfs-mode.install` (systemd's `kernel-install` runs only
  files that end in `.install`)
- **OpenRC.** **FILE** `/etc/kernel/postinst.d/95-initramfs-mode.install` (Gentoo's `installkernel` runs every
  executable file there)

Both get the same content:

```
#!/bin/sh
# The initramfs images in /boot hold a key: make them readable by root only.
# kernel-install passes "add" or "remove" first; the traditional installkernel passes the version.
[ "${1}" = "remove" ] && exit 0
for f in /boot/initramfs*; do
	[ -f "${f}" ] && [ ! -L "${f}" ] && chmod 0600 "${f}"
done
exit 0
```

**systemd.**

```
(chroot) root # mkdir -p /etc/kernel/install.d
(chroot) root # nano -w /etc/kernel/install.d/95-initramfs-mode.install
(chroot) root # chmod 0755 /etc/kernel/install.d/95-initramfs-mode.install
```

**OpenRC.**

```
(chroot) root # mkdir -p /etc/kernel/postinst.d
(chroot) root # nano -w /etc/kernel/postinst.d/95-initramfs-mode.install
(chroot) root # chmod 0755 /etc/kernel/postinst.d/95-initramfs-mode.install
```

Plugins run in the order of their names; `95` runs after the plugins that copy kernel and initramfs to `/boot`. The
plugin changes every initramfs file in `/boot`, also the `initramfs-<version>.img.old` copies that the OpenRC
installer keeps.

**All options.** Rebuild the initramfs of the installed kernel:

```
(chroot) root # for p in $(portageq match / sys-kernel/gentoo-kernel-bin); do emerge --config "=${p}"; done
```

Use `sys-kernel/gentoo-kernel` in that line if you installed that kernel instead.

**Check:**

```
(chroot) root # INITRD="$(ls -t /boot/initramfs-*.img | head -n 1)"
(chroot) root # ls -l "${INITRD}"
(chroot) root # lsinitrd "${INITRD}" | grep -E 'zfs\.ko|etc/hostid|zpool\.cache|zroot\.key|cryptsetup$'
(chroot) root # lsinitrd -f etc/crypttab "${INITRD}"
(chroot) root # lsinitrd "${INITRD}" | grep -i -E 'microcode|nvme'
(chroot) root # find /boot -maxdepth 1 -name 'initramfs*' -type f -perm /077
(chroot) root # ls /boot/efi
```

| | Option A | Option B | Option C, systemd | Option C, OpenRC |
|---|---|---|---|---|
| `ls -l` mode | any | `-rw-------` | `-rw-------` | `-rw-------` |
| first `grep` | `zfs.ko`, `etc/hostid`, `zpool.cache` | A's and `etc/zfs/zroot.key` | A's, see below | A's, see below |
| `lsinitrd -f etc/crypttab` | nothing | nothing | nothing | your line with the key file |
| `find` | (anything) | nothing | nothing | nothing |

Option C's first `grep` also lists `etc/cryptsetup-keys.d/zroot.key`, and `systemd-cryptsetup` (systemd) or
`bin/cryptsetup` (OpenRC).

- The microcode line lists `kernel/x86/microcode/GenuineIntel.bin` or `AuthenticAMD.bin` (with a generic
  initramfs, both, if both packages are installed). This guide did not confirm how `lsinitrd` shows the early part
  of the image; if the line is missing, `lsinitrd --unpackearly` is not needed to boot, and the microcode is
  optional. `nvme.ko` is listed only if the kernel builds the driver as a module; whether
  `sys-kernel/gentoo-kernel-bin` builds it in was not checked for this guide, and either way works. `modinfo -k
  "$(ls /lib/modules)" nvme | grep filename` shows `(builtin)` or a module path.
- `ls /boot/efi` prints nothing: no kernel and no initramfs on the ESP.
- If `find` prints a file (Options B and C), the plugin did not run: check its directory and mode.

## Configuring the system

Why: services, network, file systems and users of the installed system.

**systemd.** Enable systemd's presets and the ZFS services, and schedule a monthly scrub (it checks every block) and
a monthly full TRIM, which complements `autotrim`:

```
(chroot) root # systemd-machine-id-setup
(chroot) root # systemctl preset-all --preset-mode=enable-only
(chroot) root # systemctl enable zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target zfs-zed.service
(chroot) root # systemctl enable zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

Configure wired networking with systemd-networkd (for wireless, see the Gentoo wiki pages on iwd or
NetworkManager):

**FILE** `/etc/systemd/network/20-wired.network`

```
[Match]
Name=en* eth*

[Network]
DHCP=yes
```

```
(chroot) root # systemctl enable systemd-networkd.service systemd-resolved.service
```

**OpenRC.** Add the ZFS services to the runlevels that OpenZFS's `etc/init.d/README.md` gives for Gentoo (this
guide leaves out `zfs-share`, which only NFS and SMB shares need), and install a DHCP client for wired networking.
The Gentoo wiki's ZFS page, to which OpenZFS's documentation points for Gentoo, was not reachable for this guide.
`ls /etc/init.d/zfs-*` lists the services the installed `sys-fs/zfs` has:

```
(chroot) root # ls /etc/init.d/zfs-*
(chroot) root # rc-update add zfs-import boot
(chroot) root # rc-update add zfs-load-key boot
(chroot) root # rc-update add zfs-mount boot
(chroot) root # rc-update add zfs-zed default
(chroot) root # emerge --ask net-misc/dhcpcd
(chroot) root # rc-update add dhcpcd default
```

OpenRC has no timers; run the scrub and TRIM of [Scrubbing and trimming](#scrubbing-and-trimming) by hand, or from a
cron daemon of your choice.

**Both profiles.** Only the ESP goes into fstab; ZFS mounts its datasets itself. `generate-zbm` mounts the ESP from
this line when it is not mounted:

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

Set the root password and create a user (replace `USERNAME`):

```
(chroot) root # passwd
(chroot) root # useradd -m -G wheel,users -s /bin/bash USERNAME
(chroot) root # passwd USERNAME
```

**Check:**

**systemd.**

```
(chroot) root # systemctl is-enabled zfs-import-cache.service zfs-mount.service zfs.target systemd-networkd.service
(chroot) root # systemctl is-enabled zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

**OpenRC.**

```
(chroot) root # rc-update show | grep -E 'zfs|dhcpcd'
```

**Both profiles.**

```
(chroot) root # cat /etc/fstab
```

The services (and, on systemd, the timers) are `enabled` or listed in their runlevels; on OpenRC, `ls` listed
`zfs-import`, `zfs-load-key`, `zfs-mount` and `zfs-zed` among the services. fstab contains one `vfat`
line for `/boot/efi`.

## Installing ZFSBootMenu

This repository's build is used for all three options: upstream ZFSBootMenu cannot unlock LUKS volumes, and using
the same build everywhere keeps the steps the same.

### Dependencies

Why: the Rust programs are built here, and `generate-zbm` builds the image from this system's kernel, ZFS and tools.

| Package | Why |
|---|---|
| `dev-vcs/git` | to clone this repository |
| `dev-lang/rust-bin` (or `dev-lang/rust`) | `cargo` and `rustc` 1.88 or later |
| `sys-devel/binutils` | `strip`, `strings` and `objcopy`, used below; in every stage3 |
| `sys-kernel/dracut` | builds the image; already installed through `sys-kernel/installkernel` |
| `sys-apps/kexec-tools` | `kexec`, required in every image |
| `app-shells/fzf` | the menu of the default (shell) runtime |
| `sys-boot/efibootmgr` | the firmware's boot entries |
| `sys-block/mbuffer` | optional; the image is built without it, with a warning |
| `sys-fs/dosfstools` | `fsck.vfat` and `mkfs.vfat` for the ESP on the installed system |
| `sys-fs/cryptsetup` (C) | LUKS support in the image (see below) |
| the EFI stub | installed in [Configuring Portage](#configuring-portage) |

`binutils` provides `strip` for `make install`, `strings` for `generate-zbm` and `objcopy` for the Check of the
image. `sys-fs/cryptsetup` was installed in [Kernel and ZFS](#kernel-and-zfs) for Option C: without it on this
system, the image silently has no LUKS support. ZFS, util-linux, kmod, procps, ncurses and bash are already
installed. The Perl modules that older guides list are not
needed: `generate-zbm` is a Rust program on this branch.

```
(chroot) root # emerge --ask --noreplace dev-vcs/git dev-lang/rust-bin sys-apps/kexec-tools app-shells/fzf \
  sys-boot/efibootmgr sys-block/mbuffer sys-fs/dosfstools
```

Optionally add `sys-apps/nvme-cli` for the SSD's health report in [Usage](#scrubbing-and-trimming).

**Check:**

```
(chroot) root # cargo --version && rustc --version
(chroot) root # command -v git kexec fzf efibootmgr dracut lsinitrd strings strip objcopy fsck.vfat
(chroot) root # command -v cryptsetup
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
```

`rustc` is 1.88 or later (Gentoo's stable `rust-bin` is newer). If `cargo` is not found, `dev-lang/rust-bin` is
installed in a slot that is not selected: `eselect rust list` and `eselect rust set <number>`. Every program has a
path; `cryptsetup` only for Option C. The repository's `rust-toolchain.toml` names Rust 1.97.0, but only `rustup`
reads that file; Gentoo's `cargo` builds with its own version.

### Building and installing

Why: `make` builds `zbm`, `brush`, `generate-zbm` and `zbm-kcl`; `make install` puts them, the dracut modules and the
configuration where image builds look for them.

```
(chroot) root # git clone --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2.git /usr/src/zbm
(chroot) root # make -C /usr/src/zbm
(chroot) root # make -C /usr/src/zbm install
```

> **Warning**
> The repository's default branch, `master`, holds the upstream layout without the LUKS code. Always use
> `claude/zbm-refactor`. Do not run `make core dracut`, which older guides show: it installs neither `zbm` nor
> `generate-zbm`.

The Rust crates are vendored in the repository, so the build needs no network after the clone. `make install`
writes the version into the checkout and builds the programs again, as the user who runs it, so root needs `cargo`
on its `PATH`; on Gentoo `/usr/bin/cargo` is. `make install` puts:

- `generate-zbm` and `zbm-kcl` in `/usr/bin`;
- `zbm` and `brush` in `/usr/libexec/zfsbootmenu`, where every image build looks for `zbm`;
- the shell runtime in `/usr/share/zfsbootmenu`;
- the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in `/usr/lib/dracut/modules.d`, and the mkinitcpio
  hooks in `/usr/lib/initcpio`;
- `config.yaml`, `mkinitcpio.conf`, `dracut.conf.d/zfsbootmenu.conf` and `dracut.conf.d/omit-drivers.conf` in
  `/etc/zfsbootmenu`;
- the manual pages, and the `contrib/` hooks, `examples/` and `splash.bmp` in `/usr/share/examples/zfsbootmenu`.

`make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the two files it
installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files with other names in that directory are kept. That is why
`config.yaml` is written after `make install`, and this guide's own dracut settings for the image go into files of
their own.

The dracut module `zfsbootmenu-measured` is included in Gentoo's initramfs from its next rebuild as well. It only
copies the TPM event log that images with the Rust runtime pass on, and does nothing otherwise.

**Check:**

```
(chroot) root # git -C /usr/src/zbm branch --show-current
(chroot) root # generate-zbm --showver
(chroot) root # ls -l /usr/bin/generate-zbm /usr/bin/zbm-kcl /usr/libexec/zfsbootmenu/
(chroot) root # ls /usr/lib/dracut/modules.d/ | grep zfsbootmenu
```

The branch is `claude/zbm-refactor`, `generate-zbm` prints a version, `zbm` and `brush` are in
`/usr/libexec/zfsbootmenu`, and both dracut modules are listed.

Optional: Cargo's build directory is no longer needed and takes most of the space; delete it before the snapshot of
[Finalizing the installation](#finalizing-the-installation), so that the snapshot does not keep it. The next update
of ZFSBootMenu then builds from scratch.

```
(chroot) root # rm -rf /usr/src/zbm/target
```

### Configuring generate-zbm

Why: `generate-zbm` does nothing until it is configured, and the configuration decides where the image goes and
what ZFSBootMenu's own kernel command line is.

Set the LUKS option for ZFSBootMenu's command line. **Options A and B:**

```
(chroot) root # ZBM_LUKS=off
```

**Option C:**

```
(chroot) root # ZBM_LUKS="UUID=${LUKS_UUID}"
```

**All options.** Write the configuration (the shell fills in `${ZBM_LUKS}`, so `<<EOF` must not be quoted):

```
(chroot) root # cat > /etc/zfsbootmenu/config.yaml <<EOF
Global:
  ManageImages: true
  BootMountPoint: /boot/efi
  DracutConfDir: /etc/zfsbootmenu/dracut.conf.d
  PreHooksDir: /etc/zfsbootmenu/generate-zbm.pre.d
  PostHooksDir: /etc/zfsbootmenu/generate-zbm.post.d
  InitCPIOConfig: /etc/zfsbootmenu/mkinitcpio.conf
Components:
  Enabled: false
EFI:
  ImageDir: /boot/efi/EFI/zbm
  Versions: false
  Enabled: true
Kernel:
  Prefix: vmlinuz
  CommandLine: "quiet loglevel=0 zbm.luks=${ZBM_LUKS}"
EOF
```

| Key | Value | Meaning |
|---|---|---|
| `Global.ManageImages` | `true` | `generate-zbm` does nothing while this is `false`, the installed default |
| `Global.BootMountPoint` | `/boot/efi` | the ESP; mounted from fstab if needed, and unmounted again |
| `Global.DracutConfDir` | `/etc/zfsbootmenu/dracut.conf.d` | dracut settings of **ZFSBootMenu's** image (see below) |
| `Global.PreHooksDir`, `Global.PostHooksDir` | as shown | executables run before and after a build; none needed here |
| `Global.InitCPIOConfig` | as shown | used only with mkinitcpio; kept from the installed file |
| `Components.Enabled` | `false` | no separate kernel and initramfs; only the EFI bundle |
| `EFI.Enabled` | `true` | build the EFI bundle: kernel, initramfs and command line in one file |
| `EFI.ImageDir` | `/boot/efi/EFI/zbm` | where the bundle is written |
| `EFI.Versions` | `false` | one image and one `-backup` copy with fixed names, so the firmware entries never change |
| `Kernel.Prefix` | `vmlinuz` | fixes the image names to `vmlinuz.EFI` and `vmlinuz-backup.EFI` |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's** kernel, built into the bundle |

`Global.DracutConfDir` must not be `/etc/dracut.conf.d`, which holds the settings of Gentoo's own initramfs and,
in Options B and C, names its key file.

**`Kernel.Prefix`.** Without it, `generate-zbm` names the image after the kernel file with its version removed. On
the systemd profile Gentoo installs kernels as `kernel-<version>`, which would give `kernel.EFI` instead of the
`vmlinuz.EFI` the boot entries point to. On OpenRC the kernels are `vmlinuz-<version>` already; the setting keeps the
name fixed either way.

**Which kernel the image uses.** `generate-zbm` takes the highest version among `/boot/vmlinuz*`, and only if there
is none, among `/boot/vmlinux*`, `/boot/linux*` and then `/boot/kernel*`. The ZFS modules for that kernel must be
installed, which `USE=dist-kernel` ensures.

**`Kernel.CommandLine`.**

| Option | Value | Why |
|---|---|---|
| A, B | `quiet loglevel=0 zbm.luks=off` | ZFSBootMenu unlocks no LUKS volumes |
| C | `quiet loglevel=0 zbm.luks=UUID=<LUKS UUID>` | unlock only this container, as `luks-<UUID>` |

Without `zbm.luks=off`, ZFSBootMenu would offer every LUKS volume it finds, for example on a USB disk plugged in at
boot. With `zbm.luks=UUID=` it asks for the passphrase of this one container. The UUID is that of the **LUKS
container** (`cryptsetup luksUUID`), not the pool's GUID and not a file system UUID.

No ZFSBootMenu crypttab (`/etc/zfsbootmenu/crypttab`) is used: `zbm.luks=` takes precedence over one. Options you may
add later: `zbm.timeout=<seconds>` changes the countdown (default 10; `zbm.show` always shows the menu, `zbm.skip`
boots at once), and `loglevel=7` shows more messages for troubleshooting.

**ZFSBootMenu's dracut settings.** Build an image that does not depend on the machine it was built on: the first
image is built inside a chroot on the live system, whose kernel and loaded drivers are not those of the installed
system.

**FILE** `/etc/zfsbootmenu/dracut.conf.d/generic.conf`

```
# Build an image that does not depend on the machine it was built on: the first image is built
# inside a chroot on a live system, whose kernel and loaded drivers are not those of the installed system.
hostonly="no"
```

The project's build container builds its images the same way. The image gets larger; the ESP has room. The file's
name is not one that `make install` overwrites.

**Check:**

```
(chroot) root # cat /etc/zfsbootmenu/config.yaml
(chroot) root # ls /etc/zfsbootmenu/dracut.conf.d/
(chroot) root # cat /etc/zfsbootmenu/dracut.conf.d/generic.conf
```

`config.yaml` shows `ManageImages: true` and, for Option C, a real UUID after `zbm.luks=UUID=` (not an empty value).
`ls` lists `generic.conf omit-drivers.conf zfsbootmenu.conf`, and `generic.conf` contains `hostonly="no"`.

### Choosing the runtime

Why: the image runs one of two runtimes, and the choice is made at build time.

The **shell runtime** (Bash with the `fzf` menu) is the default and is enough for all three options. The **Rust
runtime** (the program `zbm` with a new menu) is opt-in and will replace the shell runtime after a beta. Both unlock
LUKS volumes with passphrases. Only the Rust runtime has the locked menu, measured boot, keyboard layouts other than
US at the passphrase prompt, and keeps decrypted keys only in process memory. The project's VM suite runs with the
Rust runtime by default.

Optional, to choose the Rust runtime:

```
(chroot) root # echo 'zfsbootmenu_runtime=rust' > /etc/zfsbootmenu/dracut.conf.d/runtime.conf
```

and add `init_on_free=1` to the end of `CommandLine` in `/etc/zfsbootmenu/config.yaml`, so that the kernel zeroes
the memory it frees. This command appends it to the line already there, for every option:

```
(chroot) root # sed -i 's/^\(  CommandLine: ".*\)"$/\1 init_on_free=1"/' /etc/zfsbootmenu/config.yaml
(chroot) root # grep CommandLine /etc/zfsbootmenu/config.yaml
```

The line ends in `init_on_free=1"`. Options A and B keep `zbm.luks=off`, and Option C keeps its
`zbm.luks=UUID=...`; never replace Option C's with `zbm.luks=off`, which switches LUKS unlocking off.
`zbm.menu=classic` on the same line keeps the Rust runtime's classic menu for this release.

Keyboard: with the shell runtime the passphrase prompt uses the US keyboard layout; choose characters that are in the
same place on your layout, or use the Rust runtime. With the Rust runtime, either add `zbm.console=drm` to
`Kernel.CommandLine` (the console on the display then types with the image's first layout), or press **Ctrl-K** at
the prompt to choose a layout. `zbm.console=drm` needs the kernel's `evdev` module and a DRM driver in the image
(`simpledrm` on the firmware's framebuffer is enough), for example with `add_drivers+=" evdev "` in a file of its own
in `/etc/zfsbootmenu/dracut.conf.d/`. The layouts in the image are those of `zfsbootmenu_keyboard_layouts`; unset, the
layout of `XKBLAYOUT` in `/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`. They are compiled from
`x11-misc/xkeyboard-config` and the compose rules of `x11-libs/libX11`; without them the build only warns.

**Check:**

```
(chroot) root # cat /etc/zfsbootmenu/dracut.conf.d/*.conf
```

The output shows `hostonly="no"`, the two installed files' settings, and `zfsbootmenu_runtime=rust` only if you
chose it.

### Building and checking the image

Why: the image is ZFSBootMenu itself; the second build keeps the first as a backup, so that the backup entry has
something to boot from the start.

```
(chroot) root # generate-zbm --dry-run --debug
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

`--dry-run` prints one `Would ...` line for each change and makes none, and names the kernel it would use; `--debug`
prints every command. The first real run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`; the second renames it to
`vmlinuz-backup.EFI` and writes a new one.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | `make install` was not run |
| `failed to install essential executable '<name>'` | a program is missing: often `kexec`, or `fzf` |
| `Unable to find UEFI stub loader at default locations` | the EFI stub is missing (USE `boot`) |
| `ManageImages not enabled, no action taken` | `make install` overwrote `config.yaml`; write it again |
| `Unable to locate libgcc_s.so` | the GCC runtime library was not found (see below) |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (see below) |
| `WARNING: ignoring inconsistent versions in kernel /boot/vmlinuz-<version>.old` | OpenRC: harmless (see below) |

- `fzf` is needed by the shell runtime only. The EFI stub comes with USE `boot` on `sys-apps/systemd` or
  `sys-apps/systemd-utils`; see [Configuring Portage](#configuring-portage).
- `libgcc_s`: the image build looks for `libgcc_s.so*` first in `/usr/lib/gcc/<target>/<version>/` of the GCC that
  `gcc-config -c` names, then in the system's library directories, then in every `/usr/lib/gcc/*/*/`. The message
  means none was found: `gcc-config -l` must list an installed GCC (`sys-devel/gcc`, in every stage3), and
  `gcc-config <number>` selects one. Set `zfsbootmenu_skip_gcc_s=yes` in a file in `/etc/zfsbootmenu/dracut.conf.d/`
  only if `zpool` does not need the library (see zfsbootmenu(7)).
- Host IDs: the SPL module's `spl_hostid` parameter differs from `/etc/hostid` (the check is skipped while the parameter
  is 0). The image takes `/etc/hostid`, which must be `00bab10c` (`od -A n -t x4 /etc/hostid`).
- The `.old` warning: Gentoo's `installkernel` keeps the previous files of a reinstalled version as `….old`.
  `generate-zbm` skips `vmlinuz-<version>.old`, because the version in its name (`<version>.old`) is not the one
  inside the kernel, and uses `vmlinuz-<version>`. The warning can be ignored; `rm -f /boot/*.old` removes the copies.

**Check** the image, without a reboot:

```
(chroot) root # ls -l /boot/efi/EFI/zbm/
(chroot) root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
(chroot) root # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/cryptsetup$|etc/hostid|zfs\.ko|nvme'
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'cryptsetup-keys\.d|\.key$|etc/zfsbootmenu/crypttab'
(chroot) root # rm /run/zbm-initrd.img
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `ls` | `vmlinuz.EFI` and `vmlinuz-backup.EFI` | the same | the same |
| `zbm-kcl` prints | `quiet loglevel=0 zbm.luks=off` | the same | `quiet loglevel=0 zbm.luks=UUID=` and the LUKS UUID |
| first `grep` lists | `etc/hostid`, `zfs.ko`, and `nvme` if it is a module | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

The second `grep` must print nothing: no key file and no crypttab are in the image. If it prints a line, do not boot
this image; find out why first (usually a key named in `/etc/dracut.conf` or in `/usr/lib/dracut/dracut.conf.d/`).
If `cryptsetup` is listed for Options A or B, that is fine: it is in the image whenever the package is installed.

### Creating the firmware boot entries

Why: the firmware starts what its boot entries name; one entry for the image and one for its backup.

```
(chroot) root # efibootmgr -c -d "${DISK}" -p 1 -L "ZFSBootMenu (Backup)" -l '\EFI\ZBM\VMLINUZ-BACKUP.EFI'
(chroot) root # efibootmgr -c -d "${DISK}" -p 1 -L "ZFSBootMenu" -l '\EFI\ZBM\VMLINUZ.EFI'
```

- `-d` takes the **SSD** (`/dev/nvme0n1`), `-p` the **number** of the ESP's partition (`1`).
- `-c` puts the new entry first in the boot order. The backup is created first and the main entry second, so the
  main entry ends up first.
- The path is written from the root of the ESP with backslashes; FAT is not case-sensitive.
- If `efibootmgr` reports that EFI variables are not supported, run
  `mount -t efivarfs efivarfs /sys/firmware/efi/efivars` and try again.
- Entries of an earlier installation stay in the firmware. `efibootmgr` lists them; `efibootmgr -b XXXX -B` deletes
  entry `BootXXXX`.

Also copy the image to the path that firmware starts when it has no usable entry, and install a `generate-zbm`
post-hook that refreshes the copy after every build:

```
(chroot) root # mkdir -p /boot/efi/EFI/BOOT /etc/zfsbootmenu/generate-zbm.post.d
(chroot) root # cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI
```

**FILE** `/etc/zfsbootmenu/generate-zbm.post.d/50-bootx64.sh`

```
#!/bin/sh
# Keep the firmware's fallback copy the same as the image generate-zbm just built.
cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI
```

```
(chroot) root # chmod 0755 /etc/zfsbootmenu/generate-zbm.post.d/50-bootx64.sh
```

- Some firmware ignores boot entries, or loses them after a firmware update or a reset of its settings, and starts
  `\EFI\BOOT\BOOTX64.EFI` instead. A stale copy there could be too old for the pool (see
  [Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).
- `generate-zbm` runs every executable file in `Global.PostHooksDir` (set in `config.yaml` above) after it has
  written the images, in name order; a failed build stops before the hooks, so the copy is never replaced by a
  failed image. A file without the `x` bit is skipped, and the hook's exit code is ignored.

**Check:**

```
(chroot) root # efibootmgr -v | grep -i zfsbootmenu
(chroot) root # efibootmgr | grep BootOrder
(chroot) root # ls -l /boot/efi/EFI/BOOT/ /etc/zfsbootmenu/generate-zbm.post.d/
(chroot) root # cmp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI
```

Two entries point at `\EFI\ZBM\VMLINUZ.EFI` and `\EFI\ZBM\VMLINUZ-BACKUP.EFI`, `BootOrder` starts with the number of
the `ZFSBootMenu` entry, `BOOTX64.EFI` exists, `50-bootx64.sh` is `-rwxr-xr-x`, and `cmp` prints nothing.

## Finalizing the installation

Why: a snapshot of the fresh system is the first thing to roll back to, and the pool must be exported so that the
next import finds it in the state it expects.

**systemd.** Switch DNS to systemd-resolved (this breaks name resolution inside the chroot, so it comes last):

```
(chroot) root # ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
```

**All options, both profiles.** Delete the variables file (it holds no secret) and leave the chroot:

```
(chroot) root # rm /root/install.env
(chroot) root # exit
```

Snapshot, unmount and export:

```
root # zfs snapshot "${POOL}/ROOT/${BE}@install"
root # umount /mnt/boot/efi
root # umount -R -l /mnt/dev /mnt/sys /mnt/proc /mnt/run
root # zfs umount -a
root # zpool export "${POOL}"
```

**Option C** then closes the container:

```
root # cryptsetup close "luks-${LUKS_UUID}"
```

**Check:**

```
root # zpool list
root # ls /dev/mapper
```

`zpool list` reports *no pools available*. Option C: `/dev/mapper` contains only `control`. If the export fails
because the pool is busy, leave any shell that is still inside `/mnt` and repeat the unmount and export commands.

Reboot and remove the live medium:

```
root # reboot
```

## First boot

Why: this is the first time ZFSBootMenu and Gentoo's initramfs run on this computer.

With the default (shell) runtime you see:

| Step | Option A | Option B | Option C |
|---|---|---|---|
| 1 | | | `Unlock LUKS volume <device>`; type the LUKS passphrase |
| 2 | `Booting zroot/ROOT/gentoo in 10 seconds` | the same | the same |
| 3 | | `Enter passphrase for 'zroot':`; type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Gentoo's kernel | the same | the same |
| 5 | Gentoo starts | Gentoo starts **without** asking again | Gentoo starts **without** asking again |

In step 1, the prompt reads `Passphrase (leave empty to skip):`. Step 2 also shows `[RETURN] boot now` and
`[ESCAPE] boot menu`. In step 3 the screen clears before the prompt.

- **Enter** boots at once; **Escape** during the countdown opens the menu, which lists `zroot/ROOT/gentoo`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: `zfs load-key` allows three attempts. After three wrong passphrases ZFSBootMenu opens its emergency shell
  with `unable to load key for zroot/ROOT/gentoo`. Type `exit` there; the menu then asks again.
- Option B, OpenRC: dracut's ZFS script may print `Encrypted ZFS password for zroot:  [1/5]:` and continue at
  once, because `zfs load-key` reads the key file without waiting for input. The prompt names the encryption root,
  `zroot`. This was read from OpenZFS 2.4.4's `contrib/dracut/90zfs/mount-zfs.sh.in`, not seen.
- With the **Rust runtime** the same questions appear as dialogs of the menu's start-up view, and the countdown reads
  `Booting <environment> in <N> s. Press any key to choose another.`

From now on, `root #` means root on the **installed** Gentoo. Log in as root and run:

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status zroot
root # hostid
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
```

**systemd:** `systemctl --failed`. **OpenRC:** `rc-status --crashed`.

**Option B:**

```
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/gentoo
```

Both datasets show `aes-256-gcm`, the encryption root `zroot` and the key status `available`. Only the encryption
root has a key location: `zroot` shows `file:///etc/zfs/zroot.key`, and `zroot/ROOT/gentoo` shows `none`.

**Option C:**

```
root # LUKS_UUID="$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)"
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

**Check:**

| | Option A | Option B | Option C |
|---|---|---|---|
| `/proc/cmdline` | `quiet`, `root=ZFS=zroot/ROOT/gentoo`, `spl.spl_hostid=0x00bab10c` | the same | the same, and more |
| `/` | `zroot/ROOT/gentoo` | the same | the same |
| `zpool status` | `ONLINE`, on `nvme0n1p2` | the same | `ONLINE`, on `luks-<UUID>` |
| `hostid` | `00bab10c` | the same | the same |
| encryption | | `aes-256-gcm`, the key file's URL, `available` | `LUKS2`, `4096`, `discards` (see below) |
| asked for the passphrase | never | once, by ZFSBootMenu | once, by ZFSBootMenu |

Option C's command line also has `luks.crypttab=no luks.uuid=... luks.key=...` (systemd) or `rd.luks.uuid=...`
(OpenRC). Option B's key location is `file:///etc/zfs/zroot.key`. Option C's `cryptsetup status` shows
`type: LUKS2`, `sector size: 4096` and `flags: discards`.

No units have failed (systemd) and no services crashed (OpenRC). `BootCurrent` names the entry the firmware used; if it
is not the `ZFSBootMenu` entry, the firmware started `\EFI\BOOT\BOOTX64.EFI`, which the post-hook of [Creating the
firmware boot entries](#creating-the-firmware-boot-entries) refreshes at every `generate-zbm`. If Gentoo asked for a
passphrase a second time, see
[Troubleshooting](#the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs).

**Option C:** make the first LUKS header backup now; see
[Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header).

## Usage

### Updating the system

```
root # zfs snapshot zroot/ROOT/gentoo@pre-update-$(date +%Y%m%d)
root # emerge --sync
root # emerge --ask --verbose --update --deep --newuse @world
root # emerge --ask --depclean
```

The snapshot is what [Boot environments, snapshots and rollback](#boot-environments-snapshots-and-rollback) goes back
to. A new distribution kernel is installed into `/boot` with a new initramfs; because `sys-fs/zfs` depends on the
kernel's version with `USE=dist-kernel`, Portage rebuilds ZFS for it, which installs the kernel and its initramfs
again, now with the ZFS module. Each initramfs is built from `/etc/dracut.conf.d/`, so it carries the key file
(Options B and C), and the plugin of [Configuring the initramfs](#configuring-the-initramfs) makes it readable by
root only. ZFSBootMenu lists every kernel in `/boot` that has an initramfs beside it (**MOD+K** in the menu); by
default it boots the newest.

**Check** after an update that brought a kernel:

```
root # ls -l /boot
root # find /boot -maxdepth 1 -name 'initramfs*' -type f -perm /077
root # lsinitrd /boot/initramfs-NEWVERSION.img | grep -E 'zfs\.ko|zroot\.key'
```

Every kernel has its `initramfs-<version>.img`, `find` prints nothing (Options B and C), and the new initramfs
holds `zfs.ko` and, for Options B and C, the key file.

### Regenerating the ZFSBootMenu image

The image contains the kernel, the ZFS module and tools, cryptsetup and the other unlock tools of **this system at
the time of the build**. Rebuild it after updates of `sys-fs/zfs`, of the kernel, of `sys-fs/cryptsetup`, and after
changing `/etc/zfsbootmenu/`:

```
root # generate-zbm
```

- Each run keeps the previous image as `vmlinuz-backup.EFI`, which the `ZFSBootMenu (Backup)` entry boots. The backup
  is replaced by the previous image at every build, so after two bad builds in a row it is bad too. A copy under
  another name with its own entry survives that:

  ```
  root # cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/zbm/known-good.EFI
  root # efibootmgr -c -d /dev/nvme0n1 -p 1 -L "ZFSBootMenu (known good)" -l '\EFI\ZBM\KNOWN-GOOD.EFI'
  ```

  `efibootmgr -c` puts that entry first; restore the order with `efibootmgr -o` (the numbers of `ZFSBootMenu`,
  `ZFSBootMenu (Backup)`, then the new one, separated by commas).
- `generate-zbm` uses the newest kernel in `/boot` and needs the ZFS module for it.
- Rebuild **before** `zpool upgrade` or raising `compatibility`
  ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).
- The post-hook `50-bootx64.sh` refreshes the firmware's fallback copy `\EFI\BOOT\BOOTX64.EFI` after every build.
  `cmp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI` prints nothing when the two are the same;
  `generate-zbm --debug` also prints `## Processing hook: ...50-bootx64.sh`. The `known-good.EFI` copy above is never
  refreshed; that is its purpose.

To rebuild the image automatically, a Portage phase hook can run `generate-zbm` after ZFS or cryptsetup is merged.
A new kernel makes Portage rebuild `sys-fs/zfs`, so the hook also runs after kernel updates, once the ZFS module for
the new kernel exists. Set it up once, after the first boot:

**FILE** `/etc/portage/bashrc` — *addition*

```
# Rebuild the ZFSBootMenu image after ZFS or cryptsetup updates
post_pkg_postinst() {
	case "${CATEGORY}/${PN}" in
	sys-fs/zfs|sys-fs/zfs-kmod|sys-fs/cryptsetup)
		[ "${ROOT:-/}" = "/" ] || return 0
		[ -x /usr/bin/generate-zbm ] || return 0
		einfo "Rebuilding the ZFSBootMenu image"
		/usr/bin/generate-zbm || ewarn "generate-zbm failed: run it by hand before rebooting"
		;;
	esac
}
```

If `/etc/portage/bashrc` already defines `post_pkg_postinst`, add the `case` block to it instead of defining it
twice. The hook has not been tested by this project. Each build replaces the backup image, so keep a known-good copy
as in the tip below.

**Check** after the next update of `sys-fs/zfs` or the kernel:

```
root # ls -l --time-style=long-iso /boot/efi/EFI/zbm/
root # zfs version
```

The images are newer than the update; after a reboot `zfs version` shows the same version for the tools and the
kernel module.

> **Tip**
> A ZFSBootMenu image holds no secrets, so copies are harmless. After a new image has booted well, keep a copy that
> `generate-zbm` never touches, with an entry of its own:
>
> ```
> root # install -D -m 0644 /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/zbm-good/vmlinuz.EFI
> root # efibootmgr -c -d /dev/nvme0n1 -p 1 -L "ZFSBootMenu (known good)" -l '\EFI\ZBM-GOOD\VMLINUZ.EFI'
> ```
>
> `efibootmgr -c` puts the new entry first; `efibootmgr -o` sets the order you want. An old image can import the
> pool only as long as the pool has no features newer than its ZFS.

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Snapshots
of it are taken on the running system (`zfs snapshot zroot/ROOT/gentoo@NAME`) or in ZFSBootMenu.

To get into the menu, press **Escape** during the countdown (shell runtime) or any key (Rust runtime). In the shell
runtime, `MOD` is **Ctrl** or **Alt**.

Main screen:

| Key | Action |
|---|---|
| **Enter** | boot the selected boot environment |
| **MOD+K** | list its kernels |
| **MOD+S** | list its snapshots |
| **MOD+D** | make it the default (`bootfs`) |
| **MOD+E** | edit the kernel command line for this boot only |
| **MOD+P** | pool status |
| **MOD+W** | import the pool read/write |
| **MOD+J** | chroot into the boot environment |
| **MOD+R** | recovery shell |
| **MOD+L** | logs |
| **MOD+X** | power menu: shut down, reboot, firmware setup |

Snapshot screen:

| Key | Action | What it does |
|---|---|---|
| **MOD+R** | roll back | rolls the boot environment back to the snapshot; destructive, and asks for confirmation |
| **MOD+C** | clone | a new boot environment that depends on the snapshot; for booting an earlier state for a while |
| **MOD+X** | clone and promote | a new boot environment that keeps the snapshots; the old one can be destroyed |
| **Enter** | duplicate | an independent copy (`zfs send` piped into `zfs recv`); uses more space |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

In the Rust runtime's menu, **2** or **s** shows the snapshots and **r** rolls back after a review page; **?** lists
the keys of every view.

A worked example:

1. Before an update: `zfs snapshot zroot/ROOT/gentoo@pre-update`.
2. After a bad update: reboot, press **Escape** in the countdown, select `zroot/ROOT/gentoo`, **MOD+S**, select
   `@pre-update`, **MOD+R**, confirm, and boot.
3. Or, to try the old state without losing the new one: **MOD+C** on the snapshot, and boot the clone.

- Rollback and the other changes need the pool imported read/write; ZFSBootMenu imports pools read-only and
  re-imports when needed. In Option B that re-import needs the key again; the key source set in
  [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties) saves the second prompt.
- A rollback of the boot environment does not touch `zroot/home`.
- A boot environment made by hand needs `mountpoint=/` and `canmount=noauto`; it inherits
  `org.zfsbootmenu:commandline` from `zroot/ROOT`.
- Snapshots and clones contain `/boot`, so each boots with the kernel and initramfs (and, in Options B and C, the key
  file) it had when the snapshot was taken. After the passphrase (B) or the key file (C) was replaced, an older
  snapshot's initramfs holds the old one, and its system asks for the passphrase or stops in its initramfs.
- `zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the `org.zfsbootmenu:commandline` property
  that every boot environment inherits; see `man zbm-kcl`. Without a dataset name `zbm-kcl` acts on the root file
  system, `zroot/ROOT/gentoo`, and a change there sets a property of its own on that boot environment only.

### Changing the passphrase

**Option A**: nothing to change.

**Option B.** The key file **is** the passphrase, so both change together, and the system's initramfs, which holds
a copy of the file, must be rebuilt. The new passphrase goes into a new file first; the old file stays in place
until the pool has accepted the new key:

```
root # bash -c 'umask 077; IFS= read -r -s -p "New passphrase: " p; echo; IFS= read -r -s -p "Again: " q; echo
  [ "$p" = "$q" ] && [ ${#p} -ge 8 ] && [ ${#p} -le 512 ] && printf "%s\n" "$p" > /etc/zfs/zroot.key.new ||
  { echo "The passphrases differ, or are not 8 to 512 characters long; nothing written." >&2; exit 1; }'
root # zfs change-key -o keylocation=file:///etc/zfs/zroot.key.new zroot
root # zfs load-key -n -L prompt zroot
root # mv /etc/zfs/zroot.key.new /etc/zfs/zroot.key
root # chmod 000 /etc/zfs/zroot.key
root # zfs set keylocation=file:///etc/zfs/zroot.key zroot
root # zfs load-key -n zroot
```

- The first command reads the new passphrase twice, as in [Creating the ZFS pool](#creating-the-zfs-pool), and writes
  `zroot.key.new` only if both are the same; if it prints the error, run it again before going on.
- `zfs change-key` needs the current key loaded, which it is on the running system. It reads the **new** key from
  the file named by `-o keylocation=`. If it fails, stop: delete `/etc/zfs/zroot.key.new` and start again; the
  pool and the old file are unchanged.
- `zfs load-key -n -L prompt` only checks a key: type the new passphrase once more. `zfs load-key -n` without `-L`
  checks the file named by `keylocation`. Both must return without an error message; rebuild the initramfs only
  then. If one reports an incorrect key, do not reboot: the key is still loaded, so nothing is lost; run
  `zfs change-key` again with a file that holds the passphrase you want.
- This procedure follows OpenZFS's documentation of `zfs change-key` and `zfs load-key`; the project has not run
  it.

> **Warning**
> Rebuild every initramfs now. An initramfs with the old file cannot load the key at the next boot, and dracut's ZFS
> module does not fall back to a prompt for a `file://` location: the boot stops in dracut's emergency shell.

```
root # for p in $(portageq match / sys-kernel/gentoo-kernel-bin); do emerge --config "=${p}"; done
root # rm -f /boot/initramfs-*.img.old
root # find /boot -maxdepth 1 -name 'initramfs*' -type f -perm /077
root # lsinitrd -f etc/zfs/zroot.key "/boot/initramfs-$(uname -r).img" | sha256sum
root # sha256sum < /etc/zfs/zroot.key
```

The `rm` removes the initramfs copies that the OpenRC installer keeps as `.old`: they hold the old passphrase (on
the systemd profile there are none). `find` prints nothing, and the last two lines print the same checksum. No
`generate-zbm` is needed: ZFSBootMenu's image holds no key.

**Option C:**

```
root # cryptsetup luksChangeKey /dev/disk/by-partlabel/zroot-luks
```

It asks for the old passphrase, then the new one twice. The key file's slot is not affected, and no image needs
rebuilding. Make new header backups afterwards: the old ones still accept the old passphrase. A second passphrase as
a reserve: `cryptsetup luksAddKey /dev/disk/by-partlabel/zroot-luks`.

### Replacing the key file

**Option A**: there is none. **Option B**: the key file is the passphrase; see
[Changing the passphrase](#changing-the-passphrase).

**Option C.** Create a new key file, add it, test it, rebuild every initramfs with it, and only then remove the old
key. Until the last step both keys open the container, so a failed rebuild or a power cut leaves a system that
still boots without typing.

```
root # cd /etc/cryptsetup-keys.d
root # install -m 0400 /dev/null zroot.key.new
root # dd if=/dev/urandom of=zroot.key.new bs=64 count=1
root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  /dev/disk/by-partlabel/zroot-luks zroot.key.new
root # cryptsetup open --test-passphrase --key-file zroot.key.new /dev/disk/by-partlabel/zroot-luks
root # mv zroot.key zroot.key.old
root # mv zroot.key.new zroot.key
root # for p in $(portageq match / sys-kernel/gentoo-kernel-bin); do emerge --config "=${p}"; done
root # rm -f /boot/initramfs-*.img.old
root # lsinitrd -f etc/cryptsetup-keys.d/zroot.key "/boot/initramfs-$(uname -r).img" | sha256sum
root # sha256sum < zroot.key
```

`luksAddKey` asks for the passphrase. Rebuilding the initramfs is required: it contains a copy of the key file.
The `rm` removes the initramfs copies that the OpenRC installer keeps as `.old`, which hold the old key.
Continue only when the last two lines print the same checksum. Then remove the old key slot and the old file:

```
root # cryptsetup luksRemoveKey /dev/disk/by-partlabel/zroot-luks zroot.key.old
root # rm zroot.key.old
root # cd
root # cryptsetup luksDump /dev/disk/by-partlabel/zroot-luks | grep -E '^\s+[0-9]+: luks2'
```

`luksDump` lists two key slots: the passphrase's and the new key file's (one more for each reserve passphrase).
Make a new header backup afterwards.

### Option C only: backing up the LUKS header

Skip this section for Options A and B. (Option B has no header: the wrapped keys are part of the pool's metadata.)

A damaged LUKS header makes the data unrecoverable, even with the right passphrase. Store a header backup **away from
this SSD**, for example on an encrypted USB stick:

```
root # cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks \
  --header-backup-file /PATH/ON/USB/zroot-luks-header.img
```

- Restore with `cryptsetup luksHeaderRestore /dev/disk/by-partlabel/zroot-luks --header-backup-file FILE`.
- A header backup holds the key slots as they were when it was made: whoever has it and an old passphrase (or the
  old key file) can open the container. Keep it offline, and make a new one after every change of passphrase or key
  file.

**Check:**

```
root # cryptsetup luksDump /PATH/ON/USB/zroot-luks-header.img | grep -E '^(Version|UUID):'
root # cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks
```

The dump shows `Version: 2` and the same UUID that `luksUUID` prints.

### Scrubbing and trimming

A scrub reads and checks every block; a full TRIM complements `autotrim`. On the systemd profile the timers enabled
during the installation run both monthly. To run them by hand (on OpenRC, this is the way), and to see the result:

```
root # zpool scrub zroot
root # zpool trim zroot
root # zpool status -t zroot
```

Option C: TRIM reaches the SSD because the flag is stored in the LUKS header; `lsblk -D` shows non-zero discard sizes
for `luks-<UUID>`. With `sys-apps/nvme-cli` installed, `nvme smart-log /dev/nvme0` reports the SSD's health:
`critical_warning` should be `0`; `percentage_used` is the wear.

### Updating ZFSBootMenu

```
root # cp -a /etc/zfsbootmenu/config.yaml /root/zbm-config.yaml.keep
root # git -C /usr/src/zbm checkout -- crates/zbm-platform/src/version.rs legacy/zfsbootmenu/zbm-release
root # git -C /usr/src/zbm status --short
root # git -C /usr/src/zbm pull
root # make -C /usr/src/zbm
root # make -C /usr/src/zbm install
root # cp -a /root/zbm-config.yaml.keep /etc/zfsbootmenu/config.yaml
root # generate-zbm
```

- `make install` overwrites `config.yaml`, hence the copy and the restore. `generic.conf` and `runtime.conf` are kept.
- `make install` writes the version into two files of the checkout, and `git pull` refuses to run when the update
  changes either of them. The `git checkout --` line restores both; `git status --short` must then print nothing.
- `claude/zbm-refactor` is a development branch: `git pull` shows what changed; read `git log` before building.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm before relying on it; the backup
entry still boots the previous image.

### Relaxing the pool compatibility

Most readers can skip this. The pool was created with `compatibility=openzfs-2.2-linux`, so that live systems with
ZFS 2.2 can import it. Once you no longer need that, newer features can be enabled. Update every system that imports
the pool first, starting with ZFSBootMenu, and check that your rescue medium's ZFS is new enough:

```
root # generate-zbm
root # zpool set compatibility=openzfs-2.3-linux zroot
root # zpool upgrade zroot
```

Raise the set one step at a time; use the newest set that the installed ZFS **and** the rescue medium both know
(`ls /usr/share/zfs/compatibility.d/` lists them). `compatibility=off` enables everything the installed ZFS has. When
ZFSBootMenu's ZFS is older than the pool's features, it shows *Unsupported features detected* and refuses read/write
operations, or cannot import the pool at all.

- One `generate-zbm` run refreshes `vmlinuz.EFI` and, through the post-hook, `BOOTX64.EFI`; `vmlinuz-backup.EFI`
  is then the previous image. Before `zpool upgrade`, boot the new image once and run `generate-zbm` a second time,
  so that the backup holds the new ZFS too. A `known-good.EFI` copy, if you made one, is never updated: copy the new
  image over it, or delete it and its entry (`efibootmgr -b XXXX -B`).

## Troubleshooting

**Getting a shell in ZFSBootMenu.** Shell runtime: **MOD+R** in the menu opens the recovery shell; **Escape** at the
prompt `Unable to import pool, retrying in NN seconds` opens it too. Rust runtime: **!**. `exit` returns to
ZFSBootMenu.

**More messages.** Replace `loglevel=0` with `loglevel=7` on ZFSBootMenu's command line, and run `zlogtail` in the
recovery shell. `zbm-kcl` changes the command line of the image without a rebuild. The live medium has no `zbm-kcl`:
run it on the installed system, or inside the chroot of
[Repairing the system from the live medium](#repairing-the-system-from-the-live-medium):

```
root # zbm-kcl -r loglevel -a loglevel=7 /boot/efi/EFI/zbm/vmlinuz.EFI
```

`-r loglevel` removes `loglevel=0` first. Do the same on `/boot/efi/EFI/BOOT/BOOTX64.EFI` if the firmware starts that
copy. The change lasts until `generate-zbm` runs again, also from a hook, which writes `Kernel.CommandLine` of
`/etc/zfsbootmenu/config.yaml` into the image again; to keep `loglevel=7`, change it there.

### The firmware does not find ZFSBootMenu

- Check that Secure Boot is disabled (the `od` command in [Preparing the NVMe SSD](#preparing-the-nvme-ssd)).
- Choose *ZFSBootMenu*, or the NVMe SSD itself, in the firmware's boot menu (the key differs between vendors: often
  F8, F11, F12 or Esc).
- From a live system, `efibootmgr -v` shows the entries; create them again as in
  [Creating the firmware boot entries](#creating-the-firmware-boot-entries). The copy at `\EFI\BOOT\BOOTX64.EFI` is
  used when entries are missing.

### ZFSBootMenu finds no pool

ZFSBootMenu shows `Unable to import pool, retrying in NN seconds`. In the recovery shell:

```
root # cat /proc/cmdline
root # ls /dev/nvme*
root # zpool import
```

Option C adds:

```
root # zluks list
root # zluks
root # exit
```

- No `/dev/nvme*`: the NVMe driver is not in the image. Check that `/etc/zfsbootmenu/dracut.conf.d/generic.conf`
  exists, or add `add_drivers+=" nvme "` to a file there, and run `generate-zbm`.
- `zpool import` lists the pool but names unsupported features: ZFSBootMenu's ZFS is older than the pool's features;
  rebuild the image on the installed system.
- Option C: `/proc/cmdline` must contain `zbm.luks=UUID=` with the UUID of the **LUKS container**; a pool GUID or a
  file system UUID there is the usual mistake. Fix `Kernel.CommandLine` and run `generate-zbm`. `zluks list` shows
  which LUKS volumes ZFSBootMenu can see; `zluks` unlocks by hand, and after `exit` ZFSBootMenu imports the pool.
- Options A and B, with `zbm.luks=off`: nothing to unlock; look at the first two points.

### Option C: ZFSBootMenu never asks for the passphrase (the image lacks cryptsetup)

ZFSBootMenu never asks for a passphrase and keeps retrying the pool import. In its recovery shell,
`command -v cryptsetup` prints nothing; with the shell runtime `zluks` is missing too, while with the Rust runtime
`zluks` exists but cannot open the container. `sys-fs/cryptsetup` was not installed when the image was built (dracut
then leaves LUKS out without a message). Follow
[Repairing the system from the live medium](#repairing-the-system-from-the-live-medium), run
`emerge --ask sys-fs/cryptsetup` and `generate-zbm` in the chroot, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

After ZFSBootMenu started Gentoo's kernel, Gentoo's initramfs could not use the key file.

| | What you see |
|---|---|
| Option B, key file missing or old | no second prompt; the boot stops in dracut's emergency shell |
| Option C, systemd, key file missing or wrong | systemd-cryptsetup asks for the passphrase |
| Option C, OpenRC, `rd.luks.uuid=` missing | nothing is asked; the boot stops in dracut's emergency shell |
| Option C, OpenRC, key file missing or wrong | dracut's script asks for the passphrase |

In Option B, `zfs load-key` fails without asking. Without `rd.luks.uuid=`, dracut never starts its unlock script and
gives up waiting for the root.

Type the passphrase where one is asked for. In dracut's emergency shell:

- Option B: `zfs load-key -L prompt zroot`, then `exit`;
- Option C: `cryptsetup open /dev/disk/by-partlabel/zroot-luks luks-YOUR-LUKS-UUID`, `zpool import -N zroot`, then
  `exit`.

Whether the boot then goes on was not checked for this guide; the live medium always works. Then, on the running
system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitrd "/boot/initramfs-$(uname -r).img" | grep -E 'zroot\.key|systemd-cryptsetup$|bin/cryptsetup$'
root # lsinitrd -f etc/crypttab "/boot/initramfs-$(uname -r).img"
```

Causes, in order of likelihood:

- the key file is not in the initramfs: the `install_items` line in `/etc/dracut.conf.d/20-*.conf` is missing or
  misspelled; rebuild with the `emerge --config` loop of [Configuring the initramfs](#configuring-the-initramfs);
- Option C: the command line lacks `luks.uuid=` and `luks.key=` (systemd) or `rd.luks.uuid=` (OpenRC), or carries
  another UUID;
- Option C, systemd: `systemd-cryptsetup` is not in the initramfs: `sys-apps/systemd` lacks USE `cryptsetup`;
- Option C, OpenRC: `/etc/crypttab` is not in the initramfs, or its line names another UUID or key path, or writes
  the device in another form than `UUID=<LUKS UUID>`;
- Option B: the passphrase was changed without rebuilding the initramfs;
- an old snapshot or clone was booted, whose initramfs holds an older key.

If ZFSBootMenu **itself** asks twice, that is something else: Option C with `zbm.luks=` missing lets it offer every
LUKS volume it finds.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment. In the recovery shell (Option B: `zfs load-key -L prompt zroot` first,
unless the key is already loaded; the file `keylocation` names is not in ZFSBootMenu's image):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/gentoo
root # ls -l MOUNTPOINT/boot
```

`mount_zfs` prints the mount point to use in the last command. Causes:

- `/boot` in the dataset is empty: the kernels went onto the ESP (`installkernel` has `systemd-boot`, `uki` or
  `efistub` set, or `/etc/kernel/install.conf` names another layout than `compat`);
  `tail /var/log/installkernel.log` shows where the last kernels went;
- a kernel has no `initramfs-<version>.img` beside it, for example after a failed dracut run, or the kernel was
  skipped because its ZFS module was missing; reinstall it with `emerge --config`;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

- In ZFSBootMenu: the default import policy (`zbm.import_policy=hostid`) adopts the host ID of the pool when it
  cannot import it with its own, so this usually resolves itself. Otherwise, in the recovery shell:
  `zpool import -f -N zroot`, then `exit`.
- In Gentoo's initramfs (dracut's emergency shell): `zpool import -f -N zroot`, then `exit`.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; rebuild the initramfs
  with the `emerge --config` loop and run `generate-zbm`. The usual cause is a live medium that imported the pool
  and was shut down without `zpool export`. Always export the pool on a rescue medium.

### The recovery shell and its tools

| Command | Use |
|---|---|
| `zfsbootmenu` (or `exit`) | back to the menu |
| `zpool`, `zfs` | the ZFS tools of the image; `zpool import`, `zpool status` |
| `set_rw_pool POOL`, `set_ro_pool POOL` | re-import the pool read/write or read-only |
| `mount_zfs DATASET` | mount a file system at a temporary place and print it |
| `zfs-chroot DATASET` | chroot into a boot environment (read/write only after `set_rw_pool`) |
| `zsnapshots DATASET` | the snapshot browser |
| `zkexec DATASET KERNEL INITRAMFS` | boot a chosen kernel and initramfs directly |
| `mount_esp DEVICE` | mount an ESP |
| `zluks`, `zluks list`, `zluks status` | Option C: unlock, list and show LUKS volumes |
| `zluks open DEVICE [NAME]`, `zluks close` | Option C: open or close one; `cryptsetup` and `dmsetup` are there too |
| `logs`, `zlogtail`, `zreport`, `zbmcmdline` | logs, a report for bug reports, ZFSBootMenu's command line |
| `help` | the built-in documentation |
| `reboot`, `poweroff`, `firmware-setup` | leave |

### Repairing the system from the live medium

Boot the minimal CD, set the variables, and give the live system the installed system's host ID, so that the import
does not stamp the pool with the live system's:

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
root # zgenhostid -f 0x00bab10c
```

**Option C:**

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

**All options.**

```
root # zpool import -f -N -R /mnt zroot
```

**Option B:**

```
root # zfs load-key -L prompt zroot
```

**All options.**

```
root # zfs mount zroot/ROOT/gentoo
root # zfs mount zroot/home
root # mount "${ESP_PART}" /mnt/boot/efi
```

Then chroot as in [Chrooting](#chrooting), with one difference on the **systemd** profile: there, `/etc/resolv.conf`
of the installed system is a link to `/run/systemd/resolve/stub-resolv.conf`, and `cp --dereference` refuses to
write through a link whose target does not exist. **Skip the `cp` line** and, after the `mount --bind /run` line,
give the link a target on the live system's `/run`, which the chroot shares:

```
root # mkdir -p /run/systemd/resolve
root # cp --dereference /etc/resolv.conf /run/systemd/resolve/stub-resolv.conf
```

This leaves the link in the boot environment as it is. In the chroot, skip `source /root/install.env` (the file was
deleted) and set the variables you need by hand.

Finish as in [Finalizing the installation](#finalizing-the-installation), without the snapshot and without the
`resolv.conf` link: leave the chroot, unmount, `zpool export zroot`, and for Option C `cryptsetup close`. The export
matters: without it, the next import by ZFSBootMenu finds the pool still in use. The installed system does not
export its pool when it shuts down; with the same host ID the import works without `-f`, which is kept for the case
that another system used the pool last. The stored TRIM flag is applied by `cryptsetup open` itself.

**Check:** inside the chroot, `ping -c 2 gentoo.org` gets answers; after the export, `zpool list` reports *no pools
available*.

### Continuing after a reboot of the live system

After a reboot, the live system has lost the variables, its host ID (`/etc/hostid`, written by `zgenhostid`) and,
for Option B, the passphrase file `/etc/zfs/zroot.key`: all lived only in its memory. The pool is still marked as
imported by host ID `00bab10c`. Do not continue the installation before restoring them: a missing host ID would be
copied into Gentoo, and a plain `zpool import` refuses the pool. (If the reboot came before
[Creating the ZFS pool](#creating-the-zfs-pool), only the variables are lost: set them again and repeat the step
you were in.)

Set the variables again as in [Setting variables](#setting-variables), then restore the host ID:

```
root # zgenhostid -f 0x00bab10c
```

**Option C:** open the container again (the commands of
[Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container) for a reboot):

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

**Options A and B:** `export POOL_DEV="${POOL_PART}"`.

**All options.** Import the pool without mounting it:

```
root # zpool import -N -R /mnt "${POOL}"
```

**Option B:** load the key by typing the passphrase, mount the root dataset, and put the passphrase file back on the
live system. If [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file) was done already, copy
it from the new system:

```
root # zfs load-key -L prompt "${POOL}"
root # zfs mount "${POOL}/ROOT/${BE}"
root # mkdir -p /etc/zfs
root # cp -p /mnt/etc/zfs/zroot.key /etc/zfs/zroot.key
```

If that step was not done yet, `cp` reports a missing file: write the file again with the `bash -c 'umask 077; …'`
and `chmod 000` lines of [Creating the ZFS pool](#creating-the-zfs-pool), with the **same** passphrase.

**All options.** Mount the datasets (on Option B the root dataset is mounted already, and `zfs mount` says so), and
write the cache file again:

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
```

If the installation had reached [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file),
mount the ESP again with `mount "${ESP_PART}" /mnt/boot/efi`. If it had reached [Chrooting](#chrooting), repeat the
`mount` lines of that section and enter the chroot again.

**Check:**

```
root # hostid
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`hostid` prints `00bab10c`, `/mnt` is `zroot/ROOT/gentoo` and `/mnt/home` is `zroot/home`, and for Option B the
key status is `available` and `/etc/zfs/zroot.key` exists again.

### installkernel stops inside the chroot

`emerge` reports a failed kernel installation with "WARNING: Dracut will be run from inside a chroot but no cmdline
for dracut was configured". No file in `/etc/dracut.conf.d/` sets `hostonly_cmdline="no"`, `hostonly="no"` or
`kernel_cmdline=`. Write `/etc/dracut.conf.d/10-zfs.conf` as in [Kernel and ZFS](#kernel-and-zfs) and reinstall
the kernel with the `emerge --config` loop.

If `/boot` stays without a kernel and no error is shown, the ZFS module for the new kernel was missing when the
kernel was installed: the installer skips a kernel without a ZFS module on a ZFS root. `emerge --ask --oneshot
sys-fs/zfs` builds it and installs the kernel again.

### zfs.ko is missing after a kernel update

The kernel is newer than the installed ZFS supports. `dist-kernel-cap` on `sys-fs/zfs` (on by default) keeps Portage
from installing such a kernel; check that it is not disabled, then run
`emerge --ask --update --deep --newuse @world`. The supported range is in the ebuilds:

```
root # grep -h MODULES_KERNEL_MAX /var/db/repos/gentoo/sys-fs/zfs/*.ebuild
```

### generate-zbm builds the image from the wrong kernel

`generate-zbm --dry-run --debug` names another kernel than the newest one. It prefers `/boot/vmlinuz*` over
`/boot/kernel*`, so on the systemd profile a leftover `vmlinuz-*` file wins over every newer `kernel-*`; move such
files away. To pin a kernel, add `Version: "%{current}"` (the running kernel) to the `Kernel` section of
`/etc/zfsbootmenu/config.yaml`, or run `generate-zbm --kver "$(uname -r)"`. On OpenRC, the `vmlinuz-<version>.old`
copies are skipped with a warning and never chosen; see
[Building and checking the image](#building-and-checking-the-image).

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption does not encrypt metadata about the pool's structure: dataset and snapshot names, the dataset
  hierarchy, properties, file sizes, file holes and deduplication tables. File and volume data, file attributes,
  ACLs, permission bits and directory listings are encrypted. So the names `zroot/ROOT/gentoo` and `zroot/home`,
  every snapshot name, how much each holds, and all properties, including `org.zfsbootmenu:commandline`, can be read
  from the SSD.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Gentoo's initramfs in
  `/boot`, also inside the pool. It is safe only there. Any copy of the boot environment that is not encrypted
  carries it in clear: a `zfs send` without `-w` (raw), a file-level backup of `/etc` or `/boot`, an initramfs copied
  to the ESP. On the running system, root can read the passphrase. On OpenRC, the installer keeps the previous
  initramfs of a version as `initramfs-<version>.img.old`, which holds the passphrase that was current then;
  [Changing the passphrase](#changing-the-passphrase) removes these copies.
- Changing the passphrase does not change the key the data is encrypted with: `zfs change-key` "does not necessarily
  protect existing or newly-written data" (OpenZFS). It is no cure for a passphrase that someone learned together
  with a copy of the disk.
- **Never** place encryption keys inside a ZFSBootMenu image.

**Option C.**

- The ESP is not encrypted and holds only ZFSBootMenu's image and its copies. They contain no key: plain key files
  are never copied into the image, and the build warns when a crypttab names one.
- The key file opens the container. It is safe because `/etc/cryptsetup-keys.d` and the initramfs in `/boot` are
  inside the encrypted pool. It stops being safe the moment an initramfs with the key is written anywhere else: the
  ESP, an unencrypted partition, a backup, or a unified kernel image. Snapshots and `zfs send` streams of the root
  dataset contain it too; treat them like the pool. On OpenRC, the installer keeps the previous initramfs of a
  version as `initramfs-<version>.img.old`, which holds the key file that was current then.
- A LUKS header backup contains the key slots as they were: an old passphrase still opens a restored header. Store
  backups offline and replace them after key changes. Only `cryptsetup reencrypt` changes the volume key itself.
- ZFSBootMenu closes the container before `kexec`, which wipes the volume key from kernel memory.
- TRIM pass-through reveals which blocks are unused, not their content.

**All options.**

- **The ESP is not encrypted.** Anyone with access to the disk, or root on the system, can replace ZFSBootMenu's
  image with one that records the passphrase. Encryption protects data at rest, for example a stolen powered-off
  laptop, not against someone who can modify the computer and wait for you to type the passphrase. Secure Boot with
  signed images, or measured boot with a TPM, narrows that; neither is set up by this article. `contrib/zbm-sign.pl`,
  a `generate-zbm` post-hook described in [Building ZFSBootMenu](../../general/building.rst), signs images with
  `sbctl` or `sbsigntools`.
- Once the pool is unlocked, the recovery shell and the command-line editor are open to anyone at the keyboard.
- Hooks run as root in ZFSBootMenu and during `generate-zbm`; install only hooks you have read.
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed
  before updating, keep the backup entries, and keep the rescue medium.

The threat model of this fork, with what it protects against and what it does not, is in
[docs/dev/threat-model.md](../../dev/threat-model.md).

## See also

- This repository: [ZFS Pools on LUKS Volumes](../../general/luks.rst),
  [Native Encryption](../../general/native-encryption.rst), [Building ZFSBootMenu](../../general/building.rst),
  [UEFI Booting](../../general/uefi-booting.rst), [Boot Environments and You](../../general/bootenvs-and-you.rst)
- [LUKS administration](../../luks/administration.rst), [ZFS Versions and Pool Features](../../luks/zfs-versions.rst),
  [Testing](../../luks/testing.rst)
- [zfsbootmenu(7)](../../man/zfsbootmenu.7.rst), [generate-zbm(5)](../../man/generate-zbm.5.rst),
  [generate-zbm(8)](../../man/generate-zbm.8.rst), [zbm-kcl(8)](../../man/zbm-kcl.8.rst), and the manual pages
  `zfsbootmenu-luks(7)` and `zluks(8)` (`docs/man/zfsbootmenu-luks.7.rst`, `docs/man/zluks.8.rst`)
- [Gentoo Linux on a LUKS-encrypted ZFS root on NVMe](zfs-luks-v4.md), Option C on the systemd profile in more depth
- [Adding ZFSBootMenu to an existing LUKS-encrypted ZFS root](existing-luks-system.md), for a Gentoo system that is
  already installed
- [The threat model](../../dev/threat-model.md)
- Gentoo: [Handbook:AMD64](https://wiki.gentoo.org/wiki/Handbook:AMD64), [ZFS](https://wiki.gentoo.org/wiki/ZFS),
  [ZFS/rootfs](https://wiki.gentoo.org/wiki/ZFS/rootfs), [Dracut](https://wiki.gentoo.org/wiki/Dracut),
  [Installkernel](https://wiki.gentoo.org/wiki/Installkernel),
  [Distribution kernel](https://wiki.gentoo.org/wiki/Project:Distribution_Kernel),
  [Dm-crypt](https://wiki.gentoo.org/wiki/Dm-crypt)
- External: the OpenZFS documentation (<https://openzfs.github.io/openzfs-docs/>), `gdisk(8)`, `cryptsetup(8)`,
  [systemd-cryptsetup-generator(8)][sdcg] for the `luks.*` parameters, and
  [dracut.cmdline(7)](https://man7.org/linux/man-pages/man7/dracut.cmdline.7.html) for `rd.luks.uuid=`

[sdcg]: https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html
