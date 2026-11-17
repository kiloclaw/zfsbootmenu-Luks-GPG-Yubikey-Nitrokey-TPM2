# Debian Testing on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install Debian Testing from scratch on a **single NVMe SSD** with a **ZFS root
pool**. The system boots with the ZFSBootMenu build from this repository (branch `claude/zbm-refactor`).

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Debian's initramfs then unlocks the pool
again with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from the documentation of Debian, of OpenZFS and of this repository. The project has not
> run it from start to finish. `claude/zbm-refactor` is a development branch, not a release.
> The project's VM tests boot ZFSBootMenu images built with dracut, and hand over to a test system (Linux 7.3,
> OpenZFS 2.4.4) whose initramfs is built with dracut. No Debian system and no initramfs-tools initramfs, which this
> guide uses, is booted by them.
> Keep a live medium at hand until the system has booted several times, and read
> [What the project has tested](#what-the-project-has-tested) before you start.

Debian Testing changes every day. The guide was written in November 2026, when testing was called **forky** (the
future Debian 14). Where a fact about testing could not be confirmed from here, the guide says so at that place and
gives a command that shows the answer on your system. Read
[What differs on Debian Testing](#what-differs-on-debian-testing) before you start.

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
  - [What differs on Debian Testing](#what-differs-on-debian-testing)
  - [Running debootstrap](#running-debootstrap)
  - [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file)
  - [Chrooting](#chrooting)
  - [Configuring APT for testing](#configuring-apt-for-testing)
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
  - [A new kernel has no ZFS module](#a-new-kernel-has-no-zfs-module)
  - [APT wants to install dracut or remove initramfs-tools](#apt-wants-to-install-dracut-or-remove-initramfs-tools)
  - [The live system cannot install ZFS](#the-live-system-cannot-install-zfs)
- [Security notes](#security-notes)
- [See also](#see-also)

[no-cryptsetup]: #option-c-zfsbootmenu-never-asks-for-the-passphrase-the-image-lacks-cryptsetup
[second-unlock]: #the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs
[hostid-refused]: #the-pool-is-refused-wrong-host-id-or-last-used-by-another-system

## Overview

### Choosing an option

| | Option A | Option B | Option C |
|---|---|---|---|
| Pool | not encrypted | ZFS native encryption (`aes-256-gcm`) | inside a LUKS2 container |
| Passphrase typed | never | once, in ZFSBootMenu | once, in ZFSBootMenu |
| Second unlock | none | key file `/etc/zfs/zroot.key` | key file `/etc/cryptsetup-keys.d/zroot.key` |
| Partition 2 type | `BF00` | `BF00` | `8309` |
| Visible without the passphrase | everything | names, sizes, properties | the container's size, its LUKS header |
| Extra software | none | none | `cryptsetup` |

- *Second unlock*: done by Debian's initramfs, after ZFSBootMenu; see [Boot process](#boot-process).
- *Visible without the passphrase*, Option B: dataset names, sizes, properties and snapshot names; not file contents
  or file names.
- *Extra software*, Option C: `cryptsetup` on the live system, in the new system and in ZFSBootMenu's image.

Option B stores the **passphrase itself** in its key file: OpenZFS has one key per encryption root, and the key file
is the file that the pool's `keylocation` property names. Option C stores a **different, random** key in a second
LUKS key slot. The two can be combined; this guide does not.

All three options use this repository's ZFSBootMenu. Upstream ZFSBootMenu cannot unlock LUKS volumes; for Options A
and B it would work too, but one build for all three keeps the steps the same.

### Boot process

**Option A**

1. The UEFI firmware starts **ZFSBootMenu**, one EFI file on the EFI system partition (ESP).
2. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/debian` with a countdown of 10 seconds.
   When it ends, the pool's `bootfs` is booted; a key press opens the menu instead.
3. ZFSBootMenu loads Debian's kernel and initramfs from `/boot` **inside the boot environment** and starts the
   kernel with `kexec`.
4. Debian's initramfs imports the pool, mounts the root file system and starts systemd.

**Option B**

1. The UEFI firmware starts ZFSBootMenu from the ESP.
2. ZFSBootMenu imports the pool and shows `zroot/ROOT/debian` with the countdown.
3. The `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in ZFSBootMenu's
   image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime this prompt comes
   **after** the countdown.
4. ZFSBootMenu loads the kernel and initramfs from `/boot` inside the boot environment and starts the kernel with
   `kexec`.
5. Debian's initramfs imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the initramfs**,
   without asking, then mounts the root file system.

**Option C**

1. The UEFI firmware starts ZFSBootMenu from the ESP.
2. ZFSBootMenu reads `zbm.luks=` from its kernel command line, asks for the **LUKS passphrase** and opens the
   container.
3. ZFSBootMenu imports the pool and shows `zroot/ROOT/debian` with the countdown.
4. ZFSBootMenu loads the kernel and initramfs from `/boot` inside the boot environment. Before `kexec` it exports
   the pool and closes the container, which wipes the volume key from memory.
5. Debian's initramfs unlocks the container again with the key file `/etc/cryptsetup-keys.d/zroot.key` **inside the
   initramfs**, as `/etc/crypttab` tells it, without asking. It then imports the pool and mounts the root file
   system.

Why a second unlock in Options B and C: `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. ZFSBootMenu does not replace Debian's initramfs; Debian's initramfs must unlock the pool again,
exactly as it would without ZFSBootMenu.

In every option, ZFSBootMenu passes the command line from the ZFS property `org.zfsbootmenu:commandline`, the root
argument `root=zfs:zroot/ROOT/debian` and the host ID (`spl.spl_hostid=0x00bab10c`) to Debian's kernel.

### Where the keys are

**Option A**: there are none.

| Key | Option | Stored where | Used by |
|---|---|---|---|
| ZFS passphrase | B | your memory | ZFSBootMenu at every boot; a live system for repairs |
| The passphrase in `/etc/zfs/zroot.key` | B | the pool's `/etc`; Debian's initramfs | Debian's initramfs |
| LUKS passphrase (key slot 0) | C | your memory | ZFSBootMenu at every boot; a live system for repairs |
| Key file `/etc/cryptsetup-keys.d/zroot.key` (slot 1) | C | the pool's `/etc`; the initramfs | Debian's initramfs |

Debian's initramfs is the file `/boot/initrd.img-<version>`, which is inside the encrypted pool too; it uses the
key file after `kexec`. Option C's key file is random, not the passphrase.

The key file in Debian's initramfs is safe **only because** the file and the initramfs both live inside the
encrypted pool (`/etc` and `/boot`): they cannot be read before the passphrase has been typed. ZFSBootMenu's own
image lives on the unencrypted ESP and never contains a key.

### What the project has tested

- The project's VM tests boot ZFSBootMenu images that the build container (Ubuntu 26.10, Linux 7.3, OpenZFS 2.4.4,
  dracut 112) builds with dracut and `hostonly="no"`. This guide builds the image with Debian's `dracut-core`, on
  Debian Testing, with Debian's kernel and ZFS. That combination is **not** tested.
- The boot environment of the tests is a test system with a dracut initramfs, not an installed distribution.
  "Boot environments whose initramfs is not built by dracut (for example mkinitcpio or initramfs-tools)" are listed
  as not covered. Debian's initramfs-tools initramfs, which unlocks the pool a second time in Options B and C, is
  therefore not tested by the project.
- The suite runs the **Rust runtime** by default. The **shell runtime**, which this guide uses unless you choose
  otherwise in [Choosing the runtime](#choosing-the-runtime), runs only when its tests are named; its last full run
  was on 20 October 2026.
- **Option A**: unencrypted pools are booted with the Rust runtime in the suite; the shell runtime's tests run only
  when named.
- **Option B**: the tests check that ZFSBootMenu asks for the ZFS passphrase. The single prompt of Option B, with
  the key file in the system's initramfs, is **not** tested: the test system's initramfs holds no key file and asks
  again. It is upstream ZFSBootMenu's documented design (`docs/general/native-encryption.rst`).
- **Option C**: the design of ZFSBootMenu's part is tested, on an NVMe disk with 4 KiB sectors, a LUKS2 partition
  with 4 KiB encryption sectors and the TRIM flag stored in its header, selected with `zbm.luks=UUID=`, booted from
  `EFI/BOOT/BOOTX64.EFI`. The second unlock in the tests uses dracut and `luks.*` parameters, not initramfs-tools
  and `/etc/crypttab` as this guide does.
- Real hardware, Secure Boot and installed distributions are not covered. The last full run of the suite was on
  15 November 2026.

Details: [Testing](../../luks/testing.rst).

## Requirements

### Hardware

- An x86_64 computer with an NVMe SSD that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware: the image built here is not signed.
- An internet connection.
- Room on the SSD for the build: about 1 GB for the clone of this repository, plus Cargo's build directory, whose
  size was not measured.

### Installation media

This guide uses the **Debian 13 (trixie) live image**, from <https://www.debian.org/CD/live/>, which installs
Debian Testing just as well: `debootstrap` downloads testing's packages from the network. Any flavour of the live
image works; the commands below run in a terminal. Write the image to a USB stick and boot it in **UEFI** mode.

The live system must provide, or be able to install:

| Program | Needed for | Options |
|---|---|---|
| ZFS kernel module and `zpool`, `zfs`, `zgenhostid` | creating the pool | A B C |
| `gdisk` | partitioning | A B C |
| `mkfs.vfat` (dosfstools) | formatting the ESP | A B C |
| `wipefs`, `blkdiscard`, `lsblk`, `blockdev` (util-linux) | erasing and checking | A B C |
| `cryptsetup` 2.4 or later | the LUKS container | C |
| `nvme` (nvme-cli) | identifying the SSD; optional 4 KiB sectors | A B C |
| `debootstrap` | installing Debian Testing | A B C |

Open a terminal and become root:

```
user $ sudo -i
```

Make sure Secure Boot is disabled **before** installing ZFS: with Secure Boot on, the kernel refuses the ZFS module
that the live system is about to build. The last number printed is `1` when Secure Boot is enabled and `0` when it
is disabled:

```
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
```

If the file does not exist, the firmware has no Secure Boot or the live system did not boot in UEFI mode (see the
Check of [Preparing the NVMe SSD](#preparing-the-nvme-ssd)). If it prints `1`, turn Secure Boot off in the firmware
setup and boot the live system again.

Add `contrib` (where Debian keeps ZFS) to the live system's package sources and install the tools. The live system
runs Debian 13, so these are trixie's sources:

```
root # cat > /etc/apt/sources.list <<EOF
deb http://deb.debian.org/debian/ trixie main contrib non-free-firmware
EOF
root # apt update
root # apt install debootstrap gdisk dosfstools nvme-cli parted cryptsetup-bin dkms linux-headers-$(uname -r)
root # apt install zfsutils-linux zfs-dkms
root # modprobe zfs
```

- `zfs-dkms` compiles the ZFS module for the live kernel. This takes several minutes and happens in memory, so the
  computer needs a few GB of RAM.
- `zfs-dkms` shows a note that the licences of OpenZFS and Linux are incompatible; confirm it with **OK**.
- If `apt` cannot find `linux-headers-` followed by the live kernel's version, the live image is older than the
  kernel in the archive. Download the current live image.
- `cryptsetup-bin` holds the `cryptsetup` program without the initramfs integration, which the live system does not
  need.

### Conventions

| Notation | Meaning |
|---|---|
| `user $` | run as the live system's own user; do not type the prompt |
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |
| `<Enter>` | in a transcript: press Enter without typing anything |
| **Option A.** / **Option B.** / **Option C.** | the next paragraph and commands are for that option only |
| **All options.** | the text is for every option again |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Preparing the NVMe SSD

Boot the live system in UEFI mode, connect to the network, open a root shell and install the tools as shown in
[Installation media](#installation-media).

**Check:**

```
root # ls /sys/firmware/efi
root # zfs version
root # ls /usr/share/zfs/compatibility.d/ | grep -x openzfs-2.2-linux
root # dpkg -s gdisk | grep '^Version'
root # cryptsetup --version
```

- The first command lists files; otherwise the live system booted in legacy BIOS mode.
- `zfs version` prints the versions of the tools and the kernel module (Debian 13 has OpenZFS 2.3).
- The third command prints `openzfs-2.2-linux`: the live system's ZFS knows the compatibility set used below.
- gdisk is version 1.0.9 or later (Debian 13 has 1.0.10).
- `cryptsetup --version` prints a version of 2.4 or later. It matters for Option C only.

### Identifying the SSD

List the NVMe namespaces:

```
root # nvme list
```

Each line shows a device such as `/dev/nvme0n1` with its model, serial number and size. `nvme0` is the controller,
`n1` its first namespace; partitions are named `/dev/nvme0n1p1`, `/dev/nvme0n1p2`, and so on. Without `nvme-cli`,
`lsblk -d -o NAME,SIZE,MODEL,SERIAL,TRAN` shows the same (the `TRAN` column says `nvme`).

This guide uses the kernel name `/dev/nvme0n1`, because gdisk, `blkdiscard` and `efibootmgr` take it and a computer
with one NVMe SSD has only `nvme0n1`. Kernel names can change between boots when a computer has **several** NVMe
SSDs. In that case, compare the serial number with the output of

```
root # ls -l /dev/disk/by-id/ | grep nvme | grep -v -- -part
```

before every step that erases something, and after every reboot of the live system. ZFS itself finds the pool by
the labels on the disk, not by the path, so a changed name does not stop the pool from importing later.

### Setting variables

Set these variables once, in the root shell of the live system. Replace `/dev/nvme0n1` if your SSD has another
name:

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
root # export POOL=zroot
root # export BE=debian
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the EFI system partition (`p` before the number on NVMe) |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `debian` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}` |
| `POOL_DEV` | see below | A B C | the device `zpool create` gets |
| `LUKS_UUID` | see below | C | the UUID of the LUKS container |

`POOL_DEV` and `LUKS_UUID` are set later. `POOL_DEV` is `${POOL_PART}` for Options A and B, set in
[Creating the ZFS pool](#creating-the-zfs-pool), and `/dev/mapper/luks-${LUKS_UUID}` for Option C, set in
[Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container) after `luksFormat` created
the UUID.

The name of the boot environment is free; ZFSBootMenu does not read it. It recognizes Debian from
`/etc/os-release` inside the boot environment.

Some paths are written out in full below, because files cannot contain shell variables. If you change `POOL`, change
`zroot` in these paths too:

| Path | Option | What |
|---|---|---|
| `/etc/zfs/zroot.key` | B | the ZFS passphrase file |
| `/etc/cryptsetup-keys.d/zroot.key` | C | the random LUKS key file for the second unlock |
| `/dev/mapper/luks-<LUKS UUID>` | C | the opened container |
| `/etc/hostid` | A B C | the host ID, the same on the live system, the new system and both initramfs images |
| `/boot/efi` | A B C | mount point of the ESP in the new system |
| `/boot/efi/EFI/zbm/vmlinuz.EFI`, `vmlinuz-backup.EFI` | A B C | ZFSBootMenu's image and its backup |
| `/usr/src/zbm` | A B C | the clone of this repository |

> **Tip**
> If only the terminal was closed, open a root shell and set these variables again; for Option C also `LUKS_UUID`
> and `POOL_DEV` ([Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container)), and for
> Options A and B `POOL_DEV` ([Creating the ZFS pool](#creating-the-zfs-pool)). If the **live system** was restarted,
> more is lost; follow the Tip at the end of [Mounting the pool below /mnt](#mounting-the-pool-below-mnt).

**Check:**

```
root # lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"
```

The model and serial number are those of the SSD you intend to erase.

### Optional: switching to 4 KiB sectors

Many NVMe SSDs ship with 512-byte logical sectors and also support 4096-byte sectors, which can be slightly faster.
Show the formats the SSD supports:

```
root # nvme id-ns -H "${DISK}" | grep -E '^LBA Format'
```

Example output:

```
LBA Format  0 : Metadata Size: 0   bytes - Data Size: 512 bytes - Relative Performance: 0x2 Good (in use)
LBA Format  1 : Metadata Size: 0   bytes - Data Size: 4096 bytes - Relative Performance: 0x1 Better
```

If a `4096 bytes` format with `Metadata Size: 0` is listed and not `(in use)`, you can switch to it. Use its number
(`1` in the example) with `--lbaf`:

> **Warning**
> `nvme format` erases the whole SSD immediately.

```
root # nvme format "${DISK}" --lbaf=1 --force
root # nvme id-ns -H "${DISK}" | grep 'in use'
```

If only 512-byte formats are listed, skip this step. The rest of the guide works either way: ZFS gets `ashift=12`
and, in Option C, LUKS gets `--sector-size 4096`, so both use 4 KiB blocks on either format.

On a 4 KiB format, gdisk counts in 4096-byte sectors: the sector numbers in the transcript below are about eight
times smaller, and gdisk prints `Partitions will be aligned on 256-sector boundaries` instead of `2048-sector`
(1 MiB either way). What you type is the same.

**Check:** the line that ends in `(in use)` shows `Data Size: 4096 bytes`, or you skipped the step.

### Erasing the SSD

Old ZFS labels, LUKS headers and file system signatures confuse `zpool create`, `cryptsetup` and the live system's
automatic mounting. Discarding also tells the SSD that every block is free.

> **Warning**
> The commands in this section destroy everything on `${DISK}` at once and cannot be undone. Check the output of
> `lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"` one more time.

**Starting over** after a failed attempt: leave any chroot first (`exit`), then release what the earlier attempt
left mounted or open:

```
root # umount -R -l /mnt 2>/dev/null
root # zpool export -a
root # lsblk -o NAME,TYPE,MOUNTPOINTS "${DISK}"
```

An opened LUKS container (Option C) is a line of TYPE `crypt` below a partition, with no mount point; close it with
`cryptsetup close NAME`, using the name of that line. Otherwise the commands below fail with *Device or resource
busy*. On a first attempt, skip this.

```
root # lsblk -o NAME,SIZE,FSTYPE,MOUNTPOINTS "${DISK}"
root # swapoff -a
root # zpool export -a
root # for part in "${DISK}"p*; do [ -b "${part}" ] && zpool labelclear -f "${part}"; done
root # for part in "${DISK}"p*; do [ -b "${part}" ] && wipefs -a "${part}"; done
root # blkdiscard -f "${DISK}"
root # wipefs -a "${DISK}"
```

- The first command must show no mount point. Unmount anything the live system mounted from the SSD
  (`umount /media/...`) before you go on.
- `swapoff -a` and `zpool export -a` release a swap partition or an old pool that the live system took into use;
  `no pools available to export` is not an error.
- The two loops clear the signatures **inside** old partitions: ZFS keeps four labels per device and LUKS2 two
  headers, and removing only the partition table would leave them where a new partition may find them.
  `zpool labelclear` prints an error for a partition that holds no ZFS label; that is expected. On an SSD without
  partitions the loops do nothing.
- If `blkdiscard` fails with *Operation not supported*, the SSD or its controller does not pass discards on;
  continue with the next command.
- If you ran `nvme format` above, the SSD is empty already; the commands do no harm.

**Check:**

```
root # wipefs "${DISK}"
root # lsblk "${DISK}"
```

`wipefs` prints nothing, and `lsblk` shows the SSD without partitions.

### Partitioning with gdisk

The firmware needs a FAT partition for ZFSBootMenu (the ESP); everything else is one partition for the pool or the
LUKS container.

| Partition | Device | Size | gdisk type | Name | Use |
|---|---|---|---|---|---|
| 1 | `${ESP_PART}` (`/dev/nvme0n1p1`) | 1 GiB | `EF00` EFI system partition | `EFI` | the ESP, FAT32, at `/boot/efi` |
| 2, Options A and B | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `BF00` Solaris root | `zroot` | the ZFS pool |
| 2, Option C | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `8309` Linux LUKS | `zroot-luks` | the LUKS2 container |

- The ESP holds only ZFSBootMenu, but several copies of it: the image, its backup, a copy at
  `\EFI\BOOT\BOOTX64.EFI`, and perhaps a known-good copy that you keep. 1 GiB leaves room for all of them. No
  Debian kernel or initramfs is ever stored on the ESP.
- ZFS ignores the partition type; `BF00` and `8309` are labels for people and tools.
- The names `EFI` and `zroot-luks` give the stable paths `/dev/disk/by-partlabel/EFI` and
  `/dev/disk/by-partlabel/zroot-luks`, which [Usage](#usage) uses.
- gdisk aligns the start of each partition to 1 MiB, and since version 1.0.9 it offers an aligned **end** as the
  default as well. So pressing Enter at the second partition's `Last sector` prompt gives a size that is a whole
  number of MiB, which LUKS with 4 KiB sectors (Option C) needs.

Start gdisk on the SSD. What you type follows each prompt; `<Enter>` means pressing Enter alone. The numbers are
those of a 1 TB SSD with 512-byte sectors and differ on yours: never type the numbers, press Enter for the
defaults. gdisk changes nothing on the SSD until `w`; `q` leaves without writing.

| Step | Command | Answers |
|---|---|---|
| New, empty GPT | `o` | `y` |
| Partition 1, the ESP | `n` | `<Enter>`, `<Enter>`, `+1G`, `ef00` |
| Partition 2, the rest | `n` | `<Enter>`, `<Enter>`, `<Enter>`, then `bf00` (Options A, B) or `8309` (Option C) |
| Name partition 1 | `c` | `1`, `EFI` |
| Name partition 2 | `c` | `2`, then `zroot` (Options A, B) or `zroot-luks` (Option C) |
| Print and compare with the table above | `p` | |
| Write and leave | `w` | `y` |

**Options A and B.**

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

**Option C.** The same session, with two different answers. For the type of partition 2, type `8309`:

```
Hex code or GUID (L to show codes, Enter = 8300): 8309
Changed type of partition to 'Linux LUKS'
```

and name partition 2 `zroot-luks`:

```
Command (? for help): c
Partition number (1-2): 2
Enter name: zroot-luks
```

The `p` listing then ends with `8309  zroot-luks`.

**All options.** What you may see that the transcript does not show:

- The `Partition table scan` lines differ when an old table is still found (`MBR: protective`, `GPT: present` or
  `GPT: damaged`); `o` replaces whatever is there. After the erase above, all four lines say `not present`.
- The version line and all sector numbers differ from the transcript.
- If gdisk ends with *Warning: The kernel is still using the old partition table*, run `partprobe "${DISK}"`, or
  reboot the live system and set the variables again.
- A mistake before `w` costs nothing: `q` quits without saving, `d` deletes a partition.

Make the kernel see the new partitions, and format the ESP:

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
- `lsblk` lists `nvme0n1p1` (1G, `vfat`, `EFI`) and `nvme0n1p2` (no file system; `zroot` or `zroot-luks`).
- The `echo` prints `0`: the size of partition 2 is a whole number of MiB. For Options A and B another number does
  no harm. For Option C, start `gdisk "${DISK}"` again, delete partition 2 (`d`, `2`) and create it again with `n`:
  press Enter at the number and first-sector prompts; at the `Last sector` prompt type the number that
  `echo $(( (LAST + 1) / 2048 * 2048 - 1 ))` prints, with `LAST` replaced by the larger number in the prompt's range;
  answer the `Hex code` prompt with `8309`. Name it again with `c`, `2`, `zroot-luks` (the new partition is called
  `Linux LUKS` otherwise, and `/dev/disk/by-partlabel/zroot-luks` would not exist), write with `w`, `y`, run
  `udevadm settle`, and repeat this Check.
- `wipefs` prints nothing. If it prints a line, run `wipefs -a "${POOL_PART}"`.

### Option C only: creating the LUKS container

Skip this section for Options A and B.

The container encrypts everything in partition 2; the pool is created inside it. `luksFormat` asks you to type
`YES` in capitals, then for the passphrase twice:

```
root # cryptsetup luksFormat --type luks2 --sector-size 4096 "${POOL_PART}"
```

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it
> before any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

- `--sector-size 4096` makes the container encrypt in 4 KiB sectors, which match `ashift=12` and the SSD. It needs a
  partition whose size is a multiple of 4096 bytes. If `luksFormat` prints *Device size is not aligned to requested
  sector size.*, go back to the Check of [Partitioning with gdisk](#partitioning-with-gdisk).
- The other settings are cryptsetup's LUKS2 defaults; the passphrase slot uses the memory-hard Argon2id.

Open the container as `luks-<UUID>`, and store the TRIM flag **in the LUKS2 header** with `--persistent`:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open --allow-discards --persistent "${POOL_PART}" "luks-${LUKS_UUID}"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
```

- `luks-<UUID>` is the name that ZFSBootMenu gives the container when it opens it, and the name the system uses
  later. The same name everywhere keeps the device path recorded in the pool the same.
- With `--allow-discards --persistent`, the container passes TRIM requests through wherever it is opened: in
  ZFSBootMenu, in Debian's initramfs and on a live system. The pool's `autotrim=on` needs that.
- Optional: adding `--perf-no_read_workqueue --perf-no_write_workqueue` to the same `cryptsetup open` line makes the
  kernel encrypt directly instead of in work queues, which lowers latency on fast NVMe SSDs. They are stored in the
  header too.

> **Note**
> Passing TRIM through reveals which blocks are unused, but not their content. This is the usual trade-off for
> SSDs; leave out `--allow-discards` if that matters to you, and set `autotrim=off` on the pool.

**Check:**

```
root # cryptsetup luksDump "${POOL_PART}" | grep -E '^Flags|sector'
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
root # echo "${POOL_DEV}"
```

The header shows `Flags: allow-discards` and `sector: 4096 [bytes]`; the status shows `type: LUKS2`,
`sector size: 4096` and `flags: discards` (and the two work-queue flags if you added them); `POOL_DEV` is
`/dev/mapper/luks-` followed by the UUID.

After a restart of the live system, the container must be opened again; the stored flags apply by themselves. The
Tip at the end of [Mounting the pool below /mnt](#mounting-the-pool-below-mnt) lists every step, this one among them.

### Creating the ZFS pool

**All options.** ZFS records in the pool the host ID of the system that imported it, and refuses a normal import by
another host while the pool was not exported. ZFSBootMenu, Debian's initramfs and the running system all import
this pool, so they must agree. Set a fixed host ID on the live system now; it is copied into the new system later:

```
root # zgenhostid -f 0x00bab10c
```

**Option B.** Write the passphrase into the file that `keylocation` will name. `zpool create` reads it from there,
and the same file later lets Debian's initramfs load the key without asking. The command reads the passphrase twice
without showing it:

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
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it
> before any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

- You type it again in [Mounting the pool below /mnt](#mounting-the-pool-below-mnt), which catches a typing mistake
  before anything is installed.
- The file must stay at `/etc/zfs/zroot.key` on the live system until the installation is finished. It lives in the
  live system's memory: a restart of the live system loses it (see the Tip at the end of
  [Mounting the pool below /mnt](#mounting-the-pool-below-mnt)).

Check the file:

```
root # ls -l /etc/zfs/zroot.key
root # wc -l < /etc/zfs/zroot.key
```

The mode is `----------` and owned by root, and `wc` prints `1`: one line, the passphrase.

**Options A and B.** The pool goes directly on partition 2:

```
root # export POOL_DEV="${POOL_PART}"
```

(Option C set `POOL_DEV` to the opened container.)

**Options A and C.** Create the pool:

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

**Option B.** Create the pool with three more `-O` lines:

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

**All options.**

| Option | Meaning |
|---|---|
| `ashift=12` | 4 KiB blocks, matching the SSD (and the LUKS sector size in Option C) |
| `autotrim=on` | sends TRIM for freed space continuously |
| `compatibility=openzfs-2.2-linux` | only the features of OpenZFS 2.2, so that ZFS 2.2 or later can import it |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | everything the pool mounts during the installation appears below `/mnt` |
| `encryption=aes-256-gcm` (B) | the cipher; fast on current x86_64 processors |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key; it must be `passphrase` |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file written above; Debian's initramfs will hold a copy of it |

The compatibility set can be relaxed later ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).
In Option B the encryption root is the pool's root dataset, so every dataset created later inherits the
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

ZFSBootMenu treats every file system with `mountpoint=/` that has a kernel and an initramfs in its `/boot` as a
**boot environment**. User data lives outside it, so that rolling the system back does not roll back `/home`.

```
root # zfs create -o mountpoint=none "${POOL}/ROOT"
root # zfs create -o mountpoint=/ -o canmount=noauto "${POOL}/ROOT/${BE}"
root # zfs create -o mountpoint=/home "${POOL}/home"
root # zpool set bootfs="${POOL}/ROOT/${BE}" "${POOL}"
```

| Dataset | Properties | Purpose |
|---|---|---|
| `zroot/ROOT` | `mountpoint=none` | container for boot environments; carries the properties they inherit |
| `zroot/ROOT/debian` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, including `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/debian` | the boot environment ZFSBootMenu boots after the countdown |

- `canmount=noauto` is required on every file system with `mountpoint=/`. It is not inherited: a boot environment
  you create later needs it set again.
- Keep the system in one file system. Separate datasets for `/var` or `/usr` would not be rolled back together
  with the rest of the system. More datasets for user data (`/srv`, for example) are your choice.

> **Important**
> `/boot` is a directory of `zroot/ROOT/debian`, not a file system of its own and not the ESP. ZFSBootMenu reads
> Debian's kernel and initramfs from there, and in Options B and C the initramfs with the key file is then inside
> the encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/debian`.

### Setting the ZFSBootMenu properties

ZFSBootMenu passes the value of the property `org.zfsbootmenu:commandline` to Debian's kernel. It is set on
`zroot/ROOT`, so every future boot environment inherits it. Debian's initramfs needs nothing on the command line to
unlock the pool: Option B finds the key file through `keylocation`, and Option C through `/etc/crypttab`, written
later. So the value is the same for all options:

```
root # zfs set org.zfsbootmenu:commandline="quiet" "${POOL}/ROOT"
```

ZFSBootMenu adds `root=zfs:zroot/ROOT/debian` itself: that is its root prefix for systems whose `os-release` says
`debian`, and OpenZFS's initramfs-tools script accepts it. Never put `root=`, `spl_hostid=` or any `zbm.*` option
into this property; the `zbm.*` options belong on ZFSBootMenu's own command line
([Configuring generate-zbm](#configuring-generate-zbm)).

**Option B.** Name the file system in which ZFSBootMenu looks for the key file:

```
root # zfs set org.zfsbootmenu:keysource="${POOL}/ROOT/${BE}" "${POOL}"
```

This is a convenience. When you choose a boot environment **in the menu**, ZFSBootMenu mounts the named file system
after you typed the passphrase, finds `etc/zfs/zroot.key` in it and keeps a copy in memory until it boots. Menu
operations that export and import the pool again (setting the default, cloning a snapshot, switching to read/write)
then do not ask again. It does not remove the first prompt.

**Check:**

```
root # zfs get -H -o value org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"
root # zfs get -H -o value org.zfsbootmenu:keysource "${POOL}"
```

The command line is `quiet`. The key source is `zroot/ROOT/debian` for Option B and `-` for Options A and C.

### Mounting the pool below /mnt

Exporting and importing the pool again proves that it can be opened again, and mounts the datasets in the right
order: the root first.

```
root # zpool export "${POOL}"
root # zpool import -N -R /mnt "${POOL}"
```

**Option B.** Load the key by typing the passphrase:

```
root # zfs load-key -L prompt "${POOL}"
```

If it refuses the passphrase, the file holds something other than what you believe (`cat /etc/zfs/zroot.key`
shows it). Nothing is installed yet: run `zfs load-key "${POOL}"` (which reads the file),
`zpool destroy "${POOL}"`, write the file again and repeat from [Creating the ZFS pool](#creating-the-zfs-pool).

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

- `-N` imports without mounting; the root dataset is mounted by hand because it is `canmount=noauto`.
- The cache file records the pool's configuration. It is copied into the new system, where Debian's
  `zfs-import-cache` service and the initramfs use it.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot.
> The ZFS tools inside the new system may not match the ZFS kernel module that the live system has loaded.

**Check:**

```
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`/mnt` is `zroot/ROOT/debian` and `/mnt/home` is `zroot/home`. Option B: the key status is `available`
(Options A and C print `-`).

> **Tip**
> If the live system is restarted from here on, everything it held in memory is gone: the ZFS packages, the host
> ID, the variables, the opened container, Option B's `/etc/zfs/zroot.key`, the pool's import and the mounts. Restore
> them in this order before you continue where you stopped:
>
> 1. Install ZFS and the tools again as in [Installation media](#installation-media), then
>    `zgenhostid -f 0x00bab10c`.
> 2. Set the variables of [Setting variables](#setting-variables). Option C:
>    `export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"`,
>    `cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"` and
>    `export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"`. Options A and B: `export POOL_DEV="${POOL_PART}"`.
> 3. `zpool import -N -R /mnt zroot`. Option B: `zfs load-key -L prompt zroot`, then write `/etc/zfs/zroot.key`
>    again with the commands of [Creating the ZFS pool](#creating-the-zfs-pool), with the **same** passphrase (or,
>    once it was copied, `cp -p /mnt/etc/zfs/zroot.key /etc/zfs/zroot.key` after the next step).
>    `zfs load-key -n zroot` then reads the file and must not report an error.
> 4. `zfs mount zroot/ROOT/debian`, `zfs mount zroot/home` and `zpool set cachefile=/etc/zfs/zpool.cache zroot`.
> 5. Once `debootstrap` has run: `mount "${ESP_PART}" /mnt/boot/efi`, and the mounts and `chroot` of
>    [Chrooting](#chrooting).

## Installing the base system

### What differs on Debian Testing

Testing is the distribution that becomes the next Debian release. Packages reach it from `unstable` after a few days
without serious bugs. For this guide that means:

- **The kernel can get ahead of ZFS.** Debian builds ZFS with DKMS (`zfs-dkms`, in `contrib`) for each installed
  kernel. OpenZFS refuses to build for a kernel series newer than the one its release names as the maximum: the
  build stops with `Cannot build against kernel version ...`. A kernel of such a series can reach testing before
  a `zfs-linux` that supports it. In November 2026, OpenZFS 2.4.4 (testing's `zfs-dkms`) named Linux 7.2 as its
  maximum, and unstable already had kernels of the 7.3 series.
  [Configuring APT for testing](#configuring-apt-for-testing) checks this before anything is installed;
  [Updating the system](#updating-the-system) shows how to tell and how to hold the kernel later.
- **Packages can leave testing for a while.** A package with release-critical bugs, or one that depends on such a
  package, can be removed from testing until the bugs are fixed. In November 2026, Debian's package tracker (read
  through a web search; the tracker itself could not be reached from here) showed `zfs-linux` 2.4.4-1 marked for
  removal from testing on 9 December 2026, because of bug #1151447. `apt-cache policy zfs-dkms` shows whether
  testing has it on your day; <https://tracker.debian.org/pkg/zfs-linux> shows the bug's state. A removal leaves the
  installed ZFS packages in place, but they get no updates while new kernels keep arriving: hold the kernel then
  ([Holding the kernel](#holding-the-kernel)). If `zfs-dkms` is missing from testing when you install, see
  [The live system cannot install ZFS](#the-live-system-cannot-install-zfs) and
  [APT pinning](#apt-pinning-zfs-from-unstable).
- **The initramfs generator may change.** Debian's kernel team decided in September 2025 (Debian bug #1114857) to
  make **dracut** the default initramfs generator during the forky cycle. Whether that change has reached testing
  could not be confirmed when this guide was written. This guide installs **initramfs-tools** explicitly before the
  kernel, so the system uses it whatever the default is; ZFSBootMenu's own image is built with dracut in any case,
  from the package `dracut-core`, which does not replace initramfs-tools.
- **Rust is new enough.** This repository needs `rustc` 1.88 or later. Debian's package trackers listed `rustc`
  1.97.1 in testing in October 2026 and 1.98.1 in unstable in November 2026, so the distribution's package is used;
  [Dependencies](#dependencies) checks the version.
- **Security updates** for testing are not handled the way they are for stable; see Debian's page on testing
  (<https://www.debian.org/devel/testing>).

### Running debootstrap

`debootstrap` installs a minimal Debian into `/mnt`. The suite `testing` selects whatever testing is today:

```
root # debootstrap testing /mnt http://deb.debian.org/debian
```

If `debootstrap` stops with *No such script*, the live system's `debootstrap` does not know the suite; name the
script of `sid`, which testing shares:

```
root # debootstrap testing /mnt http://deb.debian.org/debian /usr/share/debootstrap/scripts/sid
```

**Check:**

```
root # grep -E '^(PRETTY_NAME|VERSION_CODENAME)=' /mnt/etc/os-release
root # ls /mnt/boot
```

`os-release` names testing's code name, which was `forky` when this guide was written (for example
`PRETTY_NAME="Debian GNU/Linux forky/sid"` and `VERSION_CODENAME=forky`). `/mnt/boot` exists and is empty: no
kernel is installed yet.

### Copying the host ID and the key file

Mount the ESP where the new system expects it, and copy the host ID and the pool's cache file into the new system:

```
root # mkdir -p /mnt/boot/efi
root # mount "${ESP_PART}" /mnt/boot/efi
root # cp /etc/hostid /mnt/etc/hostid
root # mkdir -p /mnt/etc/zfs
root # cp /etc/zfs/zpool.cache /mnt/etc/zfs/zpool.cache
```

**Option B.** Copy the passphrase file, keeping its mode:

```
root # cp -p /etc/zfs/zroot.key /mnt/etc/zfs/zroot.key
```

**All options.** Save the variables for the chroot. For Options A and B the `LUKS_UUID` line stays empty; that is
harmless:

```
root # cat > /mnt/root/install.env <<EOF
export DISK="${DISK}"
export ESP_PART="${ESP_PART}"
export POOL_PART="${POOL_PART}"
export POOL="${POOL}"
export BE="${BE}"
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
`zroot.key` with mode `----------`. `install.env` shows the values you set; for Option C `LUKS_UUID` is not empty.

### Chrooting

Give the new system the live system's name resolution and the kernel's file systems, and enter it:

```
root # cp --dereference /etc/resolv.conf /mnt/etc/resolv.conf
root # mount --types proc /proc /mnt/proc
root # mount --rbind /sys /mnt/sys && mount --make-rslave /mnt/sys
root # mount --rbind /dev /mnt/dev && mount --make-rslave /mnt/dev
root # mount --bind /run /mnt/run && mount --make-slave /mnt/run
root # chroot /mnt /bin/bash --login
```

```
(chroot) root # source /root/install.env
(chroot) root # export PS1="(chroot) ${PS1}"
```

**Check:**

```
(chroot) root # echo "${POOL} ${BE} ${LUKS_UUID}"
(chroot) root # ls /sys/firmware/efi/efivars | head -n 3
(chroot) root # getent hosts deb.debian.org
```

The variables print (`zroot debian`, and the LUKS UUID for Option C), the EFI variables are visible, and
`deb.debian.org` resolves to an address.

### Configuring APT for testing

Write the package sources. `contrib` holds ZFS, `non-free-firmware` holds CPU microcode and device firmware:

**FILE** `/etc/apt/sources.list`

```
deb http://deb.debian.org/debian testing main contrib non-free-firmware
deb http://deb.debian.org/debian-security testing-security main contrib non-free-firmware
```

`testing` follows testing for good: when forky is released as Debian 14, the system moves on to the next testing.
To stay with forky as it becomes stable instead, write `forky` and `forky-security` in place of `testing` and
`testing-security`.

```
(chroot) root # apt update
(chroot) root # apt full-upgrade
```

**Check:**

```
(chroot) root # apt-cache policy linux-image-amd64 zfs-dkms rustc initramfs-tools | grep -E '^[a-z]|Candidate'
```

Each of the four packages has a `Candidate` version, not `(none)`. If `zfs-dkms` has none, it is not in testing
today; see [The live system cannot install ZFS](#the-live-system-cannot-install-zfs). If `apt update` reports that
the security suite does not exist, remove the second line of the file and run `apt update` again.

Before anything is installed, compare the kernel with the newest kernel series that testing's ZFS builds for. The
candidate of `linux-image-amd64` carries the kernel's version (for example `7.2.15-1` is Linux 7.2). The ZFS limit is
the `Linux-Maximum` line of the `META` file in the `zfs-dkms` package:

```
(chroot) root # cd /tmp && apt-get download zfs-dkms && cd /
(chroot) root # dpkg-deb --fsys-tarfile /tmp/zfs-dkms_*.deb | tar -xO --wildcards '*/META' \
  | grep -E '^(Version|Linux-Max)'
(chroot) root # rm /tmp/zfs-dkms_*.deb
```

- `apt-get download` may note that the download is performed unsandboxed as root; that is harmless.
- In November 2026, OpenZFS 2.4.4 printed `Linux-Maximum: 7.2`. Debian has raised the limit of a release by a
  patch before, so trust the line in the package, not this number.
- That Debian's package carries `META` was not confirmed here. If `tar` reports that nothing matched, skip the
  comparison: the Check of [Kernel and ZFS](#kernel-and-zfs) shows the same answer after the installation.
- If the kernel's series (`7.3`, say) is newer than `Linux-Maximum`, stop here: nothing is installed yet. Take ZFS
  from unstable ([APT pinning](#apt-pinning-zfs-from-unstable)) if unstable has a newer `zfs-linux` that supports
  the kernel, or wait a few days.

## Kernel and ZFS

The kernel and the ZFS module are installed first, so that the following sections can configure the initramfs that
belongs to them. Install **initramfs-tools** before the kernel, so that the kernel package does not choose another
initramfs generator (see [What differs on Debian Testing](#what-differs-on-debian-testing)):

```
(chroot) root # apt install initramfs-tools
(chroot) root # apt install linux-image-amd64 linux-headers-amd64
(chroot) root # apt install zfs-dkms zfsutils-linux zfs-initramfs zfs-zed
```

- `linux-image-amd64` and `linux-headers-amd64` always point to testing's current kernel; DKMS needs the headers to
  build the ZFS module.
- `zfs-dkms` builds the ZFS module for the installed kernel; confirm its licence note with **OK**. `zfs-initramfs`
  adds OpenZFS's scripts to Debian's initramfs. `zfs-zed` is the ZFS event daemon, enabled later.
- Messages about the locale (`perl: warning: Setting locale failed`) are harmless; the locale is set in
  [Configuring the system](#configuring-the-system).

Install the CPU microcode for your processor. Debian's microcode packages put it into the same initramfs file,
which matters because ZFSBootMenu loads only one initramfs file per kernel. For an Intel processor:

```
(chroot) root # apt install intel-microcode
```

For an AMD processor:

```
(chroot) root # apt install amd64-microcode
```

**Option C.** Install cryptsetup with its initramfs integration:

```
(chroot) root # apt install cryptsetup cryptsetup-initramfs
```

**All options.** Note the version of the installed kernel:

```
(chroot) root # export KVER="$(ls /lib/modules | sort -V | tail -n 1)"
(chroot) root # echo "KVER=${KVER}" >> /root/install.env
```

> **Note**
> Do not install the packages `dracut` or `systemd-boot`. `dracut` replaces initramfs-tools as the system's
> generator, and `systemd-boot` installs kernels onto the ESP. ZFSBootMenu needs only `dracut-core` and
> `systemd-boot-efi`, installed later.

**Check:**

```
(chroot) root # dpkg -l initramfs-tools dracut 2>/dev/null | grep '^ii'
(chroot) root # dkms status
(chroot) root # modinfo -k "${KVER}" -F version zfs
(chroot) root # ls /boot
(chroot) root # ls -A /boot/efi
```

- Only `initramfs-tools` is listed as installed (`ii`), not `dracut`.
- `dkms status` lists `zfs/<version>, <KVER>, amd64: installed`.
- `modinfo` prints the ZFS version (OpenZFS 2.4.4 was in testing when this guide was written).
- `/boot` contains `vmlinuz-<KVER>`, `initrd.img-<KVER>`, `config-<KVER>` and `System.map-<KVER>`. ZFSBootMenu finds
  this pair of kernel and initramfs by these names.
- `/boot/efi` is empty: no kernel or initramfs was put on the unencrypted ESP.

If `dkms status` does not list the module as installed, build it by hand and read the messages:

```
(chroot) root # dkms autoinstall -k "${KVER}"
```

If the build stops with *Cannot build against kernel version*, testing's kernel is newer than its ZFS supports; see
[A new kernel has no ZFS module](#a-new-kernel-has-no-zfs-module). This first initramfs cannot unlock anything yet;
[Configuring the initramfs](#configuring-the-initramfs) fixes that.

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Debian's initramfs must unlock the container again after `kexec`. A random key in a second key slot lets it do so
without asking. `luksAddKey` asks for the passphrase:

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

Debian's initramfs is built by initramfs-tools. OpenZFS's hook (from `zfs-initramfs`) puts the ZFS module and tools,
`/etc/hostid` and `/etc/zfs/zpool.cache` into it; its script imports the pool and mounts `root=zfs:...`.

| | Option A | Option B | Option C |
|---|---|---|---|
| ZFS module and tools, `/etc/hostid` | yes | yes | yes |
| `/etc/zfs/zroot.key` | | yes | |
| `cryptsetup`, `/etc/crypttab` and the key file | | | yes |
| readable by root only | not needed | **required** | **required** |

**Option A.** Nothing to configure; continue with the rebuild below.

**Option B.** Make the initramfs readable by root only:

**FILE** `/etc/initramfs-tools/conf.d/umask.conf`

```
UMASK=0077
```

Make sure the passphrase file goes into the initramfs. Older guides say that it is copied automatically because it
lies in `/etc/zfs`; OpenZFS's own hook copies no key file, and whether Debian's package changes that could not be
confirmed. This hook copies it in any case:

**FILE** `/etc/initramfs-tools/hooks/zfs-key`

```
#!/bin/sh
# Copy the ZFS passphrase file into the initramfs. The initramfs lives in /boot inside the encrypted pool.
PREREQ=""
prereqs() { echo "${PREREQ}"; }
case "$1" in prereqs) prereqs; exit 0 ;; esac
. /usr/share/initramfs-tools/hook-functions
copy_file key /etc/zfs/zroot.key || true
exit 0
```

```
(chroot) root # chmod 0755 /etc/initramfs-tools/hooks/zfs-key
```

When Debian's initramfs finds that the root's encryption root has a `keylocation` other than `prompt`, it runs
`zfs load-key` without asking, which reads the copy of `/etc/zfs/zroot.key` in the initramfs.

**Option C.** Make the initramfs readable by root only:

**FILE** `/etc/initramfs-tools/conf.d/umask.conf`

```
UMASK=0077
```

Add the container to `/etc/crypttab`. The option `initramfs` makes cryptsetup's initramfs hook include it, which it
would not do by itself for a root file system on ZFS; `discard` passes TRIM through (the header holds that flag
too):

```
(chroot) root # echo "luks-${LUKS_UUID} UUID=${LUKS_UUID} /etc/cryptsetup-keys.d/zroot.key luks,discard,initramfs" \
  >> /etc/crypttab
```

Tell cryptsetup's hook that key files in `/etc/cryptsetup-keys.d` may be copied into the initramfs:

```
(chroot) root # echo 'KEYFILE_PATTERN="/etc/cryptsetup-keys.d/*.key"' >> /etc/cryptsetup-initramfs/conf-hook
```

The mapping name `luks-<UUID>` is the one ZFSBootMenu uses, so the pool finds its device under the same path in
both.

**All options.** Rebuild the initramfs of every installed kernel:

```
(chroot) root # update-initramfs -u -k all
```

> **Warning**
> A key file in an initramfs is safe only while the initramfs stays in `/boot` inside the encrypted pool. Never copy
> Debian's initramfs to the ESP or to any other unencrypted place.

**Check:**

```
(chroot) root # ls -l /boot/initrd.img-*
(chroot) root # lsinitramfs "/boot/initrd.img-${KVER}" | grep -E 'zfs\.ko|etc/hostid|zpool\.cache|nvme'
(chroot) root # lsinitramfs "/boot/initrd.img-${KVER}" | grep -E 'zroot\.key|cryptroot|sbin/cryptsetup$'
(chroot) root # grep -v '^#' /etc/crypttab
(chroot) root # ls -A /boot/efi
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `ls -l` | any mode | `-rw-------` | `-rw-------` |
| first `grep` | `zfs.ko`, `etc/hostid`, `etc/zfs/zpool.cache` | the same | the same |
| second `grep` | nothing | `etc/zfs/zroot.key` | `usr/sbin/cryptsetup`, `cryptroot/crypttab`, a key in `cryptroot/` |
| `/etc/crypttab` | no lines, or none for the pool | the same | one line for `luks-<UUID>` (see below) |
| `/boot/efi` | empty | empty | empty |

- Option C's line in `/etc/crypttab` is
  `luks-<UUID> UUID=<UUID> /etc/cryptsetup-keys.d/zroot.key luks,discard,initramfs`.
- The first `grep` also lists `nvme.ko` if Debian's kernel builds the NVMe driver as a module; this could not be
  confirmed here, and nothing is missing if the driver is built in.
- Option C: the exact place where cryptsetup's hook stores the key inside the initramfs could not be confirmed from
  Debian's documentation here; `lsinitramfs ... | grep cryptroot` shows it. `update-initramfs` may warn that it
  cannot determine the root device for cryptsetup; with the `initramfs` option in `/etc/crypttab` that is
  expected. A warning that `KEYFILE_PATTERN` or `UMASK` is not set means one of the two files above is missing.
- If the mode is `-rw-r--r--` in Option B or C, `umask.conf` is missing or misspelled: fix it and run
  `update-initramfs -u -k all` again.

## Configuring the system

Set the host name, locale, time zone and keyboard. Replace `YOURHOSTNAME`:

```
(chroot) root # echo YOURHOSTNAME > /etc/hostname
(chroot) root # printf '127.0.1.1\tYOURHOSTNAME\n' >> /etc/hosts
(chroot) root # apt install locales keyboard-configuration console-setup
(chroot) root # dpkg-reconfigure locales tzdata keyboard-configuration console-setup
```

Enable `en_US.UTF-8` among the locales, because some programs need it, and your own.

Configure wired networking with systemd-networkd. Name resolution follows in
[Finalizing the installation](#finalizing-the-installation), because installing it now would break name resolution
inside the chroot:

**FILE** `/etc/systemd/network/20-wired.network`

```
[Match]
Name=en* eth*

[Network]
DHCP=yes
```

```
(chroot) root # systemctl enable systemd-networkd.service
```

For wireless networks, install `network-manager` instead and do not create the file above.

Enable the ZFS services, which import the pool and mount its datasets on the running system, and the ZFS event
daemon:

```
(chroot) root # systemctl enable zfs.target zfs-import-cache.service zfs-mount.service zfs-import.target
(chroot) root # systemctl enable zfs-zed.service
```

Only the ESP goes into `/etc/fstab`; ZFS mounts its datasets itself. `generate-zbm` uses this entry to mount the
ESP when it is not mounted:

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

Set the root password and create a user (replace `USERNAME`):

```
(chroot) root # passwd
(chroot) root # adduser USERNAME
```

**Check:**

```
(chroot) root # cat /etc/hostname /etc/fstab
(chroot) root # systemctl is-enabled zfs-import-cache.service zfs-mount.service zfs.target systemd-networkd.service
(chroot) root # locale -a | grep -i en_us
```

The host name is yours, `/etc/fstab` has one `vfat` line for `/boot/efi`, all units are `enabled`, and
`en_US.utf8` is listed.

## Installing ZFSBootMenu

### Dependencies

ZFSBootMenu is built from source, inside the chroot, with the installed kernel and ZFS. Install what the build and
the image need:

```
(chroot) root # apt install git make cargo rustc gcc libc6-dev binutils dracut-core systemd-boot-efi kexec-tools \
  fzf mbuffer bsdextrautils ncurses-bin procps kmod udev bash util-linux less efibootmgr dosfstools
```

When `kexec-tools` asks whether it should handle reboots, answer **No**.

| Package | Provides | Needed for |
|---|---|---|
| `git`, `make` | `git`, `make` | fetching and building the source |
| `cargo`, `rustc` | the Rust compiler, 1.88 or later | building `zbm`, `brush`, `generate-zbm` and `zbm-kcl` |
| `gcc`, `libc6-dev` | `cc` and the C library files | the linker that `rustc` calls |
| `binutils` | `strip`, `strings`, `objcopy` | `make install`; reading kernel versions; the image Check |
| `dracut-core` | `dracut`, `lsinitrd` | building ZFSBootMenu's image; it does not replace initramfs-tools |
| `systemd-boot-efi` | `/usr/lib/systemd/boot/efi/linuxx64.efi.stub` | the EFI stub of the image |
| `kexec-tools` | `kexec` | starting Debian's kernel from ZFSBootMenu |
| `fzf` | the menu of the default (shell) runtime | the image |
| `mbuffer`, `bsdextrautils` (`column`), `less` | progress, tables, the log viewer | optional (the build warns) |
| `ncurses-bin`, `procps`, `kmod`, `udev`, `bash`, `util-linux` | `tput`, `ps`, kmod, udev | the image; preinstalled |
| `efibootmgr` | firmware boot entries | [Creating the firmware boot entries](#creating-the-firmware-boot-entries) |
| `dosfstools` | `mkfs.vfat`, `fsck.vfat` | the ESP |

**Option C.** `cryptsetup`, installed in [Kernel and ZFS](#kernel-and-zfs), also puts LUKS support into
ZFSBootMenu's image: the build adds `cryptsetup`, `dmsetup`, `udevadm` and the dm-crypt modules only when
`cryptsetup` is installed. Do not add dracut's `crypt` module to ZFSBootMenu's configuration; ZFSBootMenu unlocks
LUKS volumes itself.

**All options.** Optional, for the Rust runtime's keyboard layouts (see
[Choosing the runtime](#choosing-the-runtime)):

```
(chroot) root # apt install xkb-data libx11-data
```

**Check:**

```
(chroot) root # cargo --version && rustc --version
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # dpkg -l initramfs-tools dracut dracut-core | grep '^ii'
(chroot) root # command -v dracut lsinitrd kexec fzf strip
```

- `rustc` is 1.88 or later.
- The EFI stub exists.
- `initramfs-tools` and `dracut-core` are installed, `dracut` is not.
- Every program has a path. That `lsinitrd` comes with `dracut-core` could not be confirmed here; if it alone is
  missing, `apt install apt-file && apt-file update && apt-file search bin/lsinitrd` names its package.

If `rustc` is older than 1.88, install the Rust toolchain with `rustup` (<https://rustup.rs>) **as root inside the
chroot**; its installer is fetched with `curl` (`apt install curl ca-certificates` first), and it uses the `gcc`
and `libc6-dev` installed above as its linker. Run `export PATH="/root/.cargo/bin:${PATH}"` in every shell that
runs `make` below. This repository's `rust-toolchain.toml` then makes `rustup` download Rust 1.97.0 once.

### Building and installing

Why: `make` builds the programs `zbm`, `brush`, `generate-zbm` and `zbm-kcl`; `make install` installs them with the
dracut module and the configuration.

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

- The crates are part of the repository (`vendor/`), so the build itself needs no network.
- `make` runs `cargo build --release --locked` for `zbm`, `brush`, `generate-zbm` and `zbm-kcl`. `make install`
  writes the version into the checkout and builds them again, as root.
- `make install` puts `generate-zbm` and `zbm-kcl` in `/usr/bin`; `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
  the shell runtime in `/usr/share/zfsbootmenu`; the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in
  `/usr/lib/dracut/modules.d`; the mkinitcpio hooks in `/usr/lib/initcpio`; `config.yaml`, `mkinitcpio.conf` and
  `dracut.conf.d/` in `/etc/zfsbootmenu`; the manual pages; and examples and hooks in
  `/usr/share/examples/zfsbootmenu`.
- `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files
  it installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files with other names there are kept. That is why the next
  step writes `config.yaml` after `make install`, and the dracut settings below go into files of their own.

**Check:**

```
(chroot) root # git -C /usr/src/zbm branch --show-current
(chroot) root # generate-zbm --showver
(chroot) root # ls -l /usr/bin/generate-zbm /usr/bin/zbm-kcl /usr/libexec/zfsbootmenu/
(chroot) root # ls /usr/lib/dracut/modules.d/ | grep zfsbootmenu
```

The branch is `claude/zbm-refactor`, `generate-zbm` prints a version, `zbm` and `brush` are in
`/usr/libexec/zfsbootmenu`, and `90zfsbootmenu` and `90zfsbootmenu-measured` are listed.

Optional: Cargo's build directory is no longer needed and takes most of the space; delete it before the snapshot of
[Finalizing the installation](#finalizing-the-installation), so that the snapshot does not keep it. The next update
of ZFSBootMenu then builds from scratch.

```
(chroot) root # rm -rf /usr/src/zbm/target
```

### Configuring generate-zbm

`generate-zbm` builds ZFSBootMenu's image as configured in `/etc/zfsbootmenu/config.yaml`.

**Options A and B.**

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
  CommandLine: "quiet loglevel=0 zbm.luks=off"
EOF
```

**Option C.** The same file, with the LUKS UUID in the last line. The shell fills in `${LUKS_UUID}`, so check first
that it is set:

```
(chroot) root # echo "${LUKS_UUID}"
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
  CommandLine: "quiet loglevel=0 zbm.luks=UUID=${LUKS_UUID}"
EOF
```

**All options.**

| Key | Value | Meaning |
|---|---|---|
| `Global.ManageImages` | `true` | `generate-zbm` does nothing while this is `false`, the installed default |
| `Global.BootMountPoint` | `/boot/efi` | the ESP; `generate-zbm` mounts it if needed (from `/etc/fstab`) |
| `Global.DracutConfDir` | `/etc/zfsbootmenu/dracut.conf.d` | dracut settings of **ZFSBootMenu's** image only |
| `Global.PreHooksDir`, `Global.PostHooksDir` | as shown | programs run before and after each build |
| `Global.InitCPIOConfig` | as shown | used only with mkinitcpio; kept from the installed file |
| `Components.Enabled` | `false` | no separate kernel and initramfs; only the EFI bundle |
| `EFI.Enabled` | `true` | build the EFI bundle: kernel, initramfs and command line in one file |
| `EFI.ImageDir` | `/boot/efi/EFI/zbm` | where the bundle is written |
| `EFI.Versions` | `false` | one image and one `-backup` copy with fixed names, so the firmware entries never change |
| `Kernel.Prefix` | `vmlinuz` | fixes the image names to `vmlinuz.EFI` and `vmlinuz-backup.EFI` |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's** kernel, built into the bundle |

| Option | `Kernel.CommandLine` | Why |
|---|---|---|
| A, B | `quiet loglevel=0 zbm.luks=off` | do not unlock any LUKS volume |
| C | `quiet loglevel=0 zbm.luks=UUID=<LUKS UUID>` | unlock only this container, as `luks-<UUID>` |

- `zbm.luks=off` keeps ZFSBootMenu from asking for a LUKS volume, for example on a USB disk that is plugged in at
  boot. Without `cryptsetup` the image has no LUKS code anyway.
- The UUID of Option C is that of the **LUKS container** (`cryptsetup luksUUID`), not the pool's GUID and not a
  file system UUID.

Debian's kernels are named `vmlinuz-<version>`, so the image would be called `vmlinuz.EFI` without `Prefix` too;
the line keeps the names fixed whatever the kernel files are called. By default `generate-zbm` uses the kernel with
the highest version in `/boot`, and the ZFS module for that kernel must be installed.

Other options for `Kernel.CommandLine`: add `zbm.timeout=<seconds>` to change the countdown (default 10; `-1`
always shows the menu), and replace `loglevel=0` with `loglevel=7` to see more messages when something goes wrong.

ZFSBootMenu's image must not depend on the computer it was built on: this first image is built in a chroot on the
live system, whose kernel and loaded drivers are not those of the installed system. dracut's host-only mode would
pick drivers by looking at the live system. Turn it off:

**FILE** `/etc/zfsbootmenu/dracut.conf.d/generic.conf`

```
# Build an image that does not depend on the machine it was built on: the first image is built
# inside a chroot on a live system, whose kernel and loaded drivers are not those of the installed system.
hostonly="no"
```

The image is then larger and contains the NVMe driver, the keyboard drivers and, in Option C, dm-crypt. The file's
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

ZFSBootMenu's image runs one of two runtimes. The **shell runtime**, written in Bash with the `fzf` menu, is the
default and is enough for all three options. The **Rust runtime** (`zbm`, with a new menu) is opt-in; it adds,
among others, the locked menu, measured boot and keyboard layouts other than US at the passphrase prompt. Both
unlock LUKS volumes with a passphrase. To choose the Rust runtime, optionally:

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

The passphrase prompt of the shell runtime uses the US keyboard layout: choose characters that are in the same
place on your layout, or use the Rust runtime. With the Rust runtime, either add `zbm.console=drm` to
`Kernel.CommandLine` (the console on the display then types with the image's first layout; the image needs the
`evdev` module and a DRM driver, `simpledrm` on the firmware's framebuffer being enough, for example with
`add_drivers+=" evdev "` in a file of its own in `/etc/zfsbootmenu/dracut.conf.d/`), or press **Ctrl-K** at the
prompt to choose a layout. The image's layouts are those of `zfsbootmenu_keyboard_layouts`; unset, the layout
of `XKBLAYOUT` in `/etc/vconsole.conf` if that file exists, then `us`, `de`, `fr` and `gb`.

**Check:** `cat /etc/zfsbootmenu/dracut.conf.d/*.conf` shows `zfsbootmenu_runtime=rust` only if you chose it.

### Building and checking the image

Look at what `generate-zbm` would do, then build the image twice:

```
(chroot) root # generate-zbm --dry-run --debug
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

- `--dry-run` prints one `Would ...` line for each change and makes none; `--debug` prints every command. The dry
  run names the kernel it would use: `/boot/vmlinuz-<KVER>`.
- The first run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`. The second renames it to `vmlinuz-backup.EFI` and writes a
  new one, so that the backup entry has something to boot from the start.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | `make install` was not run |
| `failed to install essential executable '<name>'` | a program is missing: often `kexec`, or `fzf` (shell runtime) |
| `Unable to find UEFI stub loader at default locations` | `systemd-boot-efi` is not installed |
| `ManageImages not enabled, no action taken` | `config.yaml` was overwritten by `make install`; write it again |
| `Unable to locate libgcc_s.so` | install `libgcc-s1` |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (see below) |

The host ID message: the SPL module's `spl_hostid` parameter differs from `/etc/hostid` (the check is skipped while the
parameter is 0). The image takes `/etc/hostid`, which must be `00bab10c` (`od -A n -t x4 /etc/hostid`).

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
| first `grep` lists | `etc/hostid`, `zfs.ko`, `nvme` if a module | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

The second `grep` must print nothing: no key file and no crypttab are in ZFSBootMenu's image. If it prints a line,
do not boot this image; find out why first. For Option B it proves that `/etc/zfs/zroot.key` did not get in. If
`bin/cryptsetup` is listed for Option A or B, that is fine: it is in the image whenever the package is installed.

> **Warning**
> `generate-zbm` runs dracut with `/etc/zfsbootmenu/dracut.conf.d` instead of `/etc/dracut.conf.d`, but dracut still
> reads the main file `/etc/dracut.conf` and the packages' directory `/usr/lib/dracut/dracut.conf.d/`. A key file
> named in either would end up in ZFSBootMenu's image on the unencrypted ESP. Never name a key file there.

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

Install systemd's resolver for name resolution on the installed system. Its package replaces `/etc/resolv.conf`
with a link to the resolver, which breaks name resolution inside the chroot, so it comes last:

```
(chroot) root # apt install systemd-resolved
(chroot) root # systemctl enable systemd-resolved.service
(chroot) root # ls -l /etc/resolv.conf
```

`/etc/resolv.conf` is now a link to `../run/systemd/resolve/stub-resolv.conf`. If it is still a plain file, make the
link yourself: `ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf`. (Whether Debian's package makes the
link by itself inside a chroot could not be confirmed here.)

Leave the chroot, snapshot the fresh installation, unmount everything and export the pool:

```
(chroot) root # exit
root # zfs snapshot "${POOL}/ROOT/${BE}@install"
root # umount /mnt/boot/efi
root # umount -R -l /mnt/dev /mnt/sys /mnt/proc /mnt/run
root # zfs umount -a
root # zpool export "${POOL}"
```

**Option C.** Close the container:

```
root # cryptsetup close "luks-${LUKS_UUID}"
```

**All options.** The snapshot `@install` is the first state to roll back to. The export leaves the pool in the
state that every later import expects.

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

With the **default (shell) runtime** you see:

| Step | Option A | Option B | Option C |
|---|---|---|---|
| 1 | | | `Unlock LUKS volume <device>` and `Passphrase (leave empty to skip):`. Type the LUKS passphrase |
| 2 | `Booting zroot/ROOT/debian in 10 seconds` (1) | the same | the same |
| 3 | | the screen clears; `Enter passphrase for 'zroot':`. Type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Debian's kernel | the same | the same |
| 5 | Debian starts | Debian starts **without** asking again | Debian starts **without** asking again |

(1) With `[RETURN] boot now` and `[ESCAPE] boot menu`.

- **Enter** boots at once; **Escape** during the countdown opens the menu, which lists `zroot/ROOT/debian`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: the prompt allows three attempts. After three wrong passphrases ZFSBootMenu opens its emergency shell
  with the message `unable to load key for zroot/ROOT/debian`. Type `exit` there; the menu then asks again.
- With the **Rust runtime** the same questions appear as dialogs: `Unlock <volume>` for Option C, a ZFS key dialog
  for Option B (three attempts), and the banner
  `Booting <environment> in <N> s. Press any key to choose another.`

Log in as root. From here on, the prompt `root #` means root on the **installed** system. Check the boot:

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status zroot
root # hostid
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
root # systemctl --failed
```

**Option B.**

```
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/debian
```

**Option C.**

```
root # LUKS_UUID="$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)"
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `/proc/cmdline` | `quiet`, `root=zfs:zroot/ROOT/debian` and `spl.spl_hostid=0x00bab10c` | the same | the same |
| `/` | `zroot/ROOT/debian` | the same | the same |
| `zpool status` | `ONLINE`, on `nvme0n1p2` | the same | `ONLINE`, on `luks-<UUID>` |
| `hostid` | `00bab10c` | the same | the same |
| encryption | | `aes-256-gcm`, root `zroot`, `available` (1) | `type: LUKS2`, `sector size: 4096`, `flags: discards` |
| asked for the passphrase | never | once, by ZFSBootMenu | once, by ZFSBootMenu |
| `systemctl --failed` | `0 loaded units listed` | the same | the same |

(1) Option B: both datasets show these values. Only the encryption root has a key location: `zroot` shows
`file:///etc/zfs/zroot.key`, and `zroot/ROOT/debian` shows `none`.

`BootCurrent` names the entry the firmware used. If it is the number of the `ZFSBootMenu` entry, the firmware honours
entries and `\EFI\BOOT\BOOTX64.EFI` is only a reserve. If it is another number, the firmware started the copy; the
post-hook `50-bootx64.sh` of [Creating the firmware boot entries](#creating-the-firmware-boot-entries) keeps it
current, so check after each `generate-zbm` that it ran (see
[Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image)).

If the system asked for a passphrase a second time, see the entry about it in
[Troubleshooting][second-unlock].

Then:

- delete `/root/install.env` (`rm /root/install.env`); it holds no secret;
- set up the APT hook of [Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image);
- **Option C**: make the first LUKS header backup
  ([Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header)).

## Usage

### Updating the system

Testing gets new packages every day. Update with `full-upgrade`, which lets APT add and remove packages as testing
changes:

```
root # zfs snapshot zroot/ROOT/debian@pre-update-$(date +%Y%m%d)
root # apt update
root # apt full-upgrade
```

The snapshot lets you go back if the update breaks the system
([Boot environments, snapshots and rollback](#boot-environments-snapshots-and-rollback)). A new kernel gets its
initramfs from the kernel package's own hooks, after DKMS has built the ZFS module for it, with the key file of
Options B and C. Whether updates of `zfs-dkms`, `zfsutils-linux` and `cryptsetup` also rebuild the initramfs of the
installed kernels could not be confirmed here (OpenZFS's own DKMS configuration does not ask for it). After such an
update, rebuild them yourself, then run the checks below:

```
root # update-initramfs -u -k all
```

`apt autoremove` removes old kernels, but Debian keeps the running kernel and the newest ones. Keep at least one
kernel that is known to boot until a new one has booted.

#### Telling whether a new kernel has its ZFS module

Before an update, see whether it brings a new kernel:

```
root # apt list --upgradable 2>/dev/null | grep -E '^linux-(image|headers)-'
```

After the update, and **before rebooting**, check every installed kernel:

```
root # dkms status
root # for v in $(ls /lib/modules); do \
  printf '%s: ' "$v"; modinfo -k "$v" -F version zfs 2>/dev/null || echo MISSING; done
```

Every kernel must show a ZFS version. A kernel marked `MISSING` has no ZFS module: its initramfs cannot import the
pool, and ZFSBootMenu boots the **newest** kernel by default. Do not reboot into it; see
[A new kernel has no ZFS module](#a-new-kernel-has-no-zfs-module).

Then look inside every initramfs. An initramfs built before DKMS finished, or by a hook that failed, has no ZFS
module even when `modinfo` finds one:

```
root # for i in /boot/initrd.img-*; do echo "== $i"; \
  lsinitramfs "$i" | grep -E 'zfs\.ko|etc/zfs/zroot\.key|cryptroot/'; done
```

| | Option A | Option B | Option C |
|---|---|---|---|
| below each `==` line | a `zfs.ko` line | the same and `etc/zfs/zroot.key` | the same and lines below `cryptroot/` |

If an initramfs lacks a line, rebuild it with `update-initramfs -u -k VERSION` (the version from its file name) and
look again.

The highest kernel series that the installed OpenZFS supports is in the `Linux-Maximum` line of its `META` file;
Debian sometimes raises it with a patch:

```
root # grep -H Linux-Maximum /usr/src/zfs-*/META
```

(That Debian's `zfs-dkms` keeps `META` in `/usr/src/zfs-<version>/` could not be confirmed here; if the file is
missing, `dkms status` and the loop above are what count.)

#### Holding the kernel

To stop APT from installing new kernels until ZFS supports them, hold the two packages that always point to the
current kernel:

```
root # apt-mark hold linux-image-amd64 linux-headers-amd64
root # apt-mark showhold
```

While they are held, the system keeps its current kernel and gets **no kernel updates, security fixes included**.
Release the hold once `zfs-dkms` in testing supports the new kernel:

```
root # apt-mark unhold linux-image-amd64 linux-headers-amd64
```

**Check:** `apt-mark showhold` lists exactly the packages you held.

#### APT pinning: ZFS from unstable

When testing's `zfs-linux` lags behind, or has been removed from testing for a while, a newer one is often in
`unstable`. You can take only the ZFS packages from there. Add `unstable` with a low priority, so that nothing else
comes from it:

**FILE** `/etc/apt/sources.list.d/unstable.list`

```
deb http://deb.debian.org/debian unstable main contrib
```

**FILE** `/etc/apt/preferences.d/90-unstable`

```
# Install from unstable only what is asked for with -t unstable
Package: *
Pin: release a=unstable
Pin-Priority: 100
```

```
root # apt update
root # apt install -t unstable zfs-dkms zfsutils-linux zfs-initramfs
```

With priority 100, APT installs nothing from `unstable` by itself, and the packages installed from it stay at their
version until testing has a newer one or you repeat the command. `-t unstable` may bring in newer libraries from
unstable that these packages need; read APT's list before you confirm. This is a common Debian technique, but it was
not run for this guide. Run `generate-zbm` afterwards.

### Regenerating the ZFSBootMenu image

ZFSBootMenu's image contains the kernel, the ZFS module and tools, cryptsetup and the other unlock tools of **this
system at the time of the build**. Rebuild it after updates of the ZFS packages, of the kernel the image uses, of
cryptsetup, of dracut, and after changing anything in `/etc/zfsbootmenu/`:

```
root # generate-zbm
```

- Each run keeps the previous image as `vmlinuz-backup.EFI`, which the `ZFSBootMenu (Backup)` entry boots. The
  backup is replaced by the previous image at every build, so after two bad builds in a row it is bad too. A copy
  under another name with its own entry survives that:

  ```
  root # cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/zbm/known-good.EFI
  root # efibootmgr -c -d /dev/nvme0n1 -p 1 -L "ZFSBootMenu (known good)" -l '\EFI\ZBM\KNOWN-GOOD.EFI'
  ```

  The last command puts the new entry first in the boot order; put `ZFSBootMenu` first again with
  `efibootmgr -o XXXX,YYYY,...` (the numbers that `efibootmgr` lists).
- `generate-zbm` uses the newest kernel in `/boot` and needs the ZFS module for it. If that kernel has none, the
  build fails or the image cannot import the pool; `generate-zbm --kver VERSION` builds with an older kernel.
- The post-hook `50-bootx64.sh` refreshes the firmware's fallback copy `\EFI\BOOT\BOOTX64.EFI` after every build.
  `cmp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI` prints nothing when the two are the same;
  `generate-zbm --debug` also prints `## Processing hook: ...50-bootx64.sh`. The `known-good.EFI` copy above is never
  refreshed; that is its purpose.
- Rebuild **before** raising the pool's compatibility
  ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).

**An APT hook.** Debian has no hook for this, so this guide proposes one: after every APT run that changed the
newest kernel with a ZFS module, or one of the packages the image is built from, it runs `generate-zbm` with that
kernel. It never makes APT fail. It was written for this guide and has not been run by the project.

**FILE** `/usr/local/sbin/zbm-after-apt`

```
#!/bin/sh
# Rebuild ZFSBootMenu's image after APT changed the kernel, ZFS, cryptsetup or dracut.
# Called by /etc/apt/apt.conf.d/95-zfsbootmenu. It never makes APT fail.
stamp=/var/lib/zfsbootmenu/apt-stamp
newest=""
kver=""
for k in $(ls -1 /boot/vmlinuz-* 2>/dev/null | sort -V -r); do
  v="${k#/boot/vmlinuz-}"
  [ -n "${newest}" ] || newest="${v}"
  if modinfo -k "${v}" zfs >/dev/null 2>&1; then
    kver="${v}"
    break
  fi
done
if [ -z "${kver}" ]; then
  echo "zbm-after-apt: no kernel in /boot has a ZFS module; ZFSBootMenu not rebuilt" >&2
  exit 0
fi
if [ "${kver}" != "${newest}" ]; then
  echo "zbm-after-apt: WARNING: the newest kernel ${newest} has no ZFS module; do not boot it" >&2
fi
pkgs="zfsutils-linux zfs-dkms cryptsetup-bin dracut-core systemd-boot-efi kexec-tools"
state="${kver} $(dpkg-query -W -f='${Package}=${Version} ' ${pkgs} 2>/dev/null)"
if [ -r "${stamp}" ] && [ "$(cat "${stamp}")" = "${state}" ]; then
  exit 0
fi
echo "zbm-after-apt: rebuilding ZFSBootMenu with kernel ${kver}"
if generate-zbm --kver "${kver}"; then
  mkdir -p "$(dirname "${stamp}")"
  printf '%s\n' "${state}" > "${stamp}"
else
  echo "zbm-after-apt: generate-zbm failed; run it by hand and read its messages" >&2
fi
exit 0
```

**FILE** `/etc/apt/apt.conf.d/95-zfsbootmenu`

```
DPkg::Post-Invoke { "if [ -x /usr/local/sbin/zbm-after-apt ]; then /usr/local/sbin/zbm-after-apt; fi"; };
```

```
root # chmod 0755 /usr/local/sbin/zbm-after-apt
root # /usr/local/sbin/zbm-after-apt
root # /usr/local/sbin/zbm-after-apt
```

- APT runs `DPkg::Post-Invoke` commands after `dpkg` has finished, so the kernel's postinst scripts, DKMS and
  `update-initramfs` have run by then.
- `generate-zbm --kver` builds with that kernel even when a newer kernel without a ZFS module is in `/boot`.
- `generate-zbm` runs the post-hook `50-bootx64.sh`, which refreshes `\EFI\BOOT\BOOTX64.EFI`.

**Check:**

```
root # apt-config dump | grep -F zbm-after-apt
root # cat /var/lib/zfsbootmenu/apt-stamp
root # ls -l /boot/efi/EFI/zbm/
```

The first run of the script by hand printed `rebuilding ZFSBootMenu with kernel <version>` and built an image; the
second printed nothing. `apt-config` shows the hook, the stamp names the kernel and the package versions, and the
image's time is that of the first run. After the next APT run that installs a kernel or a ZFS package, the line
`zbm-after-apt: rebuilding ...` appears at the end of APT's output.

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Take
snapshots of it on the running system (`zfs snapshot zroot/ROOT/debian@NAME`) or in ZFSBootMenu.

Get into the menu by pressing **Escape** during the countdown (shell runtime) or any key (Rust runtime).

**Shell runtime (the default).** `MOD` is **Ctrl** or **Alt**. Main screen:

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
| **MOD+R** | roll back | rolls the boot environment back to the snapshot; destructive, asks for confirmation |
| **MOD+C** | clone | `zfs clone`: a new boot environment that depends on the snapshot |
| **MOD+X** | clone and promote | `zfs clone`, `zfs promote`: a new boot environment that keeps the snapshots |
| **Enter** | duplicate | `zfs send \| zfs recv`: an independent copy; uses as much space as the snapshot refers to |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

With the **Rust runtime**, choose the Snapshots tab (**2**) or press **s** on the Boot tab, select the snapshot
and press **r** to roll back; a review page shows what is lost, and the rollback happens once you type the target's
name. **?** lists the keys of every view.

An example:

1. Before a large update: `zfs snapshot zroot/ROOT/debian@pre-update`.
2. After a bad update, reboot, press **Escape** in the countdown, select `zroot/ROOT/debian`, press **MOD+S**,
   select `@pre-update`, press **MOD+R** and confirm.
3. Or, to try the old state without losing the new one, press **MOD+C** (clone) on the snapshot and boot the
   clone.

- Rollback and the other changes need the pool imported read/write; ZFSBootMenu imports pools read-only and
  imports them again when needed. In Option B that needs the key again; the key source set in
  [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties) saves the second prompt.
- A rollback of the boot environment does not touch `zroot/home`.
- A boot environment made by hand needs `canmount=noauto` and `mountpoint=/`; it inherits
  `org.zfsbootmenu:commandline` from `zroot/ROOT`.
- Snapshots and clones contain `/boot`, so each boots with the kernel and initramfs (and, in Options B and C, the
  key file) it had when the snapshot was taken. After the passphrase (Option B) or the key file (Option C) was
  replaced, an older snapshot's initramfs holds the old one: its system then stops in its initramfs or asks for the
  passphrase.
- To boot an older kernel of the same boot environment, press **MOD+K** in the menu. To make ZFSBootMenu prefer a
  kernel, set `org.zfsbootmenu:kernel` (see
  [A new kernel has no ZFS module](#a-new-kernel-has-no-zfs-module)).
- `zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the `org.zfsbootmenu:commandline` property
  that every boot environment inherits; see `man zbm-kcl`. Without a dataset name `zbm-kcl` acts on the root file
  system, `zroot/ROOT/debian`, and a change there sets a property of its own on that boot environment only.
  `zbm-kcl -r ARGUMENT zroot/ROOT` removes an argument.

### Changing the passphrase

**Option A.** There is nothing to change.

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
> Do not reboot before `update-initramfs` has finished and the Check below passes. An initramfs with the old file
> cannot load the key, and the boot stops in its shell.

```
root # update-initramfs -u -k all
```

ZFSBootMenu's image holds no key, so no `generate-zbm` is needed.

**Check:**

```
root # ls -l /boot/initrd.img-*
root # for i in /boot/initrd.img-*; do lsinitramfs "$i" | grep -q 'etc/zfs/zroot.key' && echo "$i: key"; done
```

Every initramfs is `-rw-------` and contains the key file.

**Option C.** It asks for the old passphrase, then the new one twice:

```
root # cryptsetup luksChangeKey /dev/disk/by-partlabel/zroot-luks
```

The key file's slot is not affected, and no image needs rebuilding. Make new header backups afterwards: the old
ones still accept the old passphrase. A second passphrase as a reserve:
`cryptsetup luksAddKey /dev/disk/by-partlabel/zroot-luks`.

### Replacing the key file

**Option A.** There is no key file.

**Option B.** The key file is the passphrase; [Changing the passphrase](#changing-the-passphrase) replaces both.

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
root # update-initramfs -u -k all
```

`luksAddKey` asks for the passphrase. Rebuilding the initramfs is required: it contains a copy of the key file.
Continue only when `update-initramfs` printed no error and `ls -l /boot/initrd.img-*` shows every image
`-rw-------`, with the time of this rebuild. Then remove the old key slot and the old file:

```
root # cryptsetup luksRemoveKey /dev/disk/by-partlabel/zroot-luks zroot.key.old
root # rm zroot.key.old
root # cd
root # cryptsetup luksDump /dev/disk/by-partlabel/zroot-luks | grep -E '^\s+[0-9]+: luks2'
```

`luksDump` lists two key slots: the passphrase's and the new key file's (one more for each reserve passphrase).
Make a new header backup afterwards.

### Option C only: backing up the LUKS header

Skip this section for Options A and B. (In Option B the wrapped keys are in the pool's metadata; there is no
separate header to back up.)

A damaged LUKS header makes the data unrecoverable, even with the right passphrase. Store a header backup **away
from this SSD**, for example on an encrypted USB stick:

```
root # cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks \
  --header-backup-file /PATH/ON/USB/zroot-luks-header.img
```

- To restore it: `cryptsetup luksHeaderRestore /dev/disk/by-partlabel/zroot-luks --header-backup-file FILE`.
- A header backup holds the key slots as they were when it was made: whoever has it and an old passphrase (or the
  old key file) can open the container. Keep it offline, and make a new one after every change of passphrase or
  key file.

**Check:** `ls -l /PATH/ON/USB/zroot-luks-header.img` shows a file of several MiB.

### Scrubbing and trimming

A scrub reads and checks every block; a full TRIM complements `autotrim`:

```
root # zpool scrub zroot
root # zpool trim zroot
root # zpool status -t zroot
```

Debian's `zfsutils-linux` has scheduled a monthly scrub and TRIM itself in the past (`/etc/cron.d/zfsutils-linux`).
Whether testing's package still does, or ships OpenZFS's timer units instead, could not be confirmed here. Look:

```
root # cat /etc/cron.d/zfsutils-linux
root # systemctl list-unit-files 'zfs-scrub-*' 'zfs-trim-*'
```

If the cron file exists, it schedules both; for Option C read the note below. If it does not and the timer units
are listed, enable them:

```
root # systemctl enable --now zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

- Option C: TRIM reaches the SSD because the flag is in the LUKS header. `lsblk -D` shows non-zero discard sizes
  for `luks-<UUID>`.
- Option C and the cron file: Debian's monthly TRIM job has, in the past, trimmed only pools whose property
  `org.debian:periodic-trim` is `enable`, or, when it is `auto` (the default), pools on NVMe devices; a pool on
  `luks-<UUID>` may not count as one. This could not be confirmed here; the job's script, named in the cron file,
  shows the rule. `autotrim=on` trims continuously in any case, and
  `zfs set org.debian:periodic-trim=enable zroot` makes the monthly TRIM run.
- The SSD's health: `nvme smart-log /dev/nvme0`. Watch `critical_warning` (should be `0`), `percentage_used`,
  `media_errors` and `available_spare`.

### Updating ZFSBootMenu

```
root # cp -a /etc/zfsbootmenu/config.yaml /root/zbm-config.yaml.keep
root # git -C /usr/src/zbm checkout -- crates/zbm-platform/src/version.rs legacy/zfsbootmenu/zbm-release
root # git -C /usr/src/zbm status --short
root # git -C /usr/src/zbm pull
root # git -C /usr/src/zbm log --oneline ORIG_HEAD..HEAD
root # make -C /usr/src/zbm
root # make -C /usr/src/zbm install
root # cp -a /root/zbm-config.yaml.keep /etc/zfsbootmenu/config.yaml
root # generate-zbm
```

- `make install` overwrites `config.yaml`, hence the copy and the restore. `generic.conf` and `runtime.conf` are
  kept.
- `make install` writes the version into two files of the checkout, and `git pull` refuses to run when the update
  changes either of them. The `git checkout --` line restores both; `git status --short` must then print nothing.
- `claude/zbm-refactor` is a development branch: read what `git log` shows before building.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm that it works before relying on
it; the backup entry still boots the previous image.

### Relaxing the pool compatibility

Most readers can skip this. The pool was created with the features of OpenZFS 2.2 only, so that live systems with
ZFS 2.2 or later can import it. To enable newer features, update every system that imports the pool first,
starting with ZFSBootMenu, then raise the set one step at a time:

```
root # generate-zbm
root # zpool set compatibility=openzfs-2.3-linux zroot
root # zpool upgrade zroot
```

- Use the newest set that the installed ZFS **and** your rescue medium both know
  (`ls /usr/share/zfs/compatibility.d/`). `compatibility=off` enables everything the installed ZFS has.
- When ZFSBootMenu's ZFS is older than the pool's features, it shows *Unsupported features detected* and refuses
  read/write operations, or cannot import the pool at all.
- One `generate-zbm` run refreshes `vmlinuz.EFI` and, through the post-hook, `BOOTX64.EFI`; `vmlinuz-backup.EFI`
  is then the previous image. Before `zpool upgrade`, boot the new image once and run `generate-zbm` a second time,
  so that the backup holds the new ZFS too. A `known-good.EFI` copy, if you made one, is never updated: copy the new
  image over it, or delete it and its entry (`efibootmgr -b XXXX -B`).

**Check:** `zpool get compatibility zroot` shows the new set, and the next boot through ZFSBootMenu works.

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

- Check that Secure Boot is disabled ([Installation media](#installation-media)).
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

**Option C.** Also:

```
root # zluks list
root # zluks
root # exit
```

- No `/dev/nvme*`: the NVMe driver is not in the image. Check that `/etc/zfsbootmenu/dracut.conf.d/generic.conf`
  exists on the installed system, or add `add_drivers+=" nvme "` to a file there, and run `generate-zbm`.
- `zpool import` lists the pool but names unsupported features: ZFSBootMenu's ZFS is older than the pool's
  features; rebuild the image on the installed system.
- Option C: `/proc/cmdline` must contain `zbm.luks=UUID=` with the UUID of the **LUKS container**; a pool GUID or a
  file system UUID there is the usual mistake. Fix `Kernel.CommandLine` and run `generate-zbm`. `zluks list` shows
  which LUKS volumes ZFSBootMenu can see; `zluks` unlocks by hand, and after `exit` ZFSBootMenu imports the pool.
- Options A and B with `zbm.luks=off`: there is nothing to unlock; look at the first two points.

### Option C: ZFSBootMenu never asks for the passphrase (the image lacks cryptsetup)

ZFSBootMenu never asks for a passphrase and keeps retrying the pool import. In its recovery shell,
`command -v cryptsetup` prints nothing; with the shell runtime `zluks` is missing too, while with the Rust runtime
`zluks` exists but cannot open the container. `cryptsetup` was not installed when the image was built, and dracut
then leaves LUKS out without a message. The cure needs the installed system: follow
[Repairing the system from the live medium](#repairing-the-system-from-the-live-medium), run
`apt install cryptsetup` in the chroot, run `generate-zbm`, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

The passphrase is asked for a second time, or the system stops in its initramfs. After ZFSBootMenu started
Debian's kernel, Debian's initramfs could not use the key file:

- **Option B**: `zfs load-key` fails, the root file system cannot be mounted, and the boot stops in the initramfs
  shell (`(initramfs)` prompt). There, `zfs load-key -L prompt zroot` loads the key by hand; whether `exit` then lets
  the boot continue was not checked. The live medium always works
  ([Repairing the system from the live medium](#repairing-the-system-from-the-live-medium)).
- **Option C**: cryptsetup's initramfs script tries a key file only once (`maximum number of tries exceeded`), then
  the pool cannot be imported and the boot stops in the initramfs shell. It does not fall back to a passphrase
  prompt for an entry with a key file. (This was read in Ubuntu's cryptsetup-initramfs, which is derived from
  Debian's; it was not seen on Debian Testing.) There, `cryptsetup open /dev/disk/by-partlabel/zroot-luks
  luks-<UUID>` opens the container by hand; whether `exit` then lets the boot continue was not checked.

Then, on the running system:

```
root # cat /proc/cmdline
root # grep -v '^#' /etc/crypttab
root # lsinitramfs "/boot/initrd.img-$(uname -r)" | grep -E 'zroot\.key|cryptroot'
root # ls -l "/boot/initrd.img-$(uname -r)"
```

Causes, in order of likelihood:

- the key file is not in the initramfs: Option B's hook `/etc/initramfs-tools/hooks/zfs-key` is missing or not
  executable; Option C's `/etc/crypttab` line, its `initramfs` option or the `KEYFILE_PATTERN` line is missing. Fix
  it and run `update-initramfs -u -k all`;
- Option C: the UUID in `/etc/crypttab` is not the container's
  (`cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks`);
- Option B: the passphrase was changed without rebuilding the initramfs
  ([Changing the passphrase](#changing-the-passphrase));
- an old snapshot or clone was booted, whose initramfs holds an older key
  ([Boot environments, snapshots and rollback](#boot-environments-snapshots-and-rollback)).

If ZFSBootMenu **itself** asks twice, that is something else: in Option C without `zbm.luks=`, it offers every LUKS
volume it finds.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment. In the recovery shell (Option B: run `zfs load-key -L prompt zroot`
first; a plain `zfs load-key` looks for the key file, which is not in ZFSBootMenu's image):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/debian
root # ls -l MOUNTPOINT/boot
```

`mount_zfs` prints the mount point; use it for `MOUNTPOINT`. Causes:

- `/boot` in the dataset is empty: the kernels were installed onto the ESP or another partition that was mounted
  at `/boot` (for example by the package `systemd-boot`);
- a kernel has no `initrd.img-<version>` beside it;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

ZFS refuses the import because the pool was last used by another system, or its host ID does not match.


- In ZFSBootMenu: the default import policy adopts the host ID of the pool when it cannot import it with its own,
  so this usually resolves itself. Otherwise, in the recovery shell: `zpool import -f -N zroot`, then `exit`.
- In Debian's initramfs (the `(initramfs)` prompt): `zpool import -f -N zroot`, then `exit`.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; run
  `update-initramfs -u -k all` and `generate-zbm`. The usual cause is a live medium that imported the pool and was
  shut down without `zpool export`: always export the pool on a rescue medium.
- `generate-zbm` printing `SPL (...) and system (...) hostids do not match!` means the loaded ZFS module and
  `/etc/hostid` disagree on the build host. It is a warning only; the image takes `/etc/hostid`.

### The recovery shell and its tools

| Command | Use |
|---|---|
| `zfsbootmenu` (or `exit`) | back to the menu |
| `zpool`, `zfs` | the ZFS tools of the image; `zpool import`, `zpool status` |
| `set_rw_pool POOL`, `set_ro_pool POOL` | import the pool again read/write or read-only |
| `mount_zfs DATASET` | mount a file system at a temporary place and print it |
| `zfs-chroot DATASET` | chroot into a boot environment (read/write only after `set_rw_pool`) |
| `zsnapshots DATASET` | the snapshot browser |
| `zkexec DATASET KERNEL INITRAMFS` | boot a chosen kernel and initramfs directly |
| `mount_esp DEVICE` | mount an ESP |
| `zluks`, `zluks list`, `zluks status`, `zluks open DEVICE [NAME]`, `zluks close` | Option C: LUKS volumes |
| `logs`, `zlogtail`, `zreport`, `zbmcmdline` | logs, a report for bug reports, ZFSBootMenu's command line |
| `help` | the built-in documentation |
| `cryptsetup`, `dmsetup` | Option C: the LUKS and device-mapper tools |
| `reboot`, `poweroff`, `firmware-setup` | leave |

### Repairing the system from the live medium

Boot the live medium, install ZFS as in [Installation media](#installation-media), and set:

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
```

**Option C.**

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

**All options.** Give the live system the host ID of the installed system first, so that the import does not
stamp the pool with another one:

```
root # zgenhostid -f 0x00bab10c
root # zpool import -f -N -R /mnt zroot
```

**Option B.**

```
root # zfs load-key -L prompt zroot
```

**All options.**

```
root # zfs mount zroot/ROOT/debian
root # zfs mount zroot/home
root # mount "${ESP_PART}" /mnt/boot/efi
```

Enter the system. Its `/etc/resolv.conf` is a link into `/run/systemd/resolve/`, so the live system's name
servers go there (into the live system's `/run`, which the chroot shares, not onto the SSD):

```
root # mount --types proc /proc /mnt/proc
root # mount --rbind /sys /mnt/sys && mount --make-rslave /mnt/sys
root # mount --rbind /dev /mnt/dev && mount --make-rslave /mnt/dev
root # mount --bind /run /mnt/run && mount --make-slave /mnt/run
root # mkdir -p /run/systemd/resolve
root # [ -e /run/systemd/resolve/stub-resolv.conf ] || cp -L /etc/resolv.conf /run/systemd/resolve/stub-resolv.conf
root # chroot /mnt /bin/bash --login
(chroot) root # export POOL_PART=/dev/nvme0n1p2
(chroot) root # getent hosts deb.debian.org
```

The last command prints an address. For Option C, also set
`export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"` in the chroot. Repair, then leave and close everything:

```
(chroot) root # exit
root # umount /mnt/boot/efi
root # umount -R -l /mnt/dev /mnt/sys /mnt/proc /mnt/run
root # zfs umount -a
root # zpool export zroot
```

Option C: `cryptsetup close "luks-${LUKS_UUID}"` as well. With the same host ID, `-f` is not strictly needed for
the import; it lets the import proceed if the pool still records another host. Always export the pool before
leaving the live system. The stored TRIM flag is applied by `cryptsetup open` by itself.

Debian 13's live system has an older OpenZFS than testing. It can import the pool as long as the pool's
compatibility set is one it knows; see [Relaxing the pool compatibility](#relaxing-the-pool-compatibility).

### A new kernel has no ZFS module

During `apt full-upgrade`, DKMS printed an error for `zfs`, for example
`Cannot build against kernel version ... The maximum supported kernel version is ...`, and the loop of
[Telling whether a new kernel has its ZFS module](#telling-whether-a-new-kernel-has-its-zfs-module) shows `MISSING`
for the new kernel. Its initramfs cannot import the pool, and ZFSBootMenu would boot it because it is the newest.

Whether the kernel's installation itself fails when DKMS cannot build the module depends on Debian's `dkms`
version; this guide does not rely on either behaviour.

What to do, before rebooting:

1. Make ZFSBootMenu prefer the kernel that works. Replace `VERSION` with the version of that kernel (`uname -r` if
   it is the running one):

   ```
   root # zfs set org.zfsbootmenu:kernel=vmlinuz-VERSION zroot/ROOT/debian
   ```

   If the value matches no kernel, ZFSBootMenu falls back to the newest. Remove the property once ZFS supports
   the new kernel: `zfs inherit org.zfsbootmenu:kernel zroot/ROOT/debian`.

   Protect that kernel from `apt autoremove`, which keeps only a few kernels and could remove it once more new
   ones arrive:

   ```
   root # apt-mark manual linux-image-VERSION
   ```

   Release it later with `apt-mark auto linux-image-VERSION`.
2. Hold the kernel ([Holding the kernel](#holding-the-kernel)), so that no further kernels arrive.
3. Wait until testing's `zfs-dkms` supports the kernel, then `apt full-upgrade`, check again, remove the property and
   the hold. Or take ZFS from unstable ([APT pinning](#apt-pinning-zfs-from-unstable)) if unstable already has a
   version that supports the kernel.
4. Alternatively, remove the new kernel: `apt remove linux-image-NEWVERSION`, with the version shown by `ls /boot`.

If you already rebooted into the kernel without a module, the boot stops in Debian's initramfs. Reboot, press
**Escape** in ZFSBootMenu's countdown, select the boot environment, press **MOD+K**, choose the older kernel and boot
it; then follow the steps above.

The ZFSBootMenu image itself is not affected as long as it was built with a kernel that has a module; the APT hook
of [Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image) only uses such a kernel.

If this happens during the installation (in [Kernel and ZFS](#kernel-and-zfs)), testing's only kernel is too new for
its ZFS. Take ZFS from unstable with the pinning above, inside the chroot, or wait a few days and start again.

### APT wants to install dracut or remove initramfs-tools

If an update proposes to install the package `dracut` and remove `initramfs-tools`, answer **No**. That can happen
if testing's kernel packages come to prefer dracut. This guide's Option B and C configuration is for initramfs-tools
only; with dracut, Debian's initramfs would not unlock the pool without other settings. Keep initramfs-tools
installed:

```
root # apt-mark manual initramfs-tools
root # apt install initramfs-tools
```

and read APT's list before every `full-upgrade`. `dracut-core`, which ZFSBootMenu needs, does not conflict with
initramfs-tools.

### The live system cannot install ZFS

- `apt` cannot find `linux-headers-<version>` for the live kernel: the live image is older than the kernel in the
  archive. Download the current Debian 13 live image.
- `modprobe zfs` prints *Key was rejected by service*: Secure Boot is on and refuses the module that DKMS built.
  Turn Secure Boot off in the firmware setup and boot the live system again.
- `zfs-dkms` has no candidate on the live system: `contrib` is missing from `/etc/apt/sources.list`; add it and run
  `apt update`.
- Inside the chroot, `zfs-dkms` has no candidate: the package is not in testing today. Use the pinning of
  [APT pinning](#apt-pinning-zfs-from-unstable) inside the chroot (with `unstable` in the sources), or wait until
  it returns to testing.

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption leaves readable without the passphrase: dataset and snapshot names, the dataset hierarchy,
  properties, file sizes, holes and deduplication tables. File contents, file attributes, ACLs, permission bits and
  directory listings are encrypted. So the names `zroot/ROOT/debian` and `zroot/home`, every snapshot name, how
  much each holds, and all properties, including `org.zfsbootmenu:commandline`, can be read from the SSD.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Debian's initramfs in
  `/boot`, also inside the pool. It is safe only there. Any copy of the boot environment that is not encrypted
  carries it in clear: a `zfs send` without `-w` (raw), a file-level backup of `/etc` or `/boot`, an initramfs
  copied to the ESP. On the running system, root can read the passphrase.
- Changing the passphrase does not change the key the data is encrypted with: `zfs change-key` "does not
  necessarily protect existing or newly-written data" (OpenZFS). It is no cure for a passphrase that someone else
  learned together with a copy of the disk.
- Never place encryption keys inside a ZFSBootMenu image: it lives on the unencrypted ESP.

**Option C.**

- The ESP is not encrypted and holds only ZFSBootMenu's image and its copies. They contain no key: plain key files
  are never copied into the image.
- The key file opens the container. It is safe because `/etc/cryptsetup-keys.d` and the initramfs in `/boot` are
  inside the encrypted pool. It stops being safe the moment an initramfs with the key is written anywhere else: the
  ESP, an unencrypted partition, a backup. Snapshots and `zfs send` streams of the root dataset contain it too;
  treat them like the pool.
- A LUKS header backup contains the key slots as they were: an old passphrase still opens a restored header. Store
  backups offline and replace them after key changes.
- ZFSBootMenu closes the container before `kexec`, which wipes the volume key from kernel memory.
- TRIM pass-through reveals which blocks are unused, not their content.

**All options.**

- **The ESP is not encrypted.** Anyone with access to the disk, or root on the system, can replace ZFSBootMenu's
  image with one that records the passphrase. Encryption protects data at rest, for example a stolen powered-off
  laptop, not against someone who can modify the computer and wait for you to type the passphrase. Secure Boot
  with signed images, or measured boot with a TPM, narrows that; neither is set up by this article. For signing,
  see *Secure Boot signing* in [Building ZFSBootMenu](../../general/building.rst).
- Once the pool is unlocked, the recovery shell and the command-line editor are open to anyone at the keyboard.
- Hooks run as root in ZFSBootMenu and in APT; install only hooks you have read.
- **Debian Testing** does not get security updates the way stable does, and holding the kernel stops kernel
  security fixes.
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed
  before updating, keep the backup entries, and keep the live medium.

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
- [Gentoo Linux on a LUKS-encrypted ZFS root](../gentoo/zfs-luks-v4.md), a guide with the same design for Option C
  on Gentoo
- [The threat model](../../dev/threat-model.md)
- Debian: [Debian testing](https://www.debian.org/devel/testing), [ZFS](https://wiki.debian.org/ZFS),
  [Debootstrap](https://wiki.debian.org/Debootstrap), [initramfs-tools](https://wiki.debian.org/initramfs-tools),
  `crypttab(5)` and `/usr/share/doc/cryptsetup-initramfs/README.initramfs.gz` on the installed system (Option C),
  Debian bug [#1114857](https://bugs.debian.org/1114857) (dracut as the default initramfs generator)
- [OpenZFS documentation](https://openzfs.github.io/openzfs-docs/), with its *Getting Started* pages for Debian
- `gdisk(8)`, `cryptsetup(8)`, `apt_preferences(5)`, `dkms(8)`
