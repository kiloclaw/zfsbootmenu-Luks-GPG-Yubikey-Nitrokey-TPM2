# Ubuntu on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install Ubuntu 26.04 LTS ("Resolute Raccoon") from scratch on a **single NVMe SSD**
with a **ZFS root pool**. The system boots with the ZFSBootMenu build from this repository (branch
`claude/zbm-refactor`). Ubuntu is installed with `debootstrap` from the Ubuntu Desktop live image. The ZFS kernel
module comes with Ubuntu's kernel packages, and Ubuntu's own initramfs is built by **dracut**, Ubuntu's default
initramfs generator for installed systems since 25.10. (The live image itself still boots with `initramfs-tools`, which
its `casper` package needs; that does not matter for the installed system.)

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Ubuntu's initramfs then unlocks the pool again
with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from the documentation and package archive of Ubuntu, from OpenZFS, dracut and this
> repository. The project has not run it from start to finish. `claude/zbm-refactor` is a development branch, not a
> release.
> The project's VM tests boot ZFSBootMenu images built with dracut and a test system whose initramfs is built
> with dracut, with unencrypted pools, ZFS native encryption and LUKS; they do not install Ubuntu.
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
  - [Running debootstrap](#running-debootstrap)
  - [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file)
  - [Chrooting](#chrooting)
  - [Configuring APT](#configuring-apt)
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
  - [The live system cannot load ZFS](#the-live-system-cannot-load-zfs)
  - [dracut cannot find the zfs module](#dracut-cannot-find-the-zfs-module)
  - [APT wants to install initramfs-tools or remove dracut][apt-initramfs-tools]
- [Security notes](#security-notes)
- [See also](#see-also)

[no-cryptsetup]: #option-c-zfsbootmenu-never-asks-for-the-passphrase-the-image-lacks-cryptsetup
[second-unlock]: #the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs
[hostid-refused]: #the-pool-is-refused-wrong-host-id-or-last-used-by-another-system
[apt-initramfs-tools]: #apt-wants-to-install-initramfs-tools-or-remove-dracut

## Overview

### Choosing an option

| | Option A | Option B | Option C |
|---|---|---|---|
| Pool | not encrypted | ZFS native encryption (`aes-256-gcm`) | inside a LUKS2 container |
| Passphrase typed | never | once, in ZFSBootMenu | once, in ZFSBootMenu |
| Second unlock, by Ubuntu's initramfs | none | key file (1) | key file (2) |
| Partition 2 type | `BF00` | `BF00` | `8309` |
| Visible without the passphrase | everything | the pool's structure (3) | the container's size and LUKS header |
| Extra software | none | none | `cryptsetup` (4) |

1. `/etc/zfs/zroot.key`.
2. `/etc/cryptsetup-keys.d/zroot.key`.
3. Dataset names, sizes, properties and snapshot names; not file contents or file names.
4. On the live system, in the new system and in ZFSBootMenu's image.

Option B stores the **passphrase itself** in its key file: OpenZFS has one key per encryption root, and the file
named by the `keylocation` property lets the system load it without asking. Option C stores a **different, random**
key in a second LUKS key slot. Both could be combined; this guide does not.

### Boot process

1. The UEFI firmware starts **ZFSBootMenu**, one EFI file on the EFI system partition (ESP).
2. **Option C**: ZFSBootMenu reads `zbm.luks=` from its kernel command line, asks for the **LUKS passphrase** and
   opens the container.
3. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/ubuntu` with a countdown of 10 seconds.
   When the countdown ends, the pool's `bootfs` is booted; a key press opens the menu instead.
4. **Option B**: the `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in
   ZFSBootMenu's image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime this
   prompt comes **after** the countdown.
5. ZFSBootMenu loads Ubuntu's kernel and initramfs from `/boot` **inside the boot environment** and starts the kernel
   with `kexec`. It passes the command line stored in the ZFS property `org.zfsbootmenu:commandline`, the root
   argument `root=zfs:zroot/ROOT/ubuntu` and the host ID. **Option C**: before `kexec` it exports the pool and closes
   the container, which wipes the volume key from memory.
6. Ubuntu's initramfs (dracut, with systemd and OpenZFS's `zfs` dracut module from the package `zfs-dracut`) starts
   in the new kernel:
   - **Option A**: it imports the pool and mounts the root file system.
   - **Option B**: it imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the initramfs**,
     without asking.
   - **Option C**: systemd-cryptsetup unlocks the container again with the key file
     `/etc/cryptsetup-keys.d/zroot.key` **inside the initramfs**, without asking; then the pool is imported.

Why a second unlock (Options B and C): `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. The boot environment's own initramfs must unlock the pool again, exactly as it would without
ZFSBootMenu. ZFSBootMenu does not replace Ubuntu's initramfs.

### Where the keys are

**Option A.** There are none.

| Key | Option | Stored where | Used by |
|---|---|---|---|
| ZFS passphrase | B | your memory | ZFSBootMenu at every boot; a live system for repairs |
| The same passphrase in `/etc/zfs/zroot.key` | B | in the pool (see below) | Ubuntu's initramfs |
| LUKS passphrase (key slot 0) | C | your memory | ZFSBootMenu at every boot; a live system for repairs |
| Random key file `/etc/cryptsetup-keys.d/zroot.key` (slot 1) | C | in the pool (see below) | Ubuntu's initramfs |

"In the pool" means twice: the file itself in `/etc`, and a copy of it in Ubuntu's initramfs in `/boot`. Both
directories are inside the encrypted pool.

The key file in Ubuntu's initramfs is safe **only because** the file and the initramfs both live inside the encrypted
pool (`/etc` and `/boot`); they cannot be read before the passphrase has been typed. ZFSBootMenu's own image lives on
the unencrypted ESP and never contains a key.

### What the project has tested

From [Testing](../../luks/testing.rst):

- The VM tests boot ZFSBootMenu images built **with dracut** by the project's build container, which is **Ubuntu
  26.10** (Linux 7.3, OpenZFS 2.4.4, cryptsetup 2.8.7, systemd 261, dracut 112), in QEMU, with `hostonly="no"`.
- The system they boot is a test payload, not an installed distribution: Ubuntu's kernel with a **dracut** initramfs
  built with `--no-hostonly` that holds OpenZFS's `zfs` module and systemd-cryptsetup, and a minimal root file
  system. It is not an installed Ubuntu.
- For this guide that means: it is the closest of the project's guides to the tested case. Both generators are the
  tested ones, and Ubuntu's kernel and dracut initramfs are what the tests boot. **Not** covered: Ubuntu 26.04 itself
  (its Linux 7.0, OpenZFS 2.4.1, systemd 259 and dracut 110 differ from the container's), an image built on the
  installed system, the `debootstrap` installation and real hardware.
- The container's coreutils are uutils' with one hard link per command. On 26.04 each command in `/usr/bin` is a
  symbolic link to such a hard link (`/usr/lib/cargo/bin/coreutils/<command>`). An image built on that layout is not
  tested; the Check of [Building and checking the image](#building-and-checking-the-image) looks for the commands.
- **Option A**: unencrypted pools are booted with the Rust runtime in every run of the suite, and with the shell
  runtime in the run of 20 October 2026 (the shell runtime's tests run only when named).
- **Option B**: ZFSBootMenu's passphrase prompt is tested. The single-prompt arrangement, with the key file in the
  system's initramfs, is **not**: the test payload's initramfs holds no ZFS key file. It is the design described in
  [Native Encryption](../../general/native-encryption.rst).
- **Option C**: the design of this guide is tested on the payload: an NVMe disk with 4 KiB sectors, a LUKS2 container
  with 4 KiB encryption sectors and the TRIM flag in its header, `zbm.luks=UUID=`, and a dracut initramfs that
  unlocks again with a key file and the `luks.uuid=`, `luks.key=` and `luks.crypttab=no` parameters, as here.
- Secure Boot is not covered.

## Requirements

### Hardware

- An x86_64 computer with an NVMe SSD that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware: the ZFSBootMenu image built here is not signed. How to check is shown in
  [Preparing the NVMe SSD](#preparing-the-nvme-ssd).
- An internet connection, wired if possible.
- Free space for the build: about 3 GB in the new pool while ZFSBootMenu is built. The source alone, with its
  vendored crates, is about 0.9 GB; Cargo's build directory comes on top and can be deleted after the installation.

### Installation media

The facts this guide depends on, and how to check each on your system. Versions are those of Ubuntu's package archive
on 17 November 2026; updates raise them.

| Fact | Ubuntu 26.04 LTS | Check |
|---|---|---|
| Release | 26.04 LTS `resolute` (23 April 2026); point release 26.04.1 | `grep -E '^VERSION' /etc/os-release` |
| Kernel | Linux 7.0, package `linux-image-generic` | `uname -r` |
| ZFS kernel module | in the kernel packages (`linux-main-modules-zfs-<version>`) (1) | `modinfo -F version zfs` |
| OpenZFS tools | 2.4.1, package `zfsutils-linux` | `zfs version` |
| System initramfs | dracut 110 and OpenZFS's module from `zfs-dracut` (2) | `dracut --version` |
| systemd | 259 | `systemctl --version` |
| `rustc`, `cargo` | 1.93 | `rustc --version` |
| cryptsetup | 2.8.4 | `cryptsetup --version` |
| gdisk | 1.0.10 | `dpkg-query -W gdisk` |
| EFI stub | package `systemd-boot-efi`, in `universe` | `dpkg -S /usr/lib/systemd/boot/efi/linuxx64.efi.stub` |
| Core utilities | uutils (`coreutils-from-uutils`) (3) | `ls -l /usr/bin/install` |

1. Built for each kernel by Ubuntu; there is no DKMS.
2. Images are `/boot/initrd.img-<version>`.
3. Rust's implementation is the default; GNU's programs are installed as well, as `gnuinstall`, `gnurealpath` and so
   on.

**The newest interim release** is Ubuntu 26.10, code name `stonking` (released on 15 October 2026), which the
project's build container uses. Its archive has Linux 7.3, OpenZFS 2.4.4 (`zfs-dracut` 2.4.4), dracut 112 and Rust
1.97; its support ends after nine months. This guide was written for 26.04 LTS; the rest of the package names were not
checked for 26.10, and `apt-cache policy PACKAGE` shows whether each exists. For 26.10:

- replace `resolute` by `stonking` in [Configuring APT](#configuring-apt);
- in [Running debootstrap](#running-debootstrap), the `debootstrap` of the 26.04 live image has no script for
  `stonking`. Name Ubuntu's common script:
  `debootstrap stonking /mnt http://archive.ubuntu.com/ubuntu /usr/share/debootstrap/scripts/gutsy`. From the 26.10
  live image, take the code name from `grep VERSION_CODENAME /etc/os-release`.

Download the **Ubuntu 26.04 LTS Desktop** image for x86_64 from <https://ubuntu.com/download/desktop>, write it to a
USB stick and boot it in **UEFI** mode. Choose **Try Ubuntu**; the live user is `ubuntu` and needs no password. Connect
to the network and open a terminal.

The 26.04.1 Desktop image already has the ZFS module for its kernel, `zfsutils-linux`, `cryptsetup`, `gdisk`,
`dosfstools` and `efibootmgr` (its manifest lists them); `debootstrap` and `nvme-cli` must be installed. The live
system runs entirely in memory: what you install or write on it is gone after a reboot (see
[Continuing after a reboot of the live system](#continuing-after-a-reboot-of-the-live-system)).

Stop the desktop from mounting old partitions of the SSD (run this as `ubuntu`, before becoming root):

```
ubuntu $ gsettings set org.gnome.desktop.media-handling automount false
```

Become root and install the tools this guide uses. APT leaves the packages the live image already has as they are.

```
ubuntu $ sudo -i
root # apt update
root # apt install --yes debootstrap gdisk zfsutils-linux dosfstools nvme-cli
root # modprobe zfs
```

**Option C** also needs `cryptsetup`, which is on the live image (`apt install --yes cryptsetup` if a check below
reports it missing).

All these packages are in Ubuntu's `main` component. If `modprobe zfs` reports `Module zfs not found`, see
[The live system cannot load ZFS](#the-live-system-cannot-load-zfs).

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new Ubuntu system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |
| `<Enter>` | in a transcript: press Enter without typing anything |
| **Option A.** / **Option B.** / **Option C.** | the next paragraph and commands are for that option only |
| **All options.** | the text is for every option again |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Preparing the NVMe SSD

Everything in this section runs on the live system. First check that it booted in UEFI mode, that ZFS works, and
that Secure Boot is off.

**Check:**

```
root # ls /sys/firmware/efi
root # zfs version
root # dpkg-query -W gdisk
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
root # cryptsetup --version
```

- `ls` lists files; otherwise the live system booted in legacy BIOS mode.
- `zfs version` prints `zfs-2.4...` (the tools) and `zfs-kmod-2.4...` (the module), or later versions. The pool in
  this guide needs 2.2 or later.
- `gdisk` is version 1.0.9 or later (26.04 has 1.0.10).
- The last number printed by `od` is `0` when Secure Boot is disabled and `1` when it is enabled. If it is `1`,
  disable Secure Boot in the firmware setup first. If the file does not exist, the firmware has no Secure Boot.
  (`mokutil --sb-state` shows the same where `mokutil` is installed.)
- `cryptsetup --version` (Option C only) prints 2.4 or later.

### Identifying the SSD

Why: every following command acts on one device, and the wrong one would be erased.

```
root # nvme list
root # lsblk -d -o NAME,SIZE,MODEL,SERIAL,TRAN
```

`nvme list` shows each NVMe namespace, such as `/dev/nvme0n1`, with its model, serial number and size. `nvme0` is the
controller, `n1` its first namespace; the partitions created below will be `/dev/nvme0n1p1` and `/dev/nvme0n1p2`. In
the `lsblk` output the `TRAN` column of an NVMe SSD shows `nvme`. The live USB stick shows up as `sda` or similar.

Kernel names can change between boots when a computer has several NVMe SSDs. This guide uses the kernel name because
gdisk, `efibootmgr` and `blkdiscard` take it, and a single-SSD computer has only `nvme0n1`. With more than one NVMe
SSD, compare the serial number with `ls -l /dev/disk/by-id/ | grep nvme | grep -v -- -part` before every destructive
step and after every reboot of the live system. ZFS itself finds the pool by its labels, so a changed kernel name
does not stop an import later.

### Setting variables

Why: the commands of this guide use these variables, so that they can be copied unchanged.

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
root # export POOL=zroot
root # export BE=ubuntu
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the EFI system partition (1) |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `ubuntu` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}` |
| `POOL_DEV` | A, B: `${POOL_PART}`; C: `/dev/mapper/luks-${LUKS_UUID}` | A B C | the device `zpool create` gets (2) |
| `LUKS_UUID` | `$(cryptsetup luksUUID "${POOL_PART}")` | C | the LUKS container's UUID (3) |

1. NVMe partitions have a `p` before the number.
2. Set in [Creating the ZFS pool](#creating-the-zfs-pool) (A, B) or in
   [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container) (C). It is needed up to
   `zpool create` and for the variables file written in
   [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file).
3. It exists only after `luksFormat`.

The name in `BE` is free; ZFSBootMenu does not read it. It recognizes the distribution from `/etc/os-release` inside
the boot environment.

Some paths are written literally, because files cannot expand variables. If you change `POOL`, change `zroot` in
these paths too: `/etc/zfs/zroot.key` (Option B), `/etc/cryptsetup-keys.d/zroot.key` (Option C), and every later
command that names `zroot` after the installation.

> **Tip**
> If only the terminal was closed, set these variables again before continuing; for Option C, set `LUKS_UUID` and
> `POOL_DEV` again as well (see
> [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container)). If the live system was
> rebooted, much more is lost: follow
> [Continuing after a reboot of the live system](#continuing-after-a-reboot-of-the-live-system).

**Check:**

```
root # lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"
```

The model and serial number are those of the SSD to erase.

### Optional: switching to 4 KiB sectors

Why: many NVMe SSDs ship with 512-byte logical sectors and also support 4096-byte sectors, which can be slightly
faster. Show the formats the SSD supports:

```
root # nvme id-ns -H "${DISK}" | grep -E '^LBA Format'
```

If a format with `Data Size: 4096 bytes` and `Metadata Size: 0` is listed and not marked `(in use)`, switch to it,
where `N` is its number:

> **Warning**
> `nvme format` erases the whole SSD immediately.

```
root # nvme format "${DISK}" --lbaf=N --force
```

If only 512-byte formats are listed, skip this step. The rest of the guide works either way: ZFS gets `ashift=12` and,
for Option C, LUKS gets `--sector-size 4096`, so both use 4 KiB blocks on either format. On a 4 KiB namespace gdisk
counts in 4096-byte sectors, so its sector numbers are about eight times smaller than in the transcript below and it
reports alignment on 256-sector boundaries (1 MiB, the same as 2048 sectors of 512 bytes); the answers you type are
the same.

**Check:**

```
root # nvme id-ns -H "${DISK}" | grep 'in use'
```

The format in use is the one you chose.

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

- The first command must show no mount point; unmount anything the live system mounted from the SSD (`umount PATH`).
- `swapoff -a` and `zpool export -a` release a swap partition or an old pool that the live system took into use;
  "no pools available to export" is not an error.
- The two loops clear signatures **inside** old partitions: ZFS keeps four labels per device and LUKS2 two headers,
  and wiping only the partition table would leave them where a new partition may find them. On an SSD without
  partitions the loops do nothing. `zpool labelclear` prints an error for a partition that holds no ZFS label; that
  is expected.
- `blkdiscard -f` discards every block of the SSD. If it fails with *Operation not supported*, continue with the
  next command.
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
| 1 | `${ESP_PART}` (`/dev/nvme0n1p1`) | 1 GiB | `EF00` EFI system partition | `EFI` | the ESP (1) |
| 2, Options A and B | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `BF00` Solaris root | `zroot` | the ZFS pool |
| 2, Option C | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `8309` Linux LUKS | `zroot-luks` | LUKS2 container (2) |

1. FAT32, mounted at `/boot/efi`; it holds ZFSBootMenu.
2. The container holds the pool.

- The ESP holds only ZFSBootMenu: the image, its backup, and a copy at `\EFI\BOOT\BOOTX64.EFI`. Each image holds a
  kernel and drivers, so 1 GiB leaves room for all of them. No Ubuntu kernel or initramfs is ever stored on the ESP.
- ZFS ignores the partition type; `BF00` and `8309` are labels for people and tools. The names give the stable paths
  `/dev/disk/by-partlabel/EFI` and `/dev/disk/by-partlabel/zroot-luks`, which the [Usage](#usage) section uses.
- gdisk aligns the start of each partition to 1 MiB, and since version 1.0.9 also the **default** end. Pressing Enter
  at the second partition's `Last sector` prompt therefore gives a size that is a whole number of MiB, which LUKS2
  with 4 KiB sectors needs (Option C).

The keys to type:

| Step | Command | Answers |
|---|---|---|
| New, empty GPT | `o` | `y` |
| Partition 1, the ESP | `n` | `<Enter>`, `<Enter>`, `+1G`, `ef00` |
| Partition 2, the rest | `n` | `<Enter>`, `<Enter>`, `<Enter>`, then `bf00` (Options A, B) or `8309` (Option C) |
| Name partition 1 | `c` | `1`, `EFI` |
| Name partition 2 | `c` | `2`, then `zroot` (Options A, B) or `zroot-luks` (Option C) |
| Print and compare with the table above | `p` | |
| Write and leave | `w` | `y` |

The transcript of a session for Options A and B, on a 1 TB SSD with 512-byte sectors. What you type follows each
prompt; never type the sector numbers shown, press Enter for the defaults. gdisk changes nothing on the SSD until `w`;
`q` leaves without writing.

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

and names it `zroot-luks` instead of `zroot`:

```
Command (? for help): c
Partition number (1-2): 2
Enter name: zroot-luks
```

Its `p` listing ends with `8309  zroot-luks`.

**All options.** What you may see that the transcript does not show:

- The opening lines differ when an old table is still found (`MBR: protective`, `GPT: present` or `GPT: damaged`);
  `o` replaces whatever is there.
- The version line and all sector numbers differ from SSD to SSD.
- If gdisk ends with "Warning: The kernel is still using the old partition table", run `partprobe "${DISK}"`, or
  reboot the live system and follow
  [Continuing after a reboot of the live system](#continuing-after-a-reboot-of-the-live-system).
- A mistake before `w` costs nothing: `q` quits without saving, `d` deletes a partition.

Make the kernel see the new partitions and format the ESP:

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
- The `echo` prints `0`: the size of partition 2 is a whole number of MiB. If it prints another number on an SSD
  with 512-byte sectors, gdisk is older than 1.0.9. Delete partition 2 (`d`, `2`) and create it again, answering
  the `Last sector` prompt with the number that `echo $(( (LAST + 1) / 2048 * 2048 - 1 ))` prints, where `LAST` is
  the larger number in the prompt's range. On a 4 KiB namespace every size is a multiple of 4096 bytes, which is all
  Option C needs, so no fix is needed there. For Options A and B a non-zero result does no harm.
- `wipefs` prints nothing: partition 2 carries no old signature. If it prints a line, run `wipefs -a "${POOL_PART}"`.

### Option C only: creating the LUKS container

Skip this section for Options A and B.

Why: the container encrypts everything in partition 2; the pool is created inside it.

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it
> before any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

```
root # cryptsetup luksFormat --type luks2 --sector-size 4096 "${POOL_PART}"
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open --allow-discards --persistent "${POOL_PART}" "luks-${LUKS_UUID}"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
```

- `luksFormat` asks you to type `YES` in capitals, then for the passphrase twice. The other settings are
  cryptsetup's LUKS2 defaults; the passphrase slot uses Argon2id.
- `--sector-size 4096`: 4 KiB encryption sectors, matching `ashift=12` and the SSD. If `luksFormat` prints "Device
  size is not aligned to requested sector size.", go back to the Check of
  [Partitioning with gdisk](#partitioning-with-gdisk).
- The container is opened as `luks-<UUID>`, the name systemd and dracut use by default and the name ZFSBootMenu gives
  the volume. The same name everywhere keeps the path recorded in the pool the same in ZFSBootMenu and in Ubuntu.
- `--allow-discards --persistent` stores the TRIM flag **in the LUKS2 header**, so the volume passes TRIM requests
  through wherever it is opened (needed for `autotrim=on`): in ZFSBootMenu, in Ubuntu's initramfs and on a live
  system, without any crypttab option.
- Optional: adding `--perf-no_read_workqueue --perf-no_write_workqueue` to the `cryptsetup open` line processes
  encryption directly instead of in kernel work queues, which lowers latency on fast NVMe SSDs. These flags are
  stored in the header as well.

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
`sector size: 4096` and `flags: discards` (plus `no-read-workqueue no-write-workqueue` if you added them).
`POOL_DEV` is `/dev/mapper/luks-` followed by the UUID.

After a reboot of the live system, open the container again as part of
[Continuing after a reboot of the live system](#continuing-after-a-reboot-of-the-live-system); the stored flags are
applied by themselves:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

### Creating the ZFS pool

Why: the pool holds the whole system. Three parties import it, ZFSBootMenu, Ubuntu's initramfs and the running
system, and ZFS refuses a normal import by a host whose host ID differs from the one recorded in the pool. So the host
ID is fixed first, and the same value is used everywhere.

**All options.**

```
root # zgenhostid -f 0x00bab10c
```

**Option B.** Write the passphrase file that `zpool create` reads and that later lets Ubuntu's initramfs load the key
without asking.

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it
> before any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

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

- You type the passphrase once more when the pool is opened again in
  [Mounting the pool below /mnt](#mounting-the-pool-below-mnt), which catches a typing mistake before anything is
  installed.
- The file must stay at `/etc/zfs/zroot.key` on the **live system** until the installation is finished; a copy goes
  into the new system later.

**Options A and B.** The pool goes directly onto partition 2 (Option C set `POOL_DEV` to the opened container):

```
root # export POOL_DEV="${POOL_PART}"
```

**Options A and C.**

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

**Option B** (three more `-O` lines):

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
| `compatibility=openzfs-2.2-linux` | enables only the pool features of OpenZFS 2.2 (1) |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | what the pool mounts during the installation appears below `/mnt` (2) |
| `encryption=aes-256-gcm` (B) | the cipher; likely the fastest on modern x86_64 hardware |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key; ZFSBootMenu cannot ask for any other key format |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file just written (3) |

1. A live system or rescue medium with ZFS 2.2 or later can then still import the pool. The set can be relaxed
   later ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).
2. Never over the live system's own directories.
3. It will also live inside Ubuntu's initramfs, which is stored **on** the encrypted pool.

In Option B the encryption root is the pool's root dataset, so every dataset created later inherits the encryption,
including the boot environment with its `/boot`.

`ls /usr/share/zfs/compatibility.d/` lists the compatibility sets the installed ZFS knows; Ubuntu 26.04's ZFS 2.4.1
knows `openzfs-2.2-linux`.

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
- Option B also: `ls -l /etc/zfs/zroot.key` shows `----------` owned by root, and `wc -l < /etc/zfs/zroot.key`
  prints `1`.

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
| `zroot/ROOT/ubuntu` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, including `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/ubuntu` | the boot environment ZFSBootMenu boots after the countdown |

- `canmount=noauto` is required on every file system with `mountpoint=/`, and it is not inherited: each boot
  environment created later needs it set again.
- Keep the whole system in one boot environment: do not add separate datasets for `/var` or `/usr`, whose contents
  belong together with the package database in `/var/lib/dpkg`. More datasets for user data (`/srv`, for example) are
  your choice.

> **Important**
> `/boot` is a directory of the root dataset `zroot/ROOT/ubuntu`, not a file system of its own and not the ESP.
> ZFSBootMenu reads Ubuntu's kernel and initramfs from there, and in Options B and C the initramfs with the key file
> is then inside the encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/ubuntu`.

### Setting the ZFSBootMenu properties

Why: ZFSBootMenu passes the value of `org.zfsbootmenu:commandline` to Ubuntu's kernel. It is set on `zroot/ROOT`, so
every future boot environment inherits it.

**Options A and B.**

```
root # zfs set org.zfsbootmenu:commandline="quiet" "${POOL}/ROOT"
```

**Option B** also names the file system in which ZFSBootMenu may look for the key file:

```
root # zfs set org.zfsbootmenu:keysource="${POOL}/ROOT/${BE}" "${POOL}"
```

This is a convenience, not a requirement. When you choose a boot environment **in the menu**, ZFSBootMenu mounts the
named file system after the passphrase was typed, finds `etc/zfs/zroot.key` in it and keeps a copy in memory until it
boots, so that menu operations which export and import the pool again (setting the default boot environment, cloning
a snapshot, switching to read/write) do not ask again. It does not remove the first prompt.

**Option C.** Ubuntu's initramfs unlocks the container again with three parameters for systemd-cryptsetup (the line
is built in two steps to keep it short):

```
root # KCL="quiet luks.crypttab=no luks.uuid=${LUKS_UUID}"
root # KCL="${KCL} luks.key=${LUKS_UUID}=/etc/cryptsetup-keys.d/zroot.key"
root # zfs set org.zfsbootmenu:commandline="${KCL}" "${POOL}/ROOT"
```

| Parameter | Effect |
|---|---|
| `luks.crypttab=no` | ignore `/etc/crypttab` completely |
| `luks.uuid=<UUID>` | unlock the container with this LUKS UUID, as `luks-<UUID>` |
| `luks.key=<UUID>=<path>` | use this key file for it |

systemd reads these parameters in the initramfs **and** on the running system. That suits a single-SSD installation
without a crypttab. If you later add other LUKS volumes to `/etc/crypttab` (a data disk, for example), switch to the
forms only the initramfs reads: `rd.luks.crypttab=no rd.luks.uuid=... rd.luks.key=...`.

**All options.** Never put `root=`, `spl_hostid=`, `spl.spl_hostid=`, `BOOT_IMAGE=`, `initrd=` or `zbm.*` options
into this property: ZFSBootMenu sets the root argument and the host ID itself, and `zbm.*` options belong to
ZFSBootMenu's own command line ([Configuring generate-zbm](#configuring-generate-zbm)). For Ubuntu, ZFSBootMenu adds
`root=zfs:zroot/ROOT/ubuntu`, which OpenZFS's dracut module reads; the property `org.zfsbootmenu:rootprefix` is not
needed.

**Check:**

```
root # zfs get -H -o value org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"
root # zfs get -H -o value org.zfsbootmenu:keysource "${POOL}"
```

The command line is the value just set; for Option C it contains the LUKS UUID twice. The key source is
`zroot/ROOT/ubuntu` for Option B and `-` for Options A and C.

### Mounting the pool below /mnt

Why: an export and a fresh import prove that the pool can be opened again, and mount the datasets in the right order:
the root first.

```
root # zpool export "${POOL}"
root # zpool import -N -R /mnt "${POOL}"
```

**Option B** loads the key by typing the passphrase:

```
root # zfs load-key -L prompt "${POOL}"
```

If it refuses the passphrase, the file holds something else than you believe (`cat /etc/zfs/zroot.key` shows it).
Nothing is installed yet: load the key from the file (`zfs load-key "${POOL}"`), destroy the pool
(`zpool destroy "${POOL}"`), write the file again and repeat from [Creating the ZFS pool](#creating-the-zfs-pool).

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

- `-N` imports without mounting; the root dataset is mounted by hand because it is `canmount=noauto`.
- `cachefile` writes the pool's configuration to the live system's `/etc/zfs/zpool.cache`, which is copied into the
  new system later for its `zfs-import-cache` service and its initramfs.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot.
> The ZFS tools inside the new system may not match the ZFS kernel module that the live system has loaded.

**Check:**

```
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`/mnt` is `zroot/ROOT/ubuntu` and `/mnt/home` is `zroot/home`. Option B: the key status is `available` (Options A and
C print `-`).

## Installing the base system

### Running debootstrap

Why: `debootstrap` installs a minimal Ubuntu into `/mnt`, the boot environment, without the desktop installer, which
cannot install onto this layout.

```
root # debootstrap resolute /mnt http://archive.ubuntu.com/ubuntu
```

- It downloads and unpacks the packages of priority *required* and *important* from the `main` component; this takes
  a few minutes. More components are added in [Configuring APT](#configuring-apt).
- In 26.04's archive, `systemd-resolved`, `systemd-cryptsetup`, `netplan.io`, `locales`, `tzdata`, `console-setup`,
  `keyboard-configuration` and `sudo` have priority *important*, so the base system has them. It has no kernel, no
  initramfs generator and no boot loader.

**Check:**

```
root # grep -E '^VERSION_CODENAME=' /mnt/etc/os-release
root # ls /mnt/boot
root # chroot /mnt dpkg-query -W -f='${Status} ${Package}\n' systemd-resolved systemd-cryptsetup netplan.io \
  locales tzdata console-setup keyboard-configuration sudo
```

The code name is `resolute`, and `/mnt/boot` is empty: there is no kernel yet. `dpkg-query` prints eight lines that
start with `install ok installed`. If a package is missing or not installed, install it in the chroot after
[Configuring APT](#configuring-apt) with `apt install --no-install-recommends PACKAGE`.

### Copying the host ID and the key file

Why: the new system must use the same host ID as the pool, the same pool cache, and in Option B the passphrase file
named by `keylocation`. The ESP is mounted where the new system expects it.

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

**All options.** Carry the variables into the chroot with a file (it holds no secret;
[Finalizing the installation](#finalizing-the-installation) deletes it):

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

For Options A and B the `LUKS_UUID` line is empty; that is harmless.

The host ID must be in place **before** `zfsutils-linux` is installed in the new system: the package writes a random
host ID when `/etc/hostid` is missing.

**Check:**

```
root # findmnt /mnt/boot/efi
root # od -A n -t x4 /mnt/etc/hostid
root # ls -l /mnt/etc/zfs/
root # cat /mnt/root/install.env
```

The ESP is mounted, the host ID prints `00bab10c`, and `/mnt/etc/zfs` holds `zpool.cache` and, for Option B,
`zroot.key` with mode `----------`. The file shows the values set earlier.

### Chrooting

Why: the remaining steps run inside the new system, with the live system's kernel, devices and network.

```
root # mount -t proc proc /mnt/proc
root # mount --rbind /sys /mnt/sys
root # mount --make-rslave /mnt/sys
root # mount --rbind /dev /mnt/dev
root # mount --make-rslave /mnt/dev
root # mount -t tmpfs -o mode=0755 tmpfs /mnt/run
root # mkdir -p /mnt/run/lock /mnt/run/systemd/resolve
root # cp -L /etc/resolv.conf /mnt/run/systemd/resolve/stub-resolv.conf
root # chroot /mnt /bin/bash --login
(chroot) root # source /root/install.env
(chroot) root # ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
```

- `--rbind /sys` also brings `/sys/firmware/efi/efivars`, which `efibootmgr` needs later; `--make-rslave` keeps
  unmounting inside `/mnt` from affecting the live system.
- `/run` is an empty memory file system, not the live system's `/run`. That keeps package scripts in the chroot from
  starting or stopping services of the live system.
- The base system has `systemd-resolved`, whose `/etc/resolv.conf` is a link to
  `/run/systemd/resolve/stub-resolv.conf`; the `ln` line makes sure it is. The copy of the live system's resolver
  configuration under that path lets programs in the chroot resolve names. The installed system uses the same link
  with its own `systemd-resolved`.

If you leave the chroot (`exit`) and come back, run the `chroot` line, `source /root/install.env` again, and set
`KVER` again as shown in [Kernel and ZFS](#kernel-and-zfs).

**Check:**

```
(chroot) root # getent hosts archive.ubuntu.com
(chroot) root # findmnt /sys/firmware/efi/efivars
(chroot) root # echo "${POOL} ${BE} ${DISK}"
```

`getent` prints an address, `findmnt` shows an `efivarfs` mount, and the variables are set (`zroot ubuntu
/dev/nvme0n1`).

### Configuring APT

Why: `debootstrap` enables only `main` and the release itself. The updates, the security updates and the `universe`
component are needed; `universe` holds `systemd-boot-efi`, `fzf` and `mbuffer`, which ZFSBootMenu uses.

```
(chroot) root # rm -f /etc/apt/sources.list
(chroot) root # cat > /etc/apt/sources.list.d/ubuntu.sources <<'EOF'
Types: deb
URIs: http://archive.ubuntu.com/ubuntu/
Suites: resolute resolute-updates resolute-backports
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg

Types: deb
URIs: http://security.ubuntu.com/ubuntu/
Suites: resolute-security
Components: main restricted universe multiverse
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
EOF
(chroot) root # apt update
(chroot) root # apt full-upgrade
```

The file uses the deb822 format that Ubuntu has used since 24.04; use another mirror in `URIs` if you prefer.
`debootstrap` writes `/etc/apt/sources.list` with the release and `main` only; it is removed so that no source is
listed twice.

**Check:**

```
(chroot) root # apt-cache policy systemd-boot-efi fzf | grep -E '^[a-z]|Candidate'
(chroot) root # apt-cache policy | grep -c resolute-security
```

Both packages have a candidate version (not `(none)`), and the second command prints a number greater than `0`.

## Kernel and ZFS

Why: Ubuntu's kernel packages include a ZFS module built for each kernel, so no DKMS build and no kernel headers are
needed. dracut, OpenZFS's dracut module and the initramfs settings come first, so that the kernel package builds its
initramfs with them: the kernel package fails to install when its initramfs cannot be built.

```
(chroot) root # apt install --no-install-recommends dracut zfsutils-linux zfs-dracut
```

Then write the initramfs settings that all options share. Options B and C add their key file to this file in
[Configuring the initramfs](#configuring-the-initramfs).

**FILE** `/etc/dracut.conf.d/10-zfs.conf`

```
# A generic image: it is built in a chroot, where dracut's host-only detection sees the live system
hostonly="no"
# Do not store kernel parameters detected in the chroot; ZFSBootMenu passes them
hostonly_cmdline="no"
nofsck="yes"
add_dracutmodules+=" systemd zfs "
add_drivers+=" nvme "
```

Then the kernel:

```
(chroot) root # apt install --no-install-recommends linux-image-generic
```

**Option C** also installs cryptsetup, which the key file of the next section needs, and `systemd-cryptsetup`, the
program that unlocks the container in Ubuntu's initramfs. The base system and `zfs-dracut` already bring
`systemd-cryptsetup`; naming it makes sure it is there.

```
(chroot) root # apt install --no-install-recommends cryptsetup systemd-cryptsetup
```

**All options.** Optional: `apt install zfs-zed` adds OpenZFS's event daemon, which logs pool events and can send
notices of checksum and device errors (`/etc/zfs/zed.d/zed.rc`); `--no-install-recommends` above leaves it out.

Enable the ZFS services and remember the kernel version:

```
(chroot) root # systemctl enable zfs.target zfs-import-cache.service zfs-mount.service zfs-import.target
(chroot) root # export KVER="$(linux-version list | linux-version sort | tail -n 1)"
```

| Package | What it brings |
|---|---|
| `zfsutils-linux` | `zfs`, `zpool`, `zgenhostid`, `mount.zfs`, the udev rules and the systemd units |
| `zfs-dracut` | OpenZFS's dracut module `zfs`; it depends on `dracut` and `systemd-cryptsetup` |
| `dracut` | Ubuntu's initramfs generator (1) |
| `linux-image-generic` | the kernel, its modules and the ZFS module (2) |
| `cryptsetup` (C) | `cryptsetup` and `dmsetup` (3) |
| `systemd-cryptsetup` (C) | `systemd-cryptsetup` and its generator, for the `luks.*` kernel parameters |

1. With its kernel hook `/etc/kernel/postinst.d/dracut` and an `update-initramfs` command that runs dracut.
2. The ZFS module is the package `linux-main-modules-zfs-<version>`; `linux-firmware`, `intel-microcode` and
   `amd64-microcode` come with it.
3. For the key file now, and for LUKS support in ZFSBootMenu's image later.

- `--no-install-recommends` keeps out what the kernel package recommends, among it GRUB. ZFSBootMenu is the only boot
  loader here.
- `linux-image-generic` is used instead of upstream's `linux-generic`, which adds the kernel headers that only DKMS
  modules need.
- Never install `zfs-initramfs`: it depends on `initramfs-tools`, which conflicts with `dracut`, and APT would remove
  dracut to install it. `cryptsetup-initramfs` is not needed either: it holds the unlock scripts for
  initramfs-tools, not for dracut.
- The kernel package installs `vmlinuz-<version>` into `/boot` of the boot environment, and its post-installation
  script runs the hooks in `/etc/kernel/postinst.d/` and `/usr/share/kernel/postinst.d/`; dracut's hook writes
  `initrd.img-<version>` beside it. None of the hooks this guide installs calls systemd's `kernel-install`, which may
  put kernels onto the ESP. In 26.04's archive the package `systemd-boot` ships such a hook (`zz-systemd-boot`): do not
  install it. The Check below shows where the files went.
- Ubuntu may also keep the links `/boot/vmlinuz` and `/boot/initrd.img` (and `.old` ones after an update) that point
  to the newest kernel files. ZFSBootMenu may list such a link as one more kernel; both entries boot the same files.

**Check:**

```
(chroot) root # dpkg -l dracut zfs-dracut initramfs-tools zfs-initramfs 2>/dev/null | grep -E '^(ii|rc)'
(chroot) root # echo "${KVER}"
(chroot) root # modinfo -k "${KVER}" -F version zfs
(chroot) root # ls -l /boot
(chroot) root # ls -A /boot/efi
(chroot) root # grep -l kernel-install /etc/kernel/postinst.d/* /usr/share/kernel/postinst.d/* 2>/dev/null
(chroot) root # systemctl is-enabled zfs.target zfs-import-cache.service zfs-mount.service zfs-import.target
(chroot) root # od -A n -t x4 /etc/hostid
```

- Only `dracut` and `zfs-dracut` are listed, with `ii`; never `initramfs-tools` or `zfs-initramfs`.
- The `grep -l` prints nothing: no kernel hook calls `kernel-install`.
- `KVER` is a version such as `7.0.0-43-generic`, and `modinfo` prints the ZFS module's version (2.4.x).
- `/boot` holds `vmlinuz-${KVER}`, `initrd.img-${KVER}`, `config-${KVER}` and `System.map-${KVER}`, possibly with the
  links named above. The initramfs is rebuilt with the right settings in
  [Configuring the initramfs](#configuring-the-initramfs).
- `/boot/efi` prints nothing: no kernel or initramfs was put on the unencrypted ESP.
- The four units print `enabled`, and the host ID is still `00bab10c`.

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Why: Ubuntu's initramfs must unlock the container again after `kexec`; a random key in a second key slot lets it do
so without asking.

```
(chroot) root # install -d -m 0700 /etc/cryptsetup-keys.d
(chroot) root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot.key
(chroot) root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  "${POOL_PART}" /etc/cryptsetup-keys.d/zroot.key
```

- `install` creates the file empty with mode `0400` before `dd` writes into it, so it is never readable by others,
  not even for a moment.
- `luksAddKey` asks for the passphrase (an existing key) and adds the file's 64 random bytes as key slot 1.
- A random key needs no slow key derivation, so `--pbkdf pbkdf2 --pbkdf-force-iterations 1000` (the smallest count
  cryptsetup accepts) makes the second unlock almost instant. The passphrase slot keeps the slow, memory-hard default
  (Argon2id).

**Check:**

```
(chroot) root # ls -l /etc/cryptsetup-keys.d/zroot.key
(chroot) root # cryptsetup open --test-passphrase --verbose \
  --key-file /etc/cryptsetup-keys.d/zroot.key "${POOL_PART}"
(chroot) root # cryptsetup luksDump "${POOL_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, cryptsetup reports `Key slot 1 unlocked.`, and two key slots (`0` and
`1`) are listed.

## Configuring the initramfs

Why: Ubuntu's initramfs must contain OpenZFS's dracut module, the NVMe driver and, for Options B and C, the key file,
and it must be readable by root only. Settings go into a file of their own in `/etc/dracut.conf.d/`, never into
`/etc/dracut.conf`.

The common settings are already in `/etc/dracut.conf.d/10-zfs.conf`, written in [Kernel and ZFS](#kernel-and-zfs).

**Option A.** Nothing to add.

**Option B.** Add the passphrase file to the file:

```
(chroot) root # cat >> /etc/dracut.conf.d/10-zfs.conf <<'EOF'
# The ZFS passphrase file named by keylocation; the initramfs lives inside the encrypted pool
install_items+=" /etc/zfs/zroot.key "
EOF
```

**Option C.** Add the unlock modules and the key file to the file:

```
(chroot) root # cat >> /etc/dracut.conf.d/10-zfs.conf <<'EOF'
# systemd-cryptsetup for the luks.* kernel parameters
add_dracutmodules+=" crypt systemd-cryptsetup "
# The key file named by luks.key=; the initramfs lives inside the encrypted pool
install_items+=" /etc/cryptsetup-keys.d/zroot.key "
EOF
```

Then make sure no crypttab line names the container; the `luks.*` parameters replace it. The file may be missing or
hold only comments:

```
(chroot) root # grep -v -E '^[[:space:]]*(#|$)' /etc/crypttab
```

It must print nothing (or `No such file or directory`). Delete any line it prints.

**All options.** Rebuild the initramfs of every installed kernel:

```
(chroot) root # update-initramfs -c -k all
```

- Ubuntu's `update-initramfs` comes with the `dracut` package and runs `dracut --force /boot/initrd.img-<version>
  <version>`. The kernel hook `/etc/kernel/postinst.d/dracut` does the same for every kernel installed later, with the
  same settings. (Run by a package's maintainer script, `update-initramfs -u` defers the work to a dpkg trigger at
  the end of the APT run.)
- Ubuntu's dracut builds host-only images by default. `hostonly="no"` builds a generic image instead, as the
  project's test system does: in the chroot, host-only detection would look at the live system's root file system
  and loaded modules. The image is larger, which costs nothing here. `add_drivers+=" nvme "` is harmless when the
  driver is built into the kernel.
- `systemd` and, for Option C, `crypt` and `systemd-cryptsetup` are named explicitly (the project's test system names
  `systemd`, `zfs` and `crypt`); a generic image would include them anyway, as long as their programs are installed.
  Ubuntu's dracut 110 has a module named `systemd-cryptsetup`; naming it makes the build fail with a message if
  `systemd-cryptsetup` is missing, instead of leaving it out.
- OpenZFS's dracut module also copies `/etc/hostid` and `/etc/zfs/zpool.cache` into the initramfs, and loads a key
  whose `keylocation` is a file with `zfs load-key`, without asking.
- dracut puts CPU microcode at the start of the same file (`early_microcode` is on by default), from the
  `intel-microcode` and `amd64-microcode` packages the kernel pulled in. ZFSBootMenu loads only this one initramfs
  file per kernel, so microcode must be inside it.
- dracut writes every image with mode `0600`, also when it replaces one: it writes a new file under `umask 077` and
  moves it over the old one.

> **Warning**
> `generate-zbm` builds ZFSBootMenu's image with its own dracut directory, but dracut still reads `/etc/dracut.conf`
> and `/usr/lib/dracut/dracut.conf.d/`. Never name a key file in either: it would end up in the ZFSBootMenu image on
> the unencrypted ESP. Keep `install_items` lines for keys in `/etc/dracut.conf.d/` only.

**Check:**

```
(chroot) root # cat /etc/dracut.conf.d/10-zfs.conf
(chroot) root # ls -l "/boot/initrd.img-${KVER}"
(chroot) root # lsinitrd "/boot/initrd.img-${KVER}" | grep -E 'zfs\.ko|zroot\.key|etc/hostid|zpool\.cache|nvme'
(chroot) root # lsinitrd "/boot/initrd.img-${KVER}" | grep -E 'systemd-cryptsetup$|cryptsetup-generator'
(chroot) root # lsinitrd -f etc/crypttab "/boot/initrd.img-${KVER}"
(chroot) root # lsinitrd "/boot/initrd.img-${KVER}" | grep -i -E 'microcode|ucode'
(chroot) root # grep CONFIG_BLK_DEV_NVME= "/boot/config-${KVER}"
(chroot) root # ls -A /boot/efi
```

| | Option A | Option B | Option C |
|---|---|---|---|
| mode of the initramfs | `-rw-------` | **`-rw-------`** | **`-rw-------`** |
| first `grep` lists | the common files (1) | the same and `etc/zfs/zroot.key` | the same as A and the key file (2) |
| second `grep` lists | anything or nothing | anything or nothing | the two programs (3) |
| `etc/crypttab` | nothing | nothing | **nothing** |

1. `zfs.ko` (possibly with `.zst`), `etc/hostid`, `etc/zfs/zpool.cache` and `nvme`.
2. `etc/cryptsetup-keys.d/zroot.key`.
3. `systemd-cryptsetup` and `systemd-cryptsetup-generator`.

- `cat` shows the shared settings and, for Options B and C, the lines added above exactly once (delete a repeated
  block if you ran a `cat >>` twice).
- The microcode `grep` lists `GenuineIntel.bin` and `AuthenticAMD.bin` in the early part of the image. If it lists
  nothing, check that `intel-microcode` and `amd64-microcode` are installed (`dpkg -l '*microcode*'`) and run
  `update-initramfs -c -k all` again.
- `CONFIG_BLK_DEV_NVME=m` means the NVMe driver is a module, which must be listed above; `=y` means it is built into
  the kernel, and then no `nvme.ko` is listed.
- `/boot/efi` prints nothing.
- If an image is readable by others (`-rw-r--r--`), something other than dracut wrote it: run
  `chmod 0600 /boot/initrd.img-*` and find out what did. For Options B and C never copy an image out of the pool.
- Option C: if the second `grep` lists nothing, run `apt install --no-install-recommends systemd-cryptsetup` and
  `update-initramfs -u -k all`, and repeat the Check.

## Configuring the system

Why: the base system has no host name, locale, time zone, users, network configuration or fstab entry for the ESP.

Host name (replace `HOSTNAME`):

```
(chroot) root # echo HOSTNAME > /etc/hostname
(chroot) root # printf '127.0.1.1\tHOSTNAME\n' >> /etc/hosts
```

Locales, time zone, keyboard and console font; each asks its questions in a dialog. Enable `en_US.UTF-8` among the
locales in any case, because some programs require it:

```
(chroot) root # dpkg-reconfigure locales tzdata keyboard-configuration console-setup
```

A root password, and a user who may use `sudo` (replace `USERNAME`):

```
(chroot) root # passwd
(chroot) root # adduser USERNAME
(chroot) root # usermod -aG sudo,adm USERNAME
```

The network: this configuration lets systemd-networkd, through netplan, configure every wired interface with DHCP. A
desktop installed later ([Updating the system](#updating-the-system)) brings NetworkManager for wireless networks.

**FILE** `/etc/netplan/01-wired.yaml`

```
network:
  version: 2
  renderer: networkd
  ethernets:
    wired:
      match:
        name: "en*"
      dhcp4: true
      dhcp6: true
```

```
(chroot) root # chmod 0600 /etc/netplan/01-wired.yaml
```

The ESP in `/etc/fstab`; ZFS mounts its datasets itself, so they get no entries. `generate-zbm` mounts the ESP by this
entry when it is not mounted.

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

Monthly scrubs and full TRIMs of the pool, with the timers that `zfsutils-linux` ships:

```
(chroot) root # systemctl enable zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

- `umask=0077` makes the ESP readable by root only; `nofail` lets the system boot if the ESP is missing.
- Ubuntu's `zfsutils-linux` also ships a cron job (`/etc/cron.d/zfsutils-linux`) that scrubs and trims every pool
  monthly, but the base system has no `cron`. If you install `cron` later (desktops and `ubuntu-standard` bring it),
  you may disable the two timers again.

**Check:**

```
(chroot) root # cat /etc/hostname /etc/fstab
(chroot) root # locale -a | grep -i en_us
(chroot) root # id USERNAME
(chroot) root # netplan generate && ls /run/systemd/network/
(chroot) root # systemctl is-enabled zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

The host name and the ESP line are shown, `en_US.utf8` is listed, the user is in the groups `sudo` and `adm`,
`netplan generate` prints nothing and writes a `.network` file for `wired`, and both timers are `enabled`.

## Installing ZFSBootMenu

Why: ZFSBootMenu is built from this repository on the new system, so its image contains Ubuntu's kernel, ZFS and
cryptsetup. Everything in this section runs **inside the chroot**.

> **Warning**
> The repository's default branch, `master`, holds the upstream layout without the LUKS code. Always use
> `claude/zbm-refactor`. Do not run `make core dracut`, which older guides show: it installs neither `zbm` nor
> `generate-zbm`. Do not use Ubuntu packages or prebuilt images of upstream ZFSBootMenu either: they are not this
> build and cannot unlock LUKS.

### Dependencies

Why: the build needs a Rust compiler, and the image is assembled from programs of the build host.

```
(chroot) root # apt install --no-install-recommends git make cargo rustc binutils ca-certificates \
  dracut-core systemd-boot-efi kexec-tools fzf mbuffer bsdextrautils ncurses-bin procps kmod udev zstd \
  efibootmgr dosfstools util-linux less xkb-data libx11-data
```

When `kexec-tools` asks whether it should handle reboots, answer **No**.

| Program | Package | Needed for |
|---|---|---|
| `cargo`, `rustc` 1.88 or later | `cargo`, `rustc` (1.93 on 26.04) | building the Rust programs (1) |
| `make`, `git` | `make`, `git` | building; fetching the source (with `ca-certificates` for HTTPS) |
| `strip`, `strings`, `objcopy` | `binutils` | stripping, kernel versions, the image Check (2) |
| `dracut`, `lsinitrd` | `dracut-core` (already there through `dracut`) | building the image |
| the EFI stub `/usr/lib/systemd/boot/efi/linuxx64.efi.stub` | `systemd-boot-efi` (`universe`) | the EFI bundle |
| `kexec` | `kexec-tools` | starting Ubuntu's kernel; required for either runtime |
| `fzf` | `fzf` (`universe`) | the menu of the default (shell) runtime |
| `mbuffer`, `column`, `less` | `mbuffer` (`universe`), `bsdextrautils`, `less` | optional (3) |
| `tput`, `ps`, `insmod`, `modinfo`, udev, `lsblk`, `blkid` | see (4) | the image; the base system has most of them |
| `zstd` | `zstd` | compression of the image |
| `efibootmgr` | `efibootmgr` | the firmware boot entries |
| XKB data and compose rules | `xkb-data`, `libx11-data` | keyboard layouts of the Rust runtime; optional |
| `cryptsetup`, `dmsetup` | `cryptsetup` (5) | **Option C**: LUKS support in the image |

1. `zbm`, `brush`, `generate-zbm` and `zbm-kcl`.
2. `make install` strips the programs; `generate-zbm` reads kernel versions; `objcopy` is used in the Check of the
   image.
3. Images are built without them, with a warning.
4. `ncurses-bin`, `procps`, `kmod`, `udev` and `util-linux`.
5. Installed in [Kernel and ZFS](#kernel-and-zfs).

- This list follows the build container's (`packaging/container/setup-image.sh`), which builds the project's test
  images on Ubuntu, without its Perl modules and hardware-key tools: the Rust `generate-zbm` needs no Perl.
- `systemd-boot-efi` holds only the EFI files. Do not install `systemd-boot`: it installs kernels onto the ESP.
- **Option C**: LUKS support goes into the image only when `cryptsetup` is installed on the build host; a dracut build
  without it leaves LUKS out silently. Options A and B do not need it.

**Check:**

```
(chroot) root # cargo --version && rustc --version
(chroot) root # ls -l /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # command -v dracut lsinitrd kexec fzf strip strings objcopy efibootmgr
(chroot) root # command -v cryptsetup
(chroot) root # dpkg -l initramfs-tools systemd-boot 2>/dev/null | grep '^ii'
```

`rustc` is 1.88 or later, the stub exists, `command -v` prints eight paths, `cryptsetup` prints a path (Option C; for
Options A and B it may print nothing), and the last command prints nothing.

### Building and installing

Why: `make` builds the Rust programs; `make install` puts them and the dracut modules where `generate-zbm` looks for
them.

```
(chroot) root # git clone --depth 1 --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2 /usr/src/zbm
(chroot) root # make -C /usr/src/zbm
(chroot) root # make -C /usr/src/zbm install
```

- `--depth 1` fetches only the newest commit, which is all the build needs.
- `make` runs `cargo build --release --locked` for `zbm`, `brush`, `generate-zbm` and `zbm-kcl`. The crates are
  vendored in the repository, so the build needs no network; it takes several minutes.
- `make install` builds again as the user who runs it (root, with Ubuntu's `cargo` in `/usr/bin`) and puts:
  - `generate-zbm` and `zbm-kcl` in `/usr/bin`, and `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
  - the shell runtime in `/usr/share/zfsbootmenu`;
  - the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in `/usr/lib/dracut/modules.d`, and the
    mkinitcpio hooks in `/usr/lib/initcpio` (unused on Ubuntu);
  - `config.yaml`, `mkinitcpio.conf` and `dracut.conf.d/` in `/etc/zfsbootmenu`;
  - the manual pages, and examples in `/usr/share/examples/zfsbootmenu`.
- `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files it
  installs in `/etc/zfsbootmenu/dracut.conf.d/`. That is why this guide writes `config.yaml` after it, and keeps its
  own dracut settings in files with other names.
- The module `zfsbootmenu-measured` is also included in Ubuntu's own initramfs from now on. It only copies the TPM
  event log that images with the Rust runtime pass on, and does nothing otherwise.

> **Note**
> Ubuntu 26.04 uses the Rust implementation of the core utilities (uutils) by default, and `make install` calls
> `install` and `realpath` with GNU options. This guide could not confirm that every one of them works with uutils;
> the project's build container, also Ubuntu with uutils, does not run `make install`. If `make install` stops with
> an error from `install` or `realpath`, run it with the GNU programs, which Ubuntu installs as `gnuinstall` and
> `gnurealpath`:
>
> ```
> (chroot) root # mkdir -p /root/gnu-bin
> (chroot) root # ln -sf /usr/bin/gnuinstall /root/gnu-bin/install
> (chroot) root # ln -sf /usr/bin/gnurealpath /root/gnu-bin/realpath
> (chroot) root # PATH="/root/gnu-bin:${PATH}" make -C /usr/src/zbm install
> ```

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

Why: `generate-zbm` does nothing until it is told where to put which image and what ZFSBootMenu's own kernel command
line is.

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

**Option C.** The same file with another last line; the shell fills in the UUID, so `<<EOF` must not be quoted:

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
  CommandLine: "quiet loglevel=0 zbm.luks=UUID=${LUKS_UUID}"
EOF
```

**All options.** Build an image that does not depend on the machine it is built on:

**FILE** `/etc/zfsbootmenu/dracut.conf.d/generic.conf`

```
# Build an image that does not depend on the machine it was built on: the first image is built
# inside a chroot on a live system, whose kernel and loaded drivers are not those of the installed system.
hostonly="no"
```

| Key | Value | Meaning |
|---|---|---|
| `Global.ManageImages` | `true` | `generate-zbm` does nothing while this is `false`, the installed default |
| `Global.BootMountPoint` | `/boot/efi` | the ESP (1) |
| `Global.DracutConfDir` | `/etc/zfsbootmenu/dracut.conf.d` | dracut settings of **ZFSBootMenu's** image (2) |
| `Global.PreHooksDir`, `Global.PostHooksDir` | as shown | programs run before and after each build |
| `Global.InitCPIOConfig` | as shown | used only with mkinitcpio; kept from the installed file |
| `Components.Enabled` | `false` | no separate kernel and initramfs; only the EFI bundle |
| `EFI.Enabled` | `true` | build the EFI bundle: kernel, initramfs and command line in one file |
| `EFI.ImageDir` | `/boot/efi/EFI/zbm` | where the bundle is written |
| `EFI.Versions` | `false` | one image and one `-backup` copy with fixed names (3) |
| `Kernel.Prefix` | `vmlinuz` | fixes the names to `vmlinuz.EFI` and `vmlinuz-backup.EFI` (4) |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's** kernel, built into the bundle |

1. `generate-zbm` mounts it if needed (from fstab) and unmounts it again.
2. Never the system's `/etc/dracut.conf.d`.
3. The firmware entries then never change. Write `false`, not `true`.
4. Ubuntu's `vmlinuz-<version>` would give the same names; the prefix keeps them if that ever changes.

| Option | `Kernel.CommandLine` |
|---|---|
| A, B | `quiet loglevel=0 zbm.luks=off` |
| C | `quiet loglevel=0 zbm.luks=UUID=<LUKS UUID>` |

- `zbm.luks=off` (A, B) stops ZFSBootMenu from offering to unlock LUKS volumes it finds, for example on a USB disk
  plugged in at boot.
- `zbm.luks=UUID=` (C) unlocks only this volume. The UUID is that of the **LUKS container** (`cryptsetup luksUUID`),
  not the pool's GUID or a file system UUID. It is opened as `luks-<UUID>`.

- `generate-zbm` uses the kernel with the highest version in `/boot`, and needs the ZFS module for it; with Ubuntu's
  kernel packages the module is always installed with the kernel.
- No `/etc/zfsbootmenu/crypttab` is created: `zbm.luks=` is enough.
- Options you may add to `Kernel.CommandLine` later: `zbm.timeout=SECONDS` (the countdown, 10 by default; `-1` always
  shows the menu), `loglevel=7` for troubleshooting.
- `hostonly="no"`: the image needs the NVMe driver, the keyboard drivers and, for Option C, dm-crypt, for a kernel
  that is not the one running during the build. The build container that builds the project's test images writes the
  same setting. The file's name is not one `make install` overwrites.

**Check:**

```
(chroot) root # cat /etc/zfsbootmenu/config.yaml
(chroot) root # ls /etc/zfsbootmenu/dracut.conf.d/
(chroot) root # cat /etc/zfsbootmenu/dracut.conf.d/generic.conf
```

`config.yaml` shows `ManageImages: true` and, for Option C, a real UUID after `zbm.luks=UUID=` (not an empty value).
`ls` lists `generic.conf omit-drivers.conf zfsbootmenu.conf`, and `generic.conf` contains `hostonly="no"`.

### Choosing the runtime

The image runs one of two runtimes. The **shell runtime** (`zfsbootmenu_runtime=legacy`), written in Bash with the
`fzf` menu, is the default and is enough for all three options. The **Rust runtime** (`zfsbootmenu_runtime=rust`), the
program `zbm` with a new menu, is opt-in and will replace the shell runtime after a beta. Both unlock LUKS volumes and
ask for ZFS passphrases. Only the Rust runtime has, among others, the locked menu, measured boot, and keyboard
layouts other than US at the passphrase prompt. The project's VM suite runs with the Rust runtime by default.

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
same place on your layout, or use the Rust runtime. With the Rust runtime, press **Ctrl-K** at the prompt to choose a
layout, or add `zbm.console=drm` to `Kernel.CommandLine` so that the console types with the image's first layout. That
console needs the kernel's `evdev` module and a DRM driver in the image (`simpledrm` on the firmware's framebuffer is
enough), for example with `add_drivers+=" evdev "` in a file of its own in `/etc/zfsbootmenu/dracut.conf.d/`; without
them ZFSBootMenu stays on the kernel console, with the US layout. By default the image gets the layout named by
`XKBLAYOUT` in `/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`. Ubuntu keeps its keyboard layout in
`/etc/default/keyboard` instead, and normally has no `/etc/vconsole.conf`; to put your layout first, name it in the same
file, for example `echo 'zfsbootmenu_keyboard_layouts="de us"' >> /etc/zfsbootmenu/dracut.conf.d/runtime.conf`.

**Check:** `cat /etc/zfsbootmenu/dracut.conf.d/*.conf` shows `zfsbootmenu_runtime=rust` only if you chose it.

### Building and checking the image

Why: the first image is built and checked before anything depends on it. Building twice creates the backup image
that the backup firmware entry boots.

```
(chroot) root # generate-zbm --dry-run --debug
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

- `--dry-run` prints one `Would ...` line for each change and makes none; `--debug` prints every command. It names
  the kernel it would use: `/boot/vmlinuz-${KVER}`.
- The first run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`; the second renames it to `vmlinuz-backup.EFI` and writes a
  new one.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | no `make install` (1) |
| `failed to install essential executable '<name>'` | a required program is missing (2) |
| `Unable to find UEFI stub loader at default locations` | `systemd-boot-efi` is not installed |
| `ManageImages not enabled, no action taken` | `config.yaml` was overwritten by `make install`; write it again |
| `Unable to locate libgcc_s.so` | install `libgcc-s1`, or set `zfsbootmenu_skip_gcc_s=yes` |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (3) |

1. `make install` was not run, or `make core dracut` was.
2. Often `kexec`, or `fzf` for the shell runtime.
3. The SPL module's `spl_hostid` parameter differs from `/etc/hostid` (the check is skipped while the parameter is
   0). The image takes `/etc/hostid`, which must be `00bab10c` (`od -A n -t x4 /etc/hostid`).

**Check** the image, without a reboot:

```
(chroot) root # ls -l /boot/efi/EFI/zbm/
(chroot) root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
(chroot) root # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/cryptsetup$|etc/hostid|zfs\.ko|nvme'
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'cryptsetup-keys\.d|\.key$|etc/zfsbootmenu/crypttab'
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/(head|cat|sort|cut|tr)( |$)'
(chroot) root # rm /run/zbm-initrd.img
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `ls` | `vmlinuz.EFI` and `vmlinuz-backup.EFI` | the same | the same |
| `zbm-kcl` prints | `quiet loglevel=0 zbm.luks=off` | the same | `quiet loglevel=0 zbm.luks=UUID=` and the LUKS UUID |
| first `grep` lists | `etc/hostid`, `zfs.ko`, `nvme` | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

- `ls -l` also shows the size of the images; the 1 GiB ESP holds many of them.
- `nvme` is listed as `nvme.ko` where the kernel builds the driver as a module, and is missing where it is built in
  (`CONFIG_BLK_DEV_NVME=y`, see [Configuring the initramfs](#configuring-the-initramfs)).
- The second `grep` must print nothing: no key file and no crypttab are in the image. If it prints a line, do not
  boot this image; find out why first. For Option B the usual cause is a key named in `/etc/dracut.conf` or in
  `/usr/lib/dracut/dracut.conf.d/`.
- If `cryptsetup` is listed for Options A or B, that is fine: it is in the image whenever the program is installed.
- The third `grep` prints a line for each of the five commands `head`, `cat`, `sort`, `cut` and `tr`. The image is
  built from Ubuntu's uutils core utilities, whose layout the project's tests have not built from (see
  [What the project has tested](#what-the-project-has-tested)); a missing command breaks ZFSBootMenu's menu
  operations. If one is missing, do not rely on the image: report it, and keep the live medium at hand.

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

Why: the pool must be exported cleanly, so that ZFSBootMenu and Ubuntu's initramfs import it without complaint; a
snapshot keeps the freshly installed state.

```
(chroot) root # exit
root # rm /mnt/root/install.env
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

**All options.**

- The snapshot `@install` is the first state to roll back to ([Boot environments, snapshots and
  rollback](#boot-environments-snapshots-and-rollback)).
- The export leaves the pool in the state every later import expects.

**Check:**

```
root # zpool list
root # ls /dev/mapper
```

`zpool list` reports `no pools available`. Option C: `/dev/mapper` contains only `control`. If the export fails
because the pool is busy, leave any shell that is still inside `/mnt` and repeat the unmount and export commands.

Then reboot and remove the USB stick:

```
root # reboot
```

## First boot

With the **default (shell) runtime** you see:

| Step | Option A | Option B | Option C |
|---|---|---|---|
| 1 | | | `Unlock LUKS volume <device>` and `Passphrase (leave empty to skip):`. Type the LUKS passphrase |
| 2 | `Booting zroot/ROOT/ubuntu in 10 seconds` (1) | the same | the same |
| 3 | | the screen clears; `Enter passphrase for 'zroot':`. Type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Ubuntu's kernel | the same | the same |
| 5 | Ubuntu starts and shows a text login | the same, **without** asking again | the same, **without** asking again |

1. With `[RETURN] boot now` and `[ESCAPE] boot menu`.

- **Enter** boots at once; **Escape** during the countdown opens the menu, which lists `zroot/ROOT/ubuntu`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: `zfs load-key` allows three attempts. After three wrong passphrases the boot stops in ZFSBootMenu's
  emergency shell with `unable to load key for zroot/ROOT/ubuntu`. Type `exit`: ZFSBootMenu goes on to its menu,
  which asks for the passphrase again.
- With the **Rust runtime** the same questions appear as dialogs of the menu's start-up view: `Unlock <volume>` for
  Option C, a ZFS key dialog for Option B that allows three attempts, and the banner
  `Booting zroot/ROOT/ubuntu in <N> s. Press any key to choose another.`

If Ubuntu asks for a passphrase a second time, or stops in its initramfs, see
[The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock].

Log in as root. From here on, `root #` means root on the **installed system**.

**Check:**

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status zroot
root # hostid
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
root # systemctl --failed
root # networkctl list
```

**Option B**:

```
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/ubuntu
```

Both datasets show `aes-256-gcm`, the encryption root `zroot` and the key status `available`. Only the encryption
root has a key location: `zroot` shows `file:///etc/zfs/zroot.key`, and `zroot/ROOT/ubuntu` shows `none`.

**Option C**:

```
root # LUKS_UUID="$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)"
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `/proc/cmdline` | the common arguments (1) | the same | the same, with the `luks.*` arguments (2) |
| `/` | `zroot/ROOT/ubuntu` | the same | the same |
| `zpool status` | `ONLINE`, on `nvme0n1p2` | the same | `ONLINE`, on `luks-<UUID>` |
| `hostid` | `00bab10c` | the same | the same |
| encryption | | see (3) | see (4) |
| asked for the passphrase | never | once, by ZFSBootMenu | once, by ZFSBootMenu |

1. `quiet`, `root=zfs:zroot/ROOT/ubuntu` and `spl.spl_hostid=0x00bab10c`.
2. `luks.crypttab=no luks.uuid=... luks.key=...`.
3. `aes-256-gcm`, `file:///etc/zfs/zroot.key` and `available`.
4. `type: LUKS2`, `sector size: 4096` and `flags: discards`.

- `systemctl --failed` lists no units, and `networkctl list` shows the wired interface `routable` or `configured`.
- `BootCurrent` names the entry the firmware used. If it is the `ZFSBootMenu` entry, the firmware honours entries and
  the copy `\EFI\BOOT\BOOTX64.EFI` is only a reserve. If the firmware started another entry, the copy was used; the
  post-hook `50-bootx64.sh` of [Creating the firmware boot entries](#creating-the-firmware-boot-entries) keeps it
  current at every `generate-zbm`.
- **Option C**: make the first header backup now ([Option C only: backing up the LUKS
  header](#option-c-only-backing-up-the-luks-header)).

## Usage

### Updating the system

Take a snapshot of the boot environment before large updates; [Boot environments, snapshots and
rollback](#boot-environments-snapshots-and-rollback) shows how to go back:

```
root # zfs snapshot zroot/ROOT/ubuntu@pre-update-$(date +%Y%m%d)
root # apt update
root # apt full-upgrade
```

- A new kernel package brings its ZFS module with it, so there is no DKMS build and no kernel without a module. The
  kernel hook `/etc/kernel/postinst.d/dracut` builds `/boot/initrd.img-<version>` with the settings of
  `/etc/dracut.conf.d/10-zfs.conf`, so the key file of Options B and C is in it, and the file gets mode `0600`.
- `apt autoremove` removes old kernels, but keeps the running one and the two newest.
- To add a desktop, install for example `ubuntu-desktop-minimal`. First look at what APT would do: this guide could
  not confirm that no desktop package pulls in GRUB, `systemd-boot`, `initramfs-tools` or `shim`. The command must
  print nothing:

  ```
  root # apt install --simulate ubuntu-desktop-minimal | grep -E '^Inst (grub|systemd-boot |initramfs-tools|shim)'
  ```

  If it prints a line, do not install that way; install the packages you want one by one with
  `--no-install-recommends`.
- A release upgrade (`do-release-upgrade`) replaces the kernel, ZFS and dracut at once; take a snapshot first and
  rebuild ZFSBootMenu's image afterwards. This guide does not cover release upgrades further.

**Check** after an update that installed a kernel:

```
root # ls -l /boot/initrd.img-*
root # lsinitrd /boot/initrd.img-$(linux-version list | linux-version sort | tail -n 1) | grep -E 'zfs\.ko|zroot\.key'
```

Every initramfs is `-rw-------`, and the newest lists `zfs.ko` and, for Options B and C, the key file.

### Regenerating the ZFSBootMenu image

ZFSBootMenu's image contains the kernel, the ZFS module and tools, cryptsetup and the other unlock tools of **this
system at the time of the build**. Rebuild it after updates of the ZFS packages, of the kernel the image uses, of
cryptsetup, of dracut, and after changing anything in `/etc/zfsbootmenu/`:

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

  The last command puts the new entry first in the boot order; put `ZFSBootMenu` first again with
  `efibootmgr -o XXXX,YYYY,...` (the numbers that `efibootmgr` lists).
- `generate-zbm` uses the newest kernel in `/boot` and needs the ZFS module for it; `generate-zbm --kver VERSION`
  builds with another kernel.
- The post-hook `50-bootx64.sh` refreshes the firmware's fallback copy `\EFI\BOOT\BOOTX64.EFI` after every build.
  `cmp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI` prints nothing when the two are the same;
  `generate-zbm --debug` also prints `## Processing hook: ...50-bootx64.sh`. The `known-good.EFI` copy above is never
  refreshed; that is its purpose.
- Rebuild **before** raising the pool's compatibility
  ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).

**An APT hook.** Ubuntu has no hook for this, so this guide proposes one. After every APT run that changed one of the
packages copied into the images, it rebuilds Ubuntu's initramfs images; after every APT run that changed the newest
kernel with a ZFS module or one of the packages ZFSBootMenu's image is built from, it runs `generate-zbm` with that
kernel. It never makes APT fail. It was written for this guide and has not been run by the project.

**FILE** `/usr/local/sbin/zbm-after-apt`

```
#!/bin/sh
# Rebuild Ubuntu's initramfs and ZFSBootMenu's image after APT changed the kernel, ZFS, cryptsetup or dracut.
# Called by /etc/apt/apt.conf.d/95-zfsbootmenu. It never makes APT fail.
dir=/var/lib/zfsbootmenu
mkdir -p "${dir}"
pkgstate() { dpkg-query -W -f='${Package}=${Version} ' "$@" 2>/dev/null; }

# Ubuntu's initramfs images contain the ZFS tools, cryptsetup and dracut's modules
sys="$(pkgstate zfsutils-linux zfs-dracut cryptsetup-bin systemd-cryptsetup dracut-core)"
if [ "$(cat "${dir}/initramfs-stamp" 2>/dev/null)" != "${sys}" ]; then
  echo "zbm-after-apt: rebuilding Ubuntu's initramfs images"
  if update-initramfs -u -k all; then
    printf '%s\n' "${sys}" > "${dir}/initramfs-stamp"
  else
    echo "zbm-after-apt: update-initramfs failed; run it by hand and read its messages" >&2
  fi
fi

# ZFSBootMenu's image: the newest kernel with a ZFS module, and the programs the image is built from
newest=""
kver=""
for v in $(linux-version list | linux-version sort --reverse); do
  [ -n "${newest}" ] || newest="${v}"
  if modinfo -k "${v}" zfs >/dev/null 2>&1; then
    kver="${v}"
    break
  fi
done
if [ -z "${kver}" ]; then
  echo "zbm-after-apt: no kernel has a ZFS module; ZFSBootMenu not rebuilt" >&2
  exit 0
fi
if [ "${kver}" != "${newest}" ]; then
  echo "zbm-after-apt: WARNING: the newest kernel ${newest} has no ZFS module; do not boot it" >&2
fi
zbm="${kver} $(pkgstate zfsutils-linux cryptsetup-bin dracut-core systemd-boot-efi kexec-tools fzf)"
if [ "$(cat "${dir}/apt-stamp" 2>/dev/null)" = "${zbm}" ]; then
  exit 0
fi
echo "zbm-after-apt: rebuilding ZFSBootMenu with kernel ${kver}"
if generate-zbm --kver "${kver}"; then
  printf '%s\n' "${zbm}" > "${dir}/apt-stamp"
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

- APT runs `DPkg::Post-Invoke` commands after `dpkg` has finished, so the kernel's hooks have run by then. The stamps
  make sure each rebuild happens once, even when APT calls `dpkg` several times in one run; running `generate-zbm`
  twice in a row would replace the backup image with the new one.
- `update-initramfs -u -k all` is the `dracut` package's command; it rebuilds the images that exist.
- `generate-zbm` runs the post-hook `50-bootx64.sh`, which refreshes `\EFI\BOOT\BOOTX64.EFI`.

**Check:**

```
root # apt-config dump | grep -F zbm-after-apt
root # cat /var/lib/zfsbootmenu/apt-stamp
root # ls -l /boot/efi/EFI/zbm/ /boot/initrd.img-*
```

The first run of the script by hand printed both `rebuilding` lines and built an image; the second printed nothing.
`apt-config` shows the hook, the stamp names the kernel and the package versions, and the times of the files are those
of the first run. After the next APT run that installs a kernel or a ZFS package, `zbm-after-apt: rebuilding ...`
appears at the end of APT's output.

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Snapshots of
it are taken on the running system (`zfs snapshot zroot/ROOT/ubuntu@NAME`) or in ZFSBootMenu.

To open the menu, press **Escape** during the countdown (shell runtime) or any key (Rust runtime).

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
| **MOD+R** | roll back | rolls the boot environment back to the snapshot, after a confirmation (1) |
| **MOD+C** | clone | `zfs clone`: a new boot environment that depends on the snapshot (2) |
| **MOD+X** | clone and promote | `zfs clone`, `zfs promote`: a new boot environment that keeps the snapshots (3) |
| **Enter** | duplicate | `zfs send \| zfs recv`: an independent copy; uses as much space as the snapshot refers to |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

1. Everything written after the snapshot is lost.
2. For booting an earlier state for a while.
3. The old file system can then be destroyed.

**Rust runtime.** The menu has the tabs Boot, Snapshots, Pools and Log (**1** to **4**, or **Tab**). To roll back,
press **2** (or **s** on the Boot tab), select the snapshot and press **r**; a review page shows what is lost, and the
change is made once you type the target's name. **?** lists the keys of every view; the **MOD** keys of the shell
runtime work as aliases.

A worked example:

1. Before an update: `zfs snapshot zroot/ROOT/ubuntu@pre-update`.
2. After a bad update: reboot, press **Escape** in the countdown, select `zroot/ROOT/ubuntu`, press **MOD+S**, select
   `@pre-update`, press **MOD+R** and confirm.
3. Or, to try the old state without losing the new one: **MOD+C** (clone) on the snapshot, and boot the clone.

- Rollback and the other changes need the pool imported read/write; ZFSBootMenu imports pools read-only and imports
  them again when needed. In Option B that needs the key again; the key source set in
  [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties) saves the second prompt.
- A rollback of the boot environment does not touch `zroot/home`.
- A boot environment made by hand needs `mountpoint=/` and `canmount=noauto`; it inherits
  `org.zfsbootmenu:commandline` from `zroot/ROOT`.
- Snapshots and clones contain `/boot`, so each boots with the kernel and initramfs (and, in Options B and C, the key
  file) it had when the snapshot was taken. After the passphrase or the key file was replaced, an older snapshot's
  initramfs holds the old one, and its system will ask for the passphrase or stop in its initramfs.
- `zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the `org.zfsbootmenu:commandline` property
  that every boot environment inherits; see `man zbm-kcl`. Without a dataset name `zbm-kcl` acts on the root file
  system, `zroot/ROOT/ubuntu`, and a change there sets a property of its own on that boot environment only.

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
> Do not reboot before `update-initramfs` has finished. An initramfs with the old file cannot load the key: OpenZFS's
> dracut module runs `zfs load-key` with the file and does not fall back to a prompt, so the boot stops in the
> initramfs shell ([The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock]).

```
root # update-initramfs -u -k all
```

ZFSBootMenu's image holds no key, so no `generate-zbm` is needed.

**Check** (Option B):

```
root # ls -l /boot/initrd.img-*
root # lsinitrd -f etc/zfs/zroot.key "/boot/initrd.img-$(uname -r)" | cmp - /etc/zfs/zroot.key && echo same
```

Every image is `-rw-------`, and `cmp` prints `same`.

**Option C.**

```
root # cryptsetup luksChangeKey /dev/disk/by-partlabel/zroot-luks
```

It asks for the old passphrase, then for the new one twice. The key file's slot is not affected, and no image needs
rebuilding. Make new header backups afterwards: the old ones still accept the old passphrase. A second passphrase as
a reserve: `cryptsetup luksAddKey /dev/disk/by-partlabel/zroot-luks`.

**Check** (Option C):

```
root # cryptsetup open --test-passphrase --verbose /dev/disk/by-partlabel/zroot-luks
```

It accepts the new passphrase (`Key slot 0 unlocked.`).

### Replacing the key file

**Options A and B.** Option A has no key file; for Option B the key file is the passphrase, see
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
root # update-initramfs -u -k all
root # for i in /boot/initrd.img-*; do \
  lsinitrd -f etc/cryptsetup-keys.d/zroot.key "$i" | cmp - zroot.key && echo "$i ok"; done
```

`luksAddKey` asks for the passphrase. Rebuilding the initramfs is required: it contains a copy of the key file.
Continue only when the loop prints `ok` for every image and `ls -l /boot/initrd.img-*` shows them all
`-rw-------`. Then remove the old key slot and the old file:

```
root # cryptsetup luksRemoveKey /dev/disk/by-partlabel/zroot-luks zroot.key.old
root # rm zroot.key.old
root # cd
root # cryptsetup luksDump /dev/disk/by-partlabel/zroot-luks | grep -E '^\s+[0-9]+: luks2'
```

`luksDump` lists two key slots: the passphrase's and the new key file's (one more for each reserve passphrase).
Make a new header backup afterwards.

### Option C only: backing up the LUKS header

Skip this section for Options A and B. (Option B has no header: its wrapped keys are in the pool's metadata.)

A damaged LUKS header makes the data unrecoverable, even with the right passphrase. Store a header backup **away
from this SSD**, for example on an encrypted USB stick (replace the path):

```
root # cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks \
  --header-backup-file /PATH/ON/USB/zroot-luks-header.img
```

- A header backup holds the key slots as they were when it was made, so whoever has it and an old passphrase (or the
  old key file) can open the container. Keep it offline, and make a new one after every change of passphrase or key
  file.
- To restore: `cryptsetup luksHeaderRestore /dev/disk/by-partlabel/zroot-luks --header-backup-file FILE`.

**Check:**

```
root # cryptsetup luksDump /PATH/ON/USB/zroot-luks-header.img | grep -E '^UUID'
```

The UUID is that of the container (`cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks`).

### Scrubbing and trimming

A scrub reads and checks every block; a full TRIM complements `autotrim`. The timers enabled in
[Configuring the system](#configuring-the-system) run both monthly. By hand:

```
root # zpool scrub zroot
root # zpool trim zroot
root # zpool status -t zroot
root # systemctl list-timers 'zfs-*'
```

- `zpool status -t` shows the progress of both; the timers list their next run.
- Option C: TRIM reaches the SSD because the flag is in the LUKS header; `lsblk -D` shows non-zero discard sizes for
  `luks-<UUID>`.
- `nvme smart-log /dev/nvme0` (package `nvme-cli`) shows the SSD's health.

### Updating ZFSBootMenu

`claude/zbm-refactor` is a development branch: read what changed (`git log`) before building.

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

- `make install` overwrites `config.yaml`, hence the copy and the restore. Files of your own in
  `/etc/zfsbootmenu/dracut.conf.d/` (`generic.conf`, `runtime.conf`) are kept.
- `make install` writes the version into two files of the checkout, and `git pull` refuses to run when the update
  changes either of them. The `git checkout --` line restores both; `git status --short` must then print nothing.
- If you needed the GNU programs for `make install` in [Building and installing](#building-and-installing), use the
  same `PATH=` line again.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm before relying on it; the backup
entry still boots the previous image.

### Relaxing the pool compatibility

Most readers can skip this. The pool was created with the features of OpenZFS 2.2 only, so that older rescue systems
can import it. To enable newer features, update every system that imports the pool first, starting with ZFSBootMenu,
then raise the set one step at a time:

```
root # generate-zbm
root # zpool set compatibility=openzfs-2.3-linux zroot
root # zpool upgrade zroot
```

- Use the newest set that the installed ZFS **and** your rescue medium both know (`ls
  /usr/share/zfs/compatibility.d/`); `compatibility=off` enables everything the installed ZFS has.
- When ZFSBootMenu's ZFS is older than the pool's features, it shows *Unsupported features detected* and refuses
  read/write operations, or cannot import the pool at all.
- One `generate-zbm` run refreshes `vmlinuz.EFI` and, through the post-hook, `BOOTX64.EFI`; `vmlinuz-backup.EFI`
  is then the previous image. Before `zpool upgrade`, boot the new image once and run `generate-zbm` a second time,
  so that the backup holds the new ZFS too. A `known-good.EFI` copy, if you made one, is never updated: copy the new
  image over it, or delete it and its entry (`efibootmgr -b XXXX -B`).

**Check:** `zpool get compatibility zroot` shows the new set, and the next boot reaches Ubuntu.

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

- Check that Secure Boot is disabled ([Preparing the NVMe SSD](#preparing-the-nvme-ssd)).
- Choose *ZFSBootMenu*, or the NVMe SSD itself, in the firmware's boot menu (the key differs between vendors: often
  F8, F11, F12 or Esc).
- From the live system, `efibootmgr -v` shows the entries; create them again as in
  [Creating the firmware boot entries](#creating-the-firmware-boot-entries). The copy at `\EFI\BOOT\BOOTX64.EFI` is
  used when entries are missing.

### ZFSBootMenu finds no pool

ZFSBootMenu shows `Unable to import pool, retrying in NN seconds`. In the recovery shell:

```
root # cat /proc/cmdline
root # ls /dev/nvme*
root # zpool import
```

**Option C** adds:

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
- Options A and B with `zbm.luks=off`: there is nothing to unlock; look at the first two points.

### Option C: ZFSBootMenu never asks for the passphrase (the image lacks cryptsetup)

ZFSBootMenu never asks for a passphrase and keeps retrying the pool import. In its recovery shell,
`command -v cryptsetup` prints nothing; with the shell runtime `zluks` is missing too, while with the Rust runtime
`zluks` exists but cannot open the container. `cryptsetup` was not installed when the image was built (dracut then
leaves LUKS out without a message). Follow
[Repairing the system from the live medium](#repairing-the-system-from-the-live-medium), run
`apt install --no-install-recommends cryptsetup` and `generate-zbm` in the chroot, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

After ZFSBootMenu started Ubuntu's kernel, Ubuntu's initramfs could not use the key file:

- **Option B**: `zfs load-key` fails with the file and does not ask; the boot stops in dracut's emergency shell. There,
  `zfs load-key -L prompt zroot` loads the key and `exit` lets the boot go on; this guide could not confirm that the
  boot always continues from there, and the live medium always works.
- **Option C**: systemd-cryptsetup asks for the passphrase. Type it.

Then, on the running system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitrd "/boot/initrd.img-$(uname -r)" | grep -E 'zroot\.key|systemd-cryptsetup'
```

Causes, in order of likelihood:

- the key file is not in the initramfs: `/etc/dracut.conf.d/10-zfs.conf` is missing or misspelled; correct it and run
  `update-initramfs -u -k all`;
- Option C: the command line lacks the `luks.*` parameters, or carries another UUID
  ([Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties));
- Option C: `systemd-cryptsetup` is not in the initramfs;
- Option B: the passphrase was changed without rebuilding the initramfs
  ([Changing the passphrase](#changing-the-passphrase));
- an old snapshot or clone was booted, whose initramfs holds an older key.

If ZFSBootMenu **itself** asks twice, that is something else: in Option C without `zbm.luks=` it offers every LUKS
volume it finds.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment. In the recovery shell (Option B: `zfs load-key -L prompt zroot` first; the
key file named by `keylocation` is not in ZFSBootMenu's image):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/ubuntu
root # ls -l MOUNTPOINT/boot
```

`mount_zfs` prints the mount point to use for `MOUNTPOINT`. Causes:

- `/boot` in the dataset is empty: the kernels were installed onto the ESP or another partition mounted at `/boot`;
- a kernel has no initramfs beside it (`vmlinuz-<version>` needs `initrd.img-<version>`), for example because the
  kernel hook failed; run `update-initramfs -c -k all` on the installed system;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

- In ZFSBootMenu: the default import policy adopts the host ID of the pool when it cannot import it with its own, so
  this usually resolves itself. Otherwise, in the recovery shell: `zpool import -f -N zroot`, then `exit`.
- In Ubuntu's initramfs (emergency shell): `zpool import -f -N zroot`, then `exit`.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; run
  `update-initramfs -u -k all` and `generate-zbm`. The usual cause is a live medium that imported the pool and was
  shut down without `zpool export`: always export the pool on a rescue medium.
- `generate-zbm` printing `SPL (...) and system (...) hostids do not match!` means that the loaded ZFS module and
  `/etc/hostid` disagree on the build host. It is a warning only; the image takes `/etc/hostid`.

### The recovery shell and its tools

| Command | Use |
|---|---|
| `zfsbootmenu` (or `exit`) | back to the menu |
| `zpool`, `zfs` | the ZFS tools of the image; `zpool import`, `zpool status` |
| `set_rw_pool POOL`, `set_ro_pool POOL` | import the pool again read/write or read-only |
| `mount_zfs DATASET` | mount a file system at a temporary place and print it |
| `zfs-chroot DATASET` | chroot into a boot environment, to repair it or rebuild its initramfs (1) |
| `zsnapshots DATASET` | the snapshot browser |
| `zkexec DATASET KERNEL INITRAMFS` | boot a chosen kernel and initramfs directly |
| `mount_esp DEVICE` | mount an ESP |
| `zluks`, `zluks list`, `zluks status`, `zluks open DEVICE [NAME]`, `zluks close` | Option C (2) |
| `logs`, `zlogtail`, `zreport`, `zbmcmdline` | logs, a report for bug reports, ZFSBootMenu's command line |
| `help` | the built-in documentation |
| `reboot`, `poweroff`, `firmware-setup` | leave |

1. Read/write only after `set_rw_pool`.
2. Unlock, list and close LUKS volumes; `cryptsetup` and `dmsetup` are there as well.

### Repairing the system from the live medium

Boot the Ubuntu live medium, become root and install the tools as in [Installation media](#installation-media). Then
set the variables and the host ID:

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
root # export POOL=zroot
root # export BE=ubuntu
root # export POOL_DEV="${POOL_PART}"
root # zgenhostid -f 0x00bab10c
```

**Option C** replaces `POOL_DEV` and opens the container:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

**All options.**

```
root # zpool import -f -N -R /mnt "${POOL}"
```

**Option B**:

```
root # zfs load-key -L prompt "${POOL}"
```

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # mount "${ESP_PART}" /mnt/boot/efi
```

Write the variables file again, with the `cat > /mnt/root/install.env` block of
[Copying the host ID and the key file](#copying-the-host-id-and-the-key-file), so that the chroot has them. Then mount
and enter the chroot as in [Chrooting](#chrooting), repair, leave it, and finish as in
[Finalizing the installation](#finalizing-the-installation) without the snapshot: `rm /mnt/root/install.env`,
unmount, `zpool export "${POOL}"`, and for Option C `cryptsetup close "luks-${LUKS_UUID}"`.

- The host ID of the live system is set to the pool's, so that the installed system still imports the pool if the
  live system was shut down without `zpool export`.
- `-f` is needed when the installed system did not export the pool, which is the normal case after a crash or a
  power cut.
- The stored TRIM flag is applied by `cryptsetup open` itself.
- Do not change the pool from inside the chroot (see [Mounting the pool below /mnt](#mounting-the-pool-below-mnt)).

**Check:** `hostid` prints `00bab10c` before the import, and `findmnt -R /mnt` shows `zroot/ROOT/ubuntu` at `/mnt`.

### Continuing after a reboot of the live system

The live system keeps everything in memory. After a reboot during the installation, the installed tools, the
variables, the host ID, the pool cache, Option B's passphrase file on the live system, the open container, the
imported pool and all mounts are gone; what was written to the SSD stays. Restore what you need, then continue with
the section where you stopped.

1. Install the tools as in [Installation media](#installation-media) (`apt update`, `apt install ...`,
   `modprobe zfs`).
2. Set the variables as in [Setting variables](#setting-variables).
3. Set the host ID again, before any import: `zgenhostid -f 0x00bab10c`.
4. **Option C**, if the container exists: open it and set `LUKS_UUID` and `POOL_DEV` with the three lines at the end
   of [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container). **Options A and B**:
   `export POOL_DEV="${POOL_PART}"`.
5. If the pool does not exist yet, stop here and continue with the section where you stopped. Otherwise import it
   and, for Option B, load the key by typing the passphrase:

   ```
   root # zpool import -N -R /mnt "${POOL}"
   root # zfs load-key -L prompt "${POOL}"
   ```

   The second line is for Option B only.
6. Mount the datasets and write the pool cache again:

   ```
   root # zfs mount "${POOL}/ROOT/${BE}"
   root # zfs mount "${POOL}/home"
   root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
   ```

7. **Option B**, only if you stopped before
   [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file): that step copies the passphrase
   file from the live system. Write it again with **the same passphrase**, with the `mkdir`, `bash -c` and `chmod`
   lines of [Creating the ZFS pool](#creating-the-zfs-pool), and check it: `zfs load-key -n "${POOL}"` reads the file
   and must return without an error message. If it reports an incorrect key, the file differs from the passphrase;
   write it again. After that step, the copy in `/mnt/etc/zfs/` is the one that counts.
8. If you were past [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file), mount the ESP
   (`mount "${ESP_PART}" /mnt/boot/efi`). If you were past [Chrooting](#chrooting), mount and enter the chroot as
   shown there, and set `KVER` again as shown in [Kernel and ZFS](#kernel-and-zfs).

**Check:**

```
root # hostid
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`hostid` prints `00bab10c`, `/mnt` is `zroot/ROOT/ubuntu` and `/mnt/home` is `zroot/home` (with `/mnt/boot/efi` if you
mounted the ESP), and the key status is `available` for Option B (`-` for Options A and C). If `zpool import` refuses
the pool because it was last used by another system, step 3 was skipped or ran after the import: run it, then the
import again.

### The live system cannot load ZFS

`modprobe zfs` reports `Module zfs not found`. Ubuntu ships the ZFS module in a package of its own for each kernel,
`linux-main-modules-zfs-<version>`. The 26.04.1 Desktop image includes it (its manifest lists
`linux-main-modules-zfs-7.0.0-30-generic`); another image may not. Install the one for the running kernel and load
it:

```
root # apt install "linux-main-modules-zfs-$(uname -r)"
root # modprobe zfs
```

If APT finds no such package, the live image's kernel is older than the archive's; use a current live image (a newer
point release).

### dracut cannot find the zfs module

`apt` or `update-initramfs` fails with `dracut-install: Failed to find module 'zfs'` or a line `FAILED: ...
dracut-install ... -m zfs`. The kernel named in the message has no ZFS module:

```
root # dpkg -l 'linux-main-modules-zfs-*' | grep '^ii'
root # apt install linux-image-generic
root # update-initramfs -c -k all
```

`linux-image-generic` depends on the module package of its kernel, so installing it again brings the module back. A
kernel installed by its versioned package name alone (`linux-image-<version>`) also needs
`linux-main-modules-zfs-<version>`.

If the kernel package's installation fails in the chroot because dracut fails, check that
`/etc/dracut.conf.d/10-zfs.conf` holds the settings shown in [Kernel and ZFS](#kernel-and-zfs) (it must exist before
the kernel is installed), correct it and run `dpkg --configure -a`.

### APT wants to install initramfs-tools or remove dracut

APT proposes to install `initramfs-tools`, or to remove `dracut` and `zfs-dracut`. The two generators conflict, and a
package you asked for needs `initramfs-tools`, usually `zfs-initramfs`. Answer **n**. This guide's setup needs
`dracut` and `zfs-dracut`; leave out the package that needs `initramfs-tools`, or install what you wanted with
`--no-install-recommends`. Check afterwards that `dpkg -l dracut zfs-dracut | grep '^ii'` lists both.

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption leaves the pool's structure readable without the passphrase: dataset and snapshot names, the
  dataset hierarchy, properties (including `org.zfsbootmenu:commandline`), file sizes and holes. File and directory
  contents, file attributes, ACLs and directory listings are encrypted.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Ubuntu's initramfs in
  `/boot`, also inside the pool. It is safe only there. Any copy of the boot environment that is not encrypted carries
  it in clear: a `zfs send` without `-w` (raw), a file-level backup of `/etc` or `/boot`, an initramfs copied to the
  ESP. On the running system, root can read the passphrase.
- `zfs change-key` does not change the key the data is encrypted with, and the old wrapped key is not overwritten on
  disk. Changing the passphrase is therefore no cure for a passphrase that someone learned together with a copy of
  the disk.
- **Never** place encryption keys inside a ZFSBootMenu image.

**Option C.**

- The ESP is not encrypted and holds only ZFSBootMenu's image and its copies. They contain no key: plain key files are
  never copied into the image.
- The key file opens the container. It is safe because `/etc/cryptsetup-keys.d` and the initramfs in `/boot` are
  inside the encrypted pool. It stops being safe the moment an initramfs with the key is written anywhere else: the
  ESP, an unencrypted partition, a backup, or a unified kernel image. Snapshots and `zfs send` streams of the root
  dataset contain it too; treat them like the pool.
- A LUKS header backup contains the key slots as they were: an old passphrase still opens a restored header. Store
  backups offline and replace them after key changes.
- ZFSBootMenu closes the container before `kexec`, which wipes the volume key from kernel memory.
- TRIM pass-through reveals which blocks are unused, not their content.

**All options.**

- **The ESP is not encrypted.** Anyone with access to the disk, or root on the system, can replace ZFSBootMenu's image
  with one that records the passphrase. Encryption protects data at rest, for example a stolen powered-off laptop,
  not against someone who can modify the computer and wait for you to type the passphrase. Secure Boot with signed
  images, or measured boot with a TPM, narrows that; neither is set up by this guide. (`contrib/zbm-sign.pl` in this
  repository is a `generate-zbm` hook that signs images; see [Building ZFSBootMenu](../../general/building.rst).)
- Once the pool is unlocked, the recovery shell and the command-line editor are open to anyone at the keyboard.
- Hooks run as root in ZFSBootMenu; install only hooks you have read.
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed
  before updating, keep the backup entries, and keep the live medium.

The threat model of this fork, with what it protects against and what it does not, is in
[docs/dev/threat-model.md](../../dev/threat-model.md).

## See also

- [ZFS Pools on LUKS Volumes](../../general/luks.rst), [Native Encryption](../../general/native-encryption.rst),
  [Building ZFSBootMenu](../../general/building.rst), [UEFI Booting](../../general/uefi-booting.rst),
  [Boot Environments and You](../../general/bootenvs-and-you.rst)
- [LUKS administration](../../luks/administration.rst), [ZFS Versions and Pool Features](../../luks/zfs-versions.rst),
  [Testing](../../luks/testing.rst)
- [zfsbootmenu(7)](../../man/zfsbootmenu.7.rst), [generate-zbm(5)](../../man/generate-zbm.5.rst),
  [generate-zbm(8)](../../man/generate-zbm.8.rst), [zbm-kcl(8)](../../man/zbm-kcl.8.rst), and the manual pages
  `zfsbootmenu-luks(7)` and `zluks(8)` (`docs/man/zfsbootmenu-luks.7.rst`, `docs/man/zluks.8.rst`)
- [Ubuntu (UEFI)](uefi.rst), the upstream-style guide for Ubuntu in this repository, which this guide follows for
  `debootstrap`; it builds upstream ZFSBootMenu and has no LUKS option
- [Gentoo Linux on a LUKS-encrypted ZFS root](../gentoo/zfs-luks-v4.md), the model of this guide's form
- [The threat model](../../dev/threat-model.md)
- Ubuntu:
  [Dracut news for Ubuntu 26.04 LTS and 26.10][dracut-news],
  the package pages at <https://packages.ubuntu.com/resolute/>, and the manual pages `debootstrap(8)`, `dracut(8)`,
  `dracut.conf(5)`, `dracut.zfs(7)` (from `zfs-dracut`), `update-initramfs(8)` and `netplan(5)`
- OpenZFS documentation: <https://openzfs.github.io/openzfs-docs/>
- `gdisk(8)`, `cryptsetup(8)`, `systemd-cryptsetup-generator(8)` for the `luks.*` parameters

[dracut-news]: https://discourse.ubuntu.com/t/dracut-news-for-ubuntu-26-04-lts-and-26-10/87327
