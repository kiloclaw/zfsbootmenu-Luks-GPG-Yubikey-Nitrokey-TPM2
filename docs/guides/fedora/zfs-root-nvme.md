# Fedora 45 on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install Fedora Linux 45 from scratch on a **single NVMe SSD** with a **ZFS root
pool**. The system boots with the ZFSBootMenu build from this repository (branch `claude/zbm-refactor`). Fedora is
installed with `dnf --installroot` from the Fedora Workstation live image; OpenZFS comes from the OpenZFS project's
Fedora repository and is built for each kernel with DKMS; Fedora's own initramfs is built by **dracut**.

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Fedora's initramfs then unlocks the pool again
with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from the documentation of Fedora, OpenZFS, systemd, dracut and this repository. The project
> has not run it from start to finish. `claude/zbm-refactor` is a development branch, not a release.
> The project's VM tests boot ZFSBootMenu images built with dracut and a test system whose initramfs is built
> with dracut, with unencrypted pools, ZFS native encryption and LUKS; they do not install Fedora.
> Keep a live medium at hand until the system has booted several times, and read
> [What the project has tested](#what-the-project-has-tested) before you start.

> **Important**
> On 17 November 2026 no OpenZFS release supports Linux 7.3, and Linux 7.2 is end of life upstream. This guide holds
> the kernel at the 7.2 series, so the system gets **no kernel security fixes** until an OpenZFS release supports
> Linux 7.3. See [Installation media](#installation-media) for how to check this on the day you install.

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
  - [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file)
  - [Chrooting](#chrooting)
- [Kernel and ZFS](#kernel-and-zfs)
  - [Holding the kernel at a version OpenZFS supports](#holding-the-kernel-at-a-version-openzfs-supports)
  - [Keeping kernels in /boot](#keeping-kernels-in-boot)
  - [Installing the kernel and OpenZFS](#installing-the-kernel-and-openzfs)
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
  - [Option C only, optional: TPM2 with a PIN](#option-c-only-optional-tpm2-with-a-pin)
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
  - [The ZFS module is missing for a kernel](#the-zfs-module-is-missing-for-a-kernel)
  - [dnf refuses a kernel or ZFS update](#dnf-refuses-a-kernel-or-zfs-update)
  - [SELinux denials after the first boot](#selinux-denials-after-the-first-boot)
- [Security notes](#security-notes)
- [See also](#see-also)

[no-cryptsetup]: #option-c-zfsbootmenu-never-asks-for-the-passphrase-the-image-lacks-cryptsetup
[second-unlock]: #the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs
[hostid-refused]: #the-pool-is-refused-wrong-host-id-or-last-used-by-another-system
[kernel-hold]: #holding-the-kernel-at-a-version-openzfs-supports

## Overview

### Choosing an option

| | Option A | Option B | Option C |
|---|---|---|---|
| Pool | not encrypted | ZFS native encryption (`aes-256-gcm`) | inside a LUKS2 container |
| Passphrase typed | never | once, in ZFSBootMenu | once, in ZFSBootMenu |
| Second unlock, by Fedora's initramfs | none | key file (1) | key file (2) |
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
3. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/fedora` with a countdown of 10 seconds.
   When the countdown ends, the pool's `bootfs` is booted; a key press opens the menu instead.
4. **Option B**: the `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in
   ZFSBootMenu's image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime
   this prompt comes **after** the countdown.
5. ZFSBootMenu loads Fedora's kernel and initramfs from `/boot` **inside the boot environment** and starts the kernel
   with `kexec`. It passes the command line stored in the ZFS property `org.zfsbootmenu:commandline`, the root
   argument `root=zfs:zroot/ROOT/fedora` and the host ID. **Option C**: before `kexec` it exports the pool and closes
   the container, which wipes the volume key from memory.
6. Fedora's initramfs (dracut, with systemd and OpenZFS's `zfs` module) starts in the new kernel:
   - **Option A**: it imports the pool and mounts the root file system.
   - **Option B**: it imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the initramfs**,
     without asking.
   - **Option C**: systemd-cryptsetup unlocks the container again with the key file
     `/etc/cryptsetup-keys.d/zroot.key` **inside the initramfs**, without asking; then the pool is imported.

Why a second unlock (Options B and C): `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. The boot environment's own initramfs must unlock the pool again, exactly as it would without
ZFSBootMenu. ZFSBootMenu does not replace Fedora's initramfs.

### Where the keys are

**Option A.** There are none.

| Key | Option | Stored where | Used by |
|---|---|---|---|
| ZFS passphrase | B | your memory | ZFSBootMenu at every boot; a live system for repairs |
| The same passphrase in `/etc/zfs/zroot.key` | B | in the pool (1) | Fedora's initramfs, after `kexec` |
| LUKS passphrase (key slot 0) | C | your memory | ZFSBootMenu at every boot; a live system for repairs |
| Random key file `/etc/cryptsetup-keys.d/zroot.key` (slot 1) | C | in the pool (1) | Fedora's initramfs |

1. Inside the encrypted pool, and in the copy of it in Fedora's initramfs in `/boot`, also inside the pool.

The key file in Fedora's initramfs is safe **only because** the file and the initramfs both live inside the encrypted
pool (`/etc` and `/boot`); they cannot be read before the passphrase has been typed. ZFSBootMenu's own image lives on
the unencrypted ESP and never contains a key.

### What the project has tested

From [Testing](../../luks/testing.rst):

- The VM tests boot ZFSBootMenu images built **with dracut** by the project's build container (Ubuntu 26.10,
  Linux 7.3, OpenZFS 2.4.4, cryptsetup 2.8.7, systemd 261, dracut 112), in QEMU. The maintainer runs them on a
  Fedora 45 host, but the images and the system they boot are built in that Ubuntu container, not on Fedora.
- The system they boot is a test payload, not an installed distribution: an Ubuntu kernel with a **dracut** initramfs
  that holds OpenZFS's `zfs` module and systemd-cryptsetup, and a minimal root file system.
- For this guide that means: both generators are the tested ones (dracut for ZFSBootMenu's image, dracut with systemd
  for the system's initramfs). Fedora's kernel, the OpenZFS packages for Fedora, Fedora's dracut, SELinux and the
  `kernel-install` set-up of this guide are **not** covered.
- **Option A**: unencrypted pools are booted with both runtimes.
- **Runtimes**: the suite runs with the Rust runtime by default. The shell runtime, which this guide uses unless you
  choose otherwise, was last run in full on 20 October 2026; its `runtime-legacy-*` tests run only when named.
- **Option B**: ZFSBootMenu's passphrase prompt is tested. The single-prompt arrangement, with the key file in the
  system's initramfs, is **not**: the test payload's initramfs holds no ZFS key file. It is the design described in
  [Native Encryption](../../general/native-encryption.rst).
- **Option C**: the design of this guide is tested on the payload: an NVMe disk with 4 KiB sectors, a LUKS2 container
  with 4 KiB encryption sectors and the TRIM flag in its header, `zbm.luks=UUID=`, and a dracut initramfs that
  unlocks again with a key file and the `luks.uuid=`, `luks.key=` and `luks.crypttab=no` parameters, as here.
- Real hardware and Secure Boot are not covered.

## Requirements

### Hardware

- An x86_64 computer with an NVMe SSD that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware: the ZFSBootMenu image built here is not signed, and the ZFS module that
  DKMS builds is not signed either. How to check is shown in [Preparing the NVMe SSD](#preparing-the-nvme-ssd).
- An internet connection, wired if possible.
- At least 8 GiB of RAM is advised: the live system keeps the packages it installs in memory and builds the ZFS module
  there. This guide could not confirm how much space the Fedora 45 live image gives its writable layer; `df -h /`
  on the live system shows it.
- Free space in the pool for the build: about 1 GB for the clone of this repository (most of it the Rust crates in
  `vendor/`) and several GB for Cargo's build directory, which this guide deletes after installing. The size of the
  build directory was not measured; `df -h /mnt` on the live system shows the free space.

### Installation media

The facts this guide depends on, and how to check each on your system:

| Fact | Value for Fedora 45 | Check |
|---|---|---|
| Release | Fedora Linux 45, released on 3 November 2026 | `cat /etc/fedora-release` |
| Kernel | Linux 7.2 (1) | `uname -r` |
| OpenZFS | the newest release is 2.4.4; from the OpenZFS project's repository for Fedora (2) | `zfs version` |
| Kernels OpenZFS supports | written into the `zfs-dkms` package (3) | `rpm -q --requires zfs-dkms \| grep kernel` |
| System initramfs | dracut, with systemd in the initramfs | `dracut --version` |
| systemd | 262 | `systemctl --version` |
| `rustc` | a 1.9x version, newer than the 1.88 this repository needs | `rpm -q rust` |
| EFI stub | package `systemd-boot-unsigned` | `rpm -qf /usr/lib/systemd/boot/efi/linuxx64.efi.stub` |
| Package manager | DNF 5 (`dnf`), with `versionlock` built in | `dnf --version` |

1. Kernel 7.2.15-300.fc45 was in Fedora 45's updates on the release day. Linux 7.2 is **end of life** upstream:
   7.2.15, of 1 November 2026, was its last release, and its users are told to move to 7.3. Fedora ran a test week
   for kernel 7.3 from 1 to 7 November; this guide found no Fedora 45 update to 7.3 yet.
   `dnf list --upgrades 'kernel*'` shows whether one exists.
2. Package `zfs-release` adds the repository; the packages are `zfs`, `zfs-dkms` and `zfs-dracut`. OpenZFS 2.4.4 was
   released on 21 August 2026 and supports Linux 4.18 to 7.2. No OpenZFS release supporting Linux 7.3 was found as
   of 17 November 2026.
3. `zfs-dkms` requires `kernel-devel <= M.N.999` and `kernel-uname-r <= M.N.999` (`kernel-uname-r` is provided by
   `kernel-core`), and **conflicts** with any newer `kernel-devel` or kernel.

This guide could not open Fedora's package index or the OpenZFS download site from where it was written: the OpenZFS
version offered for Fedora 45, the exact `rustc` version and the package names `systemd-boot-unsigned`,
`systemd-resolved` and `libX11-common` for Fedora 45 are not confirmed. The commands of the guide check each of them
on your system before they matter.

> **Important**
> OpenZFS supports each kernel series only once a release says so, and Fedora moves to new kernel series during a
> release's life. While `zfs-dkms` is installed, its conflicts make DNF refuse a kernel newer than OpenZFS supports,
> and an update that brings one fails or is skipped. This guide therefore **holds the kernel** at the newest version
> OpenZFS supports ([Holding the kernel at a version OpenZFS supports][kernel-hold]), so that DNF's transactions stay
> free of those conflicts, and tells you how to move on later ([Updating the system](#updating-the-system)).
> Today this means the system stays on a 7.2 kernel, such as 7.2.15-300.fc45, and receives **no kernel security
> fixes** until OpenZFS supports Linux 7.3. Check with `rpm -q --requires zfs-dkms | grep 'kernel-devel <='` and
> `dnf list --upgrades 'kernel*'`.

Download the **Fedora Workstation 45** live image for x86_64 from <https://fedoraproject.org/workstation/download>,
write it to a USB stick and boot it in **UEFI** mode. Choose to try Fedora rather than install it; the live user is
`liveuser` and needs no password. Connect to the network and open a terminal.

Stop the desktop from mounting old partitions of the SSD (run this as `liveuser`, before becoming root):

```
liveuser $ gsettings set org.gnome.desktop.media-handling automount false
```

Become root, add the OpenZFS repository and install ZFS for the live system's kernel. `kernel-devel-$(uname -r)` must
match the running kernel exactly; Fedora's release repository keeps the kernel of the live image. Installing `zfs`
builds the module with DKMS, which takes a few minutes.

```
liveuser $ sudo -i
root # dnf install -y "https://zfsonlinux.org/fedora/zfs-release-3-1$(rpm --eval '%{dist}').noarch.rpm"
root # dnf install -y "kernel-devel-$(uname -r)"
root # dnf install -y zfs gdisk nvme-cli dosfstools
root # modprobe zfs
```

- `dnf` asks to import the OpenZFS signing key; `-y` accepts it. The URL is the one the OpenZFS documentation gives
  for Fedora; `rpm --eval '%{dist}'` prints `.fc45`.
- If the first `dnf` reports that the URL is not found (404), OpenZFS has not published the package for Fedora 45
  yet. Stop here; do not install the package of another Fedora release.
- If `dnf install zfs` fails with a conflict between `zfs-dkms` and `kernel-uname-r` or `kernel-devel`, or a
  message that nothing provides `kernel-uname-r <= ...`, the live image's kernel is newer than the OpenZFS version
  in the repository supports. Use an older Fedora live image whose kernel is supported, or wait for an OpenZFS
  release that supports it.
- If `modprobe zfs` reports `Module zfs not found`, `dkms status` shows whether the module was built, and
  `/var/lib/dkms/zfs/*/build/make.log` why not. If it reports `Permission denied`, SELinux may refuse the new module
  on the live system; this guide could not confirm whether that happens. `setenforce 0` switches the **live system**
  to permissive mode until it is rebooted; the installed system is not affected.

**Option C** also needs cryptsetup, which the Workstation live image normally has; this installs it if not:

```
root # dnf install -y cryptsetup
```

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new Fedora system** |
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
root # rpm -q gdisk
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
root # cryptsetup --version
```

- `ls` lists files; otherwise the live system booted in legacy BIOS mode.
- `zfs version` prints `zfs-2.4...` (the tools) and `zfs-kmod-2.4...` (the module), or later versions. The pool in
  this guide needs 2.2 or later.
- `rpm -q gdisk` prints `gdisk-1.0.9` or a later version (gdisk itself has no `--version` option).
- The last number printed by `od` is `0` when Secure Boot is disabled and `1` when it is enabled. If it is `1`,
  disable Secure Boot in the firmware setup first. If the file does not exist, the firmware has no Secure Boot.
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
root # export BE=fedora
root # export KERNEL_MAX="$(rpm -q --requires zfs-dkms | awk '$1 == "kernel-devel" && $2 == "<=" {print $3; exit}')"
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the EFI system partition (1) |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `fedora` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}` |
| `KERNEL_MAX` | for example `7.2.999` | A B C | the newest kernel version the installed OpenZFS supports (2) |
| `POOL_DEV` | A, B: `${POOL_PART}`; C: `/dev/mapper/luks-${LUKS_UUID}` | A B C | the device `zpool create` gets (3) |
| `LUKS_UUID` | `$(cryptsetup luksUUID "${POOL_PART}")` | C | the LUKS container's UUID (4) |

1. NVMe partitions have a `p` before the number.
2. Read from the `zfs-dkms` package; used for the kernel hold.
3. Set in [Creating the ZFS pool](#creating-the-zfs-pool) (A, B) or
   [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container) (C).
4. Exists only after `luksFormat`.

The name in `BE` is free; ZFSBootMenu does not read it. It recognizes the distribution from `/etc/os-release` inside
the boot environment.

Some paths are written literally, because files cannot expand variables. If you change `POOL`, change `zroot` in
these paths too: `/etc/zfs/zroot.key` (Option B), `/etc/cryptsetup-keys.d/zroot.key` (Option C), and every later
command that names `zroot` after the installation.

> **Tip: if the terminal is closed or the live system restarts during the installation**
>
> - **Terminal closed only:** set these variables again; Option C also sets `LUKS_UUID` and `POOL_DEV` again (the
>   first two commands at the end of
>   [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container)). Inside the chroot,
>   enter it again as shown in [Chrooting](#chrooting).
> - **Live system restarted:** it has lost everything it kept in memory: the installed packages, the variables,
>   `/etc/hostid`, Option B's `/etc/zfs/zroot.key`, the open LUKS container, the imported pool and the mounts. What
>   was written to the SSD stays. Before the pool exists, start again from [Installation media](#installation-media).
>   Once it exists:
>   1. repeat the commands of [Installation media](#installation-media) and set the variables again;
>   2. `zgenhostid -f 0x00bab10c`, before any `zpool` command;
>   3. **Option C:** open the container with the three commands at the end of
>      [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container);
>   4. `zpool import -N -R /mnt zroot`, then continue in [Mounting the pool below /mnt](#mounting-the-pool-below-mnt)
>      from the `zfs load-key` line (Option B) or the `zfs mount` lines (Options A and C);
>   5. **Option B**, only if you stopped before
>      [Copying the host ID and the key file](#copying-the-host-id-and-the-key-file): write `/etc/zfs/zroot.key`
>      again with the **same** passphrase (step 2 of [Creating the ZFS pool](#creating-the-zfs-pool)); then
>      `zfs load-key -n zroot` reads the file and must not report an error. After that step the copy in
>      `/mnt/etc/zfs/` is the one that counts;
>   6. once the base system is installed: `mount "${ESP_PART}" /mnt/boot/efi`, then the mounts of
>      [Chrooting](#chrooting).

**Check:**

```
root # lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"
root # echo "${KERNEL_MAX}"
```

The model and serial number are those of the SSD to erase. `KERNEL_MAX` is a version such as `7.2.999`: OpenZFS
supports every kernel of the series `7.2`, and none newer. If it is empty, `zfs-dkms` is not installed; go back to
[Installation media](#installation-media). Do not continue with an empty `KERNEL_MAX`: the kernel hold built from it
would hold back nothing.

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
> The second block of commands in this section destroys everything on `${DISK}` at once and cannot be undone. Run
> the first block alone and read its output before you go on.

```
root # lsblk -o NAME,SIZE,MODEL,SERIAL,FSTYPE,MOUNTPOINTS "${DISK}"
```

Continue only if the model and serial number are those of the SSD to erase and no mount point is shown; unmount
anything the live system mounted from the SSD (`umount PATH`) first.

```
root # swapoff -a
root # zpool export -a
root # for part in "${DISK}"p*; do [ -b "${part}" ] && zpool labelclear -f "${part}"; done
root # for part in "${DISK}"p*; do [ -b "${part}" ] && wipefs -a "${part}"; done
root # blkdiscard -f "${DISK}"
root # wipefs -a "${DISK}"
```

- `swapoff -a` and `zpool export -a` release a swap partition or an old pool that the live system took into use;
  "no pools available to export" is not an error. The Fedora live system also uses a swap device in memory (zram);
  `swapoff -a` switches it off too, which is harmless.
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
| 2, Option C | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `8309` Linux LUKS | `zroot-luks` | the LUKS2 container |

1. FAT32, mounted at `/boot/efi`, holds ZFSBootMenu.

The ESP holds only ZFSBootMenu, but several copies of it: the image, its backup, a copy at `\EFI\BOOT\BOOTX64.EFI`,
and any copy you keep yourself. 1 GiB leaves room for all of them. No Fedora kernel or initramfs is ever stored on the
ESP: they stay in `/boot` inside the pool. The type `BF00` is a label for people and tools; ZFS ignores it.

What you type, step by step:

| Step | Command | Answers |
|---|---|---|
| New, empty GPT | `o` | `y` |
| Partition 1, the ESP | `n` | `<Enter>`, `<Enter>`, `+1G`, `ef00` |
| Partition 2, the rest | `n` | `<Enter>`, `<Enter>`, `<Enter>`, then `bf00` (Options A, B) or `8309` (Option C) |
| Name partition 1 | `c` | `1`, `EFI` |
| Name partition 2 | `c` | `2`, then `zroot` (Options A, B) or `zroot-luks` (Option C) |
| Print and compare with the table above | `p` | |
| Write and leave | `w` | `y` |

gdisk changes nothing on the SSD until `w`; `q` leaves without writing. The transcript is for Options A and B, from a
1 TB SSD with 512-byte sectors. Your sector numbers differ: **never type the numbers from the transcript**; press
Enter for the defaults.

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

**Option C** types `8309` instead of `bf00`, and `zroot-luks` instead of `zroot`:

```
Hex code or GUID (L to show codes, Enter = 8300): 8309
Changed type of partition to 'Linux LUKS'
```

```
Command (? for help): c
Partition number (1-2): 2
Enter name: zroot-luks
```

and the `p` listing ends with `8309  zroot-luks`.

**All options.** Things you may see that the transcript does not show:

- The opening lines differ when an old table is still found (`MBR: protective`, `GPT: present`); `o` replaces
  whatever is there.
- `+1G` makes partition 1 exactly 1 GiB. gdisk 1.0.9 and later also align the **default end** of partition 2, which
  matters for Option C's 4096-byte encryption sectors; the Check below confirms it.
- If gdisk ends with "Warning: The kernel is still using the old partition table", run
  `dnf install -y parted && partprobe "${DISK}"`, or reboot the live system and set the variables again.
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
- The `echo` prints `0`: the size of partition 2 is a whole number of MiB. If it prints another number, gdisk is
  older than 1.0.9. For Options A and B that does no harm. For Option C, run `gdisk "${DISK}"` again, delete
  partition 2 (`d`, `2`) and create it again, answering the `Last sector` prompt with the number that
  `echo $(( (LAST + 1) / 2048 * 2048 - 1 ))` prints, where `LAST` is the larger number in the prompt's range.
- `wipefs` prints nothing. If it prints a line, run `wipefs -a "${POOL_PART}"`.

### Option C only: creating the LUKS container

Skip this section for Options A and B.

Why: the container encrypts everything in partition 2; the pool is created inside it.

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it before
> any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

```
root # cryptsetup luksFormat --type luks2 --sector-size 4096 "${POOL_PART}"
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open --allow-discards --persistent "${POOL_PART}" "luks-${LUKS_UUID}"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
```

- `luksFormat` asks you to type `YES` in capitals, then for the passphrase twice. The other settings are cryptsetup's
  LUKS2 defaults; the passphrase slot uses the slow, memory-hard Argon2id.
- `--sector-size 4096` gives 4 KiB encryption sectors, matching the pool's `ashift=12`. It needs a partition whose
  size is a multiple of 4096 bytes. If `luksFormat` prints "Device size is not aligned to requested sector size.", go
  back to the Check of [Partitioning with gdisk](#partitioning-with-gdisk).
- The container is opened as `luks-<UUID>`, the name systemd, dracut and ZFSBootMenu use for a volume given by UUID.
  Using the same name everywhere keeps the device path recorded in the pool the same in ZFSBootMenu and in Fedora.
- `--allow-discards --persistent` stores the TRIM flag **in the LUKS2 header**, so the container passes TRIM requests
  through wherever it is opened (needed for `autotrim=on`): in ZFSBootMenu, in Fedora's initramfs and on a live
  system, without any crypttab option.

> **Note**
> Passing TRIM through reveals which blocks are unused, but not their content. This is the usual trade-off for SSDs;
> leave out `--allow-discards` if that matters to you, and set `autotrim=off` on the pool.

Optionally, add `--perf-no_read_workqueue --perf-no_write_workqueue` to the `cryptsetup open` line. They process
encryption directly instead of in kernel work queues, which lowers latency on fast NVMe SSDs, and are stored in the
header too.

**Check:**

```
root # cryptsetup luksDump "${POOL_PART}" | grep -E '^Flags|sector'
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
root # echo "${POOL_DEV}"
```

The header shows `Flags: allow-discards` and `sector: 4096 [bytes]`; the status shows `type: LUKS2`,
`sector size: 4096` and `flags: discards` (with the optional flags also `no_read_workqueue no_write_workqueue`).
`POOL_DEV` is `/dev/mapper/luks-` followed by the UUID.

After a reboot of the live system, set the variables and open the container again; the stored flags apply by
themselves:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # export POOL_DEV="/dev/mapper/luks-${LUKS_UUID}"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

### Creating the ZFS pool

Why: the pool holds the whole system. Three steps, in this order: the host ID, the passphrase file (Option B), then
the pool.

**1. The host ID (all options).** ZFS records in the pool the host ID of the system that imported it, and refuses a
normal import by another host while the pool was not exported. ZFSBootMenu, Fedora's initramfs and the running system
all import this pool, so they must agree. Write a fixed host ID now; it is copied into the new system later.

```
root # zgenhostid -f 0x00bab10c
```

**2. Option B: the passphrase file.** `zpool create` reads the passphrase from the file named by `keylocation`, and
the same file later lets Fedora's initramfs load the key without asking.

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it before
> any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

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

The file must stay on the live system until the installation is finished.

**3. The pool.** **Options A and B** set `POOL_DEV` first (Option C did so in the previous section):

```
root # export POOL_DEV="${POOL_PART}"
```

**Options A and C** create the pool like this:

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

**Option B** adds three `-O` lines for the encryption:

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
| `compatibility=openzfs-2.2-linux` | enables only the features OpenZFS 2.2 knows (1) |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (journald and SELinux labels use them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | everything the pool mounts during the installation appears below `/mnt` |
| `encryption=aes-256-gcm` (B) | likely the fastest cipher on modern x86_64 hardware |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file of step 2; also in Fedora's initramfs, inside the pool |

1. So live systems with ZFS 2.2 or later can import the pool. It can be relaxed later
   ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)).

In Option B the pool's root dataset is the encryption root, so every dataset created later inherits the encryption,
including the boot environment with its `/boot`.

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
  `available`; also `ls -l /etc/zfs/zroot.key` shows `----------` owned by root, and `wc -l < /etc/zfs/zroot.key`
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
| `zroot/ROOT/fedora` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, including `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/fedora` | the boot environment ZFSBootMenu boots after the countdown |

- `canmount=noauto` is required on every file system with `mountpoint=/`, and it is not inherited: each boot
  environment you create later needs it set again.
- Keep `/usr`, `/etc` and `/var` in the boot environment: they change together with every package update (the RPM
  database is in `/usr/lib/sysimage/rpm`). Add datasets only for user data (`/srv`, for example), if you want them.

> **Important**
> `/boot` is a directory of the root dataset, not a file system of its own and not the ESP. ZFSBootMenu reads
> Fedora's kernel and initramfs from there, and in Options B and C the initramfs with the key file is then inside the
> encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/fedora`.

### Setting the ZFSBootMenu properties

Why: ZFSBootMenu reads the kernel command line for Fedora from a ZFS property. It is set on `zroot/ROOT`, so every
future boot environment inherits it.

**Options A and B.** Fedora's initramfs needs nothing on the command line: Option B finds the key file through the
dataset's `keylocation`.

```
root # zfs set org.zfsbootmenu:commandline="quiet" "${POOL}/ROOT"
```

**Option C.** Fedora's initramfs unlocks the container from three kernel parameters, without a crypttab. The command
line is built in two steps to keep the lines short:

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

systemd's `systemd-cryptsetup-generator` reads these parameters in the initramfs **and** on the running system. That
suits a single-SSD installation without a crypttab. If you later add other LUKS volumes to `/etc/crypttab` (swap, a
data disk), switch to the forms only the initramfs reads: `rd.luks.crypttab=no`, `rd.luks.uuid=` and `rd.luks.key=`.

**All options.** Do not put `root=` into this property: ZFSBootMenu always adds the root argument itself. For Fedora
it adds `root=zfs:zroot/ROOT/fedora`, which OpenZFS's dracut module reads; there is no need to set
`org.zfsbootmenu:rootprefix`. Do not add `spl.spl_hostid=` either; ZFSBootMenu sets it. Fedora's own installations
usually also have `rhgb`, which shows Plymouth's graphical boot screen; add it to the property if you want that.

**Option B** also names the file system in which ZFSBootMenu may find the key file:

```
root # zfs set org.zfsbootmenu:keysource="${POOL}/ROOT/${BE}" "${POOL}"
```

This is a convenience. When you choose a boot environment **in the menu**, ZFSBootMenu mounts that file system after
the passphrase was typed, finds `etc/zfs/zroot.key` in it and keeps a copy in memory, so that menu operations that
export and re-import the pool (setting the default, cloning a snapshot, switching to read/write) do not ask again. It
does not remove the first prompt.

**Check:**

```
root # zfs get -H -o value org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"
root # zfs get -H -o value org.zfsbootmenu:keysource "${POOL}"
```

The first prints `quiet` (Options A and B), or `quiet luks.crypttab=no luks.uuid=...` with the LUKS UUID twice
(Option C). The second prints `zroot/ROOT/fedora` for Option B and `-` for Options A and C.

### Mounting the pool below /mnt

Why: an export and a fresh import prove that the pool can be opened again, and mount the datasets in the right
order: the root first.

```
root # zpool export "${POOL}"
root # zpool import -N -R /mnt "${POOL}"
```

**Option B** loads the key by typing the passphrase again:

```
root # zfs load-key -L prompt "${POOL}"
```

If it refuses the passphrase, the file holds something else than you believe. Nothing is installed yet: run
`zfs load-key "${POOL}"`, `zpool destroy "${POOL}"`, write the file again and repeat
[Creating the ZFS pool](#creating-the-zfs-pool) from step 2.

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

`-N` imports without mounting; the root dataset is mounted by hand because it is `canmount=noauto`. The cache file is
copied into the new system later, where the `zfs-import-cache` service and OpenZFS's dracut module use it.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot. The
> ZFS tools inside the new system may not match the ZFS kernel module that the live system has loaded.

**Check:**

```
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`/mnt` is `zroot/ROOT/fedora` and `/mnt/home` is `zroot/home`. Option B: the key status is `available` (Options A and
C print `-`).

## Installing the base system

Why: `dnf --installroot` installs a minimal Fedora 45 into the mounted boot environment, using the live system's
package sources.

Give the new system the live system's devices and kernel interfaces, and a `/run` in memory, before installing:
package scripts run inside the new system and need them.

```
root # mkdir -p /mnt/dev /mnt/proc /mnt/sys /mnt/run
root # mount --rbind --make-rslave /dev /mnt/dev
root # mount --rbind --make-rslave /proc /mnt/proc
root # mount --rbind --make-rslave /sys /mnt/sys
root # mount -t tmpfs tmpfs /mnt/run
root # dnf --installroot=/mnt --use-host-config --releasever=45 install -y \
  @core fedora-release glibc-langpack-en NetworkManager systemd-resolved
```

- `--use-host-config` makes DNF use the live system's configuration and repositories; without it, DNF 5 reads them
  from the empty `/mnt` and finds no packages. `--releasever=45` names the release to install.
- `@core` is Fedora's smallest package group; `fedora-release` makes the new system Fedora Linux rather than a
  generic distribution, `glibc-langpack-en` provides English locales, NetworkManager and `systemd-resolved` the
  network and name resolution Fedora uses by default. A desktop can be added after the first boot
  ([Updating the system](#updating-the-system)).
- `--rbind` (not `--bind`) also brings the mounts below `/sys`, among them the EFI variables that `efibootmgr` needs.
  `--make-rslave` keeps the unmounts of the new system's tree from reaching the live system.
- No kernel is installed yet: [Kernel and ZFS](#kernel-and-zfs) installs it after preparing where it goes.

Give the new system a working name resolution for the installation. On Fedora `/etc/resolv.conf` is normally a link
into `/run`, which is empty in the new system until it boots;
[Finalizing the installation](#finalizing-the-installation) restores the link.

```
root # rm -f /mnt/etc/resolv.conf
root # cp -L /etc/resolv.conf /mnt/etc/resolv.conf
```

**Check:**

```
root # grep -E '^(NAME|VERSION_ID)=' /mnt/etc/os-release
root # rpm --root=/mnt -q fedora-release
root # ls /mnt
```

`NAME="Fedora Linux"` and `VERSION_ID=45`, `fedora-release` is installed, and `/mnt` holds a root file system (`etc`,
`usr`, `var` and so on, with `bin`, `lib` and `sbin` as links into `usr`).

### Copying the host ID and the key file

Why: the new system must use the same host ID as the live system that created the pool, the pool's cache file, and
(Option B) the passphrase file.

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

**All options.** Carry the variables into the chroot with a file. For Options A and B the `LUKS_UUID` line is
written empty; that is harmless. The file holds no secret; it is deleted at the end of the installation.

```
root # cat > /mnt/root/install.env <<EOF
export DISK="${DISK}"
export ESP_PART="${ESP_PART}"
export POOL_PART="${POOL_PART}"
export POOL="${POOL}"
export BE="${BE}"
export POOL_DEV="${POOL_DEV}"
export LUKS_UUID="${LUKS_UUID}"
export KERNEL_MAX="${KERNEL_MAX}"
EOF
```

**Check:**

```
root # findmnt /mnt/boot/efi
root # od -A n -t x4 /mnt/etc/hostid
root # ls -l /mnt/etc/zfs/
root # cat /mnt/root/install.env
```

The ESP is mounted, the host ID prints `00bab10c`, `/mnt/etc/zfs` holds `zpool.cache` and, for Option B, `zroot.key`
with mode `----------`, and the file shows the values you set (Option C: with the LUKS UUID; all options: with
`KERNEL_MAX`).

### Chrooting

Why: the rest of the installation runs inside the new system. The devices and kernel interfaces were mounted before
the base system was installed.

```
root # chroot /mnt /bin/bash --login
(chroot) root # source /root/install.env
```

If you leave the chroot and come back, run these two lines again.

**Check:**

```
(chroot) root # echo "${POOL} ${BE} ${DISK} ${KERNEL_MAX}"
(chroot) root # ls /sys/firmware/efi/efivars | head -n 3
(chroot) root # dnf repolist
```

The first prints `zroot fedora /dev/nvme0n1` and the version; the second lists EFI variables; `dnf repolist` lists
`fedora` and `updates` without errors.

## Kernel and ZFS

Why: Fedora's kernel, and the ZFS module built for it with DKMS, make up the system ZFSBootMenu will boot. Before the
kernel is installed, two things are prepared: which kernel versions may be installed, and where they go.

### Holding the kernel at a version OpenZFS supports

Why: OpenZFS's `zfs-dkms` package names the newest kernel series it supports (`KERNEL_MAX`), but Fedora's
repositories may already carry a newer kernel series. DNF 5's version lock keeps every package whose name starts
with `kernel` at or below that version, so DNF installs and updates only kernels the ZFS module can be built for.

First make sure `KERNEL_MAX` is set. DNF skips a lock condition it cannot use, so a lock written with an empty value
would hold back nothing:

```
(chroot) root # [ -n "${KERNEL_MAX}" ] && echo "KERNEL_MAX=${KERNEL_MAX}" || echo "KERNEL_MAX is empty: stop"
```

If it prints `KERNEL_MAX is empty: stop`, leave the chroot, set the variable as in
[Setting variables](#setting-variables), write `/mnt/root/install.env` again and come back. Then write the lock:

```
(chroot) root # cat > /etc/dnf/versionlock.toml <<EOF
version = "1.0"

[[packages]]
name = "kernel*"
comment = "OpenZFS supports kernels up to ${KERNEL_MAX}; raise this after updating OpenZFS"
[[packages.conditions]]
key = "evr"
comparator = "<="
value = "${KERNEL_MAX}"
EOF
```

The file is DNF 5's own format (`dnf versionlock`, see `man dnf5-versionlock`); `<=` with `7.2.999` allows every
7.2 kernel and no 7.3 kernel. The names it matches include `kernel`, `kernel-core`, `kernel-modules`,
`kernel-modules-core`, `kernel-modules-extra`, `kernel-devel` and `kernel-headers`, which all carry the kernel's
version.

**Check:**

```
(chroot) root # dnf versionlock list
(chroot) root # dnf list --available kernel-core
```

`dnf versionlock list` shows the entry for `kernel*` with `evr <= 7.2.999` (your value, never empty). The second
command ignores
the lock (it is not a transaction) and lists the kernels the repositories carry; a version above `KERNEL_MAX` there
is one the lock now holds back.

### Keeping kernels in /boot

Why: ZFSBootMenu finds kernels only as `/boot/vmlinuz-<version>` with `/boot/initramfs-<version>.img` inside the boot
environment. Fedora's kernel packages install the kernel into `/usr/lib/modules/<version>/vmlinuz` and run systemd's
`kernel-install`, which hands the files to plugins. Fedora's usual plugin that copies the kernel to `/boot` comes with
GRUB, which this system does not use, and with the ESP mounted at `/boot/efi`, `kernel-install` may detect the ESP
as the place for boot loader entries. The three files below make the placement explicit.

**FILE** `/etc/kernel/install.conf.d/90-zfsbootmenu.conf`

```
# ZFSBootMenu finds kernels only in /boot of the boot environment, on ZFS.
# Never put kernels or initramfs images on the EFI system partition (/boot/efi).
layout=other
BOOT_ROOT=/boot
```

**FILE** `/etc/kernel/install.d/95-zfsbootmenu.install`

```
#!/bin/sh
# kernel-install plugin for ZFSBootMenu on a ZFS root.
# Puts each kernel and its initramfs into /boot of the boot environment, where ZFSBootMenu
# looks for them, makes the initramfs readable by root only (it may hold a key file),
# and rebuilds ZFSBootMenu's image after the newest kernel was installed.
# Arguments from kernel-install: COMMAND KERNEL-VERSION ENTRY-DIR KERNEL-IMAGE [INITRD...]
COMMAND="$1"
KVER="$2"
KIMAGE="$4"
VMLINUZ="/boot/vmlinuz-${KVER}"
INITRD="/boot/initramfs-${KVER}.img"

case "${COMMAND}" in
add)
    if [ -n "${KIMAGE}" ] && [ "${KIMAGE}" != "${VMLINUZ}" ]; then
        install -m 0755 "${KIMAGE}" "${VMLINUZ}" || exit 1
    fi
    # dracut leaves the initramfs either in /boot or in kernel-install's staging area
    if [ -f "${KERNEL_INSTALL_STAGING_AREA}/initrd" ]; then
        install -m 0600 "${KERNEL_INSTALL_STAGING_AREA}/initrd" "${INITRD}" || exit 1
    fi
    if [ ! -f "${INITRD}" ]; then
        echo "95-zfsbootmenu.install: ${INITRD} is missing" >&2
        exit 0
    fi
    chmod 0600 "${INITRD}"
    command -v restorecon >/dev/null 2>&1 && restorecon "${VMLINUZ}" "${INITRD}" 2>/dev/null
    newest="$(ls /boot/vmlinuz-* 2>/dev/null | sort -V | tail -n 1)"
    if [ "${newest}" = "${VMLINUZ}" ] && [ -x /usr/bin/generate-zbm ]; then
        if modinfo -k "${KVER}" zfs >/dev/null 2>&1; then
            generate-zbm --kver "${KVER}" ||
                echo "95-zfsbootmenu.install: generate-zbm failed; the old image is unchanged" >&2
        else
            echo "95-zfsbootmenu.install: no ZFS module for ${KVER}; ZFSBootMenu's image not rebuilt" >&2
        fi
    fi
    ;;
remove)
    rm -f "${VMLINUZ}" "${INITRD}"
    ;;
esac
exit 0
```

**FILE** `/etc/dracut.conf.d/90-no-rescue.conf`

```
# No rescue kernel and initramfs in /boot: the rescue initramfs would hold the key file
# too, and ZFSBootMenu would list it as a second kernel.
dracut_rescue_image="no"
```

```
(chroot) root # systemd-machine-id-setup
(chroot) root # chmod 0755 /etc/kernel/install.d/95-zfsbootmenu.install
```

How the pieces fit, from systemd 262's `kernel-install(8)` and the sources of dracut and Fedora's GRUB package:

- `layout=other` tells `kernel-install` that no boot loader entries are wanted, so its own plugin
  `90-loaderentry.install`, which would copy kernels under `BOOT_ROOT`, does nothing. `BOOT_ROOT=/boot` keeps every
  plugin away from the ESP even if a boot loader directory appears there later. A drop-in in
  `/etc/kernel/install.conf.d/` overrides Fedora's own `install.conf` without replacing it.
- Plugins run in the order of their names: DKMS's `40-dkms.install` builds the ZFS module for the new kernel,
  dracut's `50-dracut.install` builds the initramfs, and `95-zfsbootmenu.install` runs last. Upstream dracut leaves
  the initramfs in `kernel-install`'s staging area; the dracut that RHEL-like distributions ship writes
  `/boot/initramfs-<version>.img` itself when no boot loader entry directory exists. This guide could not confirm
  which of the two Fedora 45's dracut does; the plugin handles both.
- `systemd-machine-id-setup` gives the new system its machine ID. Without it `kernel-install` makes up a new random
  ID for each run, and dracut's rescue plugin `51-dracut-rescue.install`, which reads it, does nothing.
- The plugin runs `generate-zbm` only after ZFSBootMenu is installed and configured; until then it skips that part.
  It never makes a kernel installation fail because of ZFSBootMenu.

> **Warning**
> Do not install `grub2-efi-x64`, `shim` or `systemd-boot` on this system, and do not run `bootctl install`. They
> install another boot loader on the ESP, and GRUB's `kernel-install` plugin also writes boot loader entries that no
> boot loader reads here.

**Check:**

```
(chroot) root # cat /etc/machine-id
(chroot) root # ls -l /etc/kernel/install.d/ /etc/kernel/install.conf.d/
(chroot) root # sh -n /etc/kernel/install.d/95-zfsbootmenu.install && echo syntax ok
```

The machine ID is 32 hexadecimal characters, the plugin is executable (`-rwxr-xr-x`), and `sh -n` prints
`syntax ok`.

### Installing the kernel and OpenZFS

Why: the kernel, its development files (DKMS needs them), and OpenZFS with its dracut module, in one transaction, so
that the module is built for the new kernel while it is installed.

```
(chroot) root # dnf install -y "https://zfsonlinux.org/fedora/zfs-release-3-1$(rpm --eval '%{dist}').noarch.rpm"
(chroot) root # dnf install -y kernel kernel-devel zfs zfs-dracut
(chroot) root # KVER="$(rpm -q --qf '%{VERSION}-%{RELEASE}.%{ARCH}\n' kernel-core | sort -V | tail -n 1)"
(chroot) root # echo "export KVER=\"${KVER}\"" >> /root/install.env
(chroot) root # source /root/install.env
(chroot) root # systemctl enable zfs-import-cache zfs-mount zfs-import.target zfs.target zfs-zed
```

- The installation takes several minutes. `zfs-dkms` first tries to build the module for the **live system's**
  kernel (`uname -r`). When the live kernel and the installed kernel differ, the live kernel's files are not in the
  new system, and messages such as `Your kernel headers for kernel ... cannot be found` may appear; they do no
  harm. Then the kernel package runs `kernel-install`, and DKMS builds the module for the new kernel.
- `KVER` names the installed kernel version, for example `7.2.15-300.fc45.x86_64`; it is added to
  `/root/install.env` for the Checks that follow.
- The first initramfs, built with the kernel, cannot unlock anything yet;
  [Configuring the initramfs](#configuring-the-initramfs) builds it again.

**Check:**

```
(chroot) root # echo "${KVER}"
(chroot) root # kernel-install inspect "${KVER}" "/usr/lib/modules/${KVER}/vmlinuz"
(chroot) root # ls -l /boot
(chroot) root # ls -A /boot/efi
(chroot) root # dkms status
(chroot) root # modinfo -k "${KVER}" -F version zfs
(chroot) root # systemctl is-enabled zfs-import-cache zfs-mount zfs-import.target zfs.target zfs-zed
(chroot) root # od -A n -t x4 /etc/hostid
```

- `KVER` is one version, not above `KERNEL_MAX`.
- `kernel-install inspect` shows the layout `other`, the boot root `/boot`, and `95-zfsbootmenu.install` among the
  plugins.
- `/boot` holds `vmlinuz-${KVER}` and `initramfs-${KVER}.img`, the initramfs with mode `-rw-------`, and the
  directory `efi`. There is no `loader` directory, no `vmlinuz-0-rescue-*`, and `/boot/efi` is empty.
- `dkms status` lists `zfs/2.4...` (or later) with `${KVER}` as `installed`, and `modinfo` prints the same ZFS
  version. If not, see [The ZFS module is missing for a kernel](#the-zfs-module-is-missing-for-a-kernel).
- `systemctl is-enabled` prints `enabled` five times, and the host ID is `00bab10c`.

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Why: Fedora's initramfs must unlock the container again after `kexec`; a random key in a second key slot lets it do
so without asking.

Install cryptsetup in the new system. It is also what puts LUKS support into ZFSBootMenu's image later.

```
(chroot) root # dnf install -y cryptsetup
(chroot) root # install -d -m 0700 /etc/cryptsetup-keys.d
(chroot) root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot.key
(chroot) root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  "${POOL_PART}" /etc/cryptsetup-keys.d/zroot.key
```

`luksAddKey` asks for the passphrase and adds the file's content as key slot 1. A random key needs no slow key
derivation, so `--pbkdf pbkdf2 --pbkdf-force-iterations 1000` (the smallest count cryptsetup accepts) makes the second
unlock almost instant. The passphrase slot keeps the slow, memory-hard default (Argon2id).

**Check:**

```
(chroot) root # ls -l /etc/cryptsetup-keys.d/zroot.key
(chroot) root # cryptsetup open --test-passphrase --verbose --key-file /etc/cryptsetup-keys.d/zroot.key "${POOL_PART}"
(chroot) root # cryptsetup luksDump "${POOL_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, cryptsetup reports `Key slot 1 unlocked.`, and two key slots (`0` and
`1`) are listed.

## Configuring the initramfs

Why: Fedora's initramfs must contain OpenZFS's dracut module, the NVMe driver and, for Options B and C, the key file,
and it must be readable by root only. Settings go into a file of their own in `/etc/dracut.conf.d/`, never into
`/etc/dracut.conf`.

Write **one** of the three files, for your option.

**Option A.**

**FILE** `/etc/dracut.conf.d/10-zfs.conf`

```
nofsck="yes"
# Do not store kernel parameters detected in the chroot; ZFSBootMenu passes them
hostonly_cmdline="no"
add_dracutmodules+=" zfs "
# Host-only detection does not always see through ZFS to the NVMe SSD
add_drivers+=" nvme "
```

**Option B.**

**FILE** `/etc/dracut.conf.d/10-zfs.conf`

```
nofsck="yes"
# Do not store kernel parameters detected in the chroot; ZFSBootMenu passes them
hostonly_cmdline="no"
add_dracutmodules+=" zfs "
# Host-only detection does not always see through ZFS to the NVMe SSD
add_drivers+=" nvme "
# The ZFS passphrase file named by keylocation; the initramfs lives inside the encrypted pool
install_items+=" /etc/zfs/zroot.key "
```

**Option C.**

**FILE** `/etc/dracut.conf.d/10-zfs.conf`

```
nofsck="yes"
# Do not store rd.luks.* or other parameters detected in the chroot; the luks.* parameters are used
hostonly_cmdline="no"
# ZFS root, and systemd-cryptsetup for the luks.* kernel parameters
add_dracutmodules+=" zfs crypt "
# Host-only detection does not always see through ZFS and LUKS to the NVMe SSD
add_drivers+=" nvme "
# The key file named by luks.key=; the initramfs lives inside the encrypted pool
install_items+=" /etc/cryptsetup-keys.d/zroot.key "
```

Make sure no crypttab is present; the `luks.*` parameters replace it:

```
(chroot) root # [ -e /etc/crypttab ] && mv /etc/crypttab /etc/crypttab.unused
```

**All options.** Build the initramfs again through `kernel-install`, the same way every kernel update will:

```
(chroot) root # kernel-install add "${KVER}" "/usr/lib/modules/${KVER}/vmlinuz"
```

- The files leave dracut's other settings at Fedora's defaults. This guide could not confirm whether Fedora 45's dracut
  builds host-only images by default; either way the image is built on this computer, for its own hardware, and the
  NVMe driver is added explicitly.
- OpenZFS's dracut module also copies `/etc/hostid` and `/etc/zfs/zpool.cache` into the initramfs.
- dracut includes CPU microcode at the start of the same file when the microcode files are installed. ZFSBootMenu
  loads only this one initramfs file per kernel, so microcode must be inside it. Fedora's kernel pulls in
  `linux-firmware`, which has AMD's microcode; Intel's is in `microcode_ctl` (this guide could not confirm both
  package names for Fedora 45; the Check shows what is in the image).

> **Warning**
> `generate-zbm` builds ZFSBootMenu's image with its own dracut directory, but dracut still reads `/etc/dracut.conf`
> and `/usr/lib/dracut/dracut.conf.d/`. Never name a key file in either: it would end up in the ZFSBootMenu image on
> the unencrypted ESP. Keep `install_items` lines for keys in `/etc/dracut.conf.d/` only.

**Check:**

```
(chroot) root # ls -l "/boot/initramfs-${KVER}.img"
(chroot) root # lsinitrd "/boot/initramfs-${KVER}.img" | grep -E 'zfs\.ko|zroot\.key|etc/hostid|zpool\.cache|nvme'
(chroot) root # lsinitrd "/boot/initramfs-${KVER}.img" | grep -E 'systemd-cryptsetup$|cryptsetup-generator'
(chroot) root # lsinitrd -f etc/crypttab "/boot/initramfs-${KVER}.img"
(chroot) root # lsinitrd "/boot/initramfs-${KVER}.img" | grep -i -E 'microcode|ucode'
(chroot) root # grep CONFIG_BLK_DEV_NVME= "/usr/lib/modules/${KVER}/config"
(chroot) root # ls -A /boot/efi
```

| | Option A | Option B | Option C |
|---|---|---|---|
| mode of the initramfs | `-rw-------` | **`-rw-------`** | **`-rw-------`** |
| first `grep` lists | the four files (1) | the same and `etc/zfs/zroot.key` | the same as A and the key file (2) |
| second `grep` lists | anything or nothing | anything or nothing | the two programs (3) |
| `etc/crypttab` | nothing | nothing | **nothing** |

1. `zfs.ko` (possibly with `.xz` or `.zst`), `etc/hostid`, `etc/zfs/zpool.cache` and `nvme`.
2. `etc/cryptsetup-keys.d/zroot.key`.
3. `systemd-cryptsetup` and `systemd-cryptsetup-generator`.

- The microcode `grep` lists `GenuineIntel.bin` or `AuthenticAMD.bin` (in the early part of the image). If it lists
  nothing, install the microcode package for your CPU and run the `kernel-install add` line again.
- `CONFIG_BLK_DEV_NVME=m` means the NVMe driver is a module, which must be listed above; `=y` means it is built into
  the kernel, and then no `nvme.ko` is listed.
- `/boot/efi` prints nothing: no kernel or initramfs was put on the unencrypted ESP.
- Option C: if `systemd-cryptsetup` is missing, `dnf provides /usr/lib/systemd/systemd-cryptsetup` names the package
  that provides it; install it and run the `kernel-install add` line again. It has been part of `systemd-udev` on
  Fedora; this guide could not confirm that for Fedora 45. If the program is installed but still not in the image,
  add `systemd-cryptsetup` to the `add_dracutmodules` line (`" zfs crypt systemd-cryptsetup "`) and run the
  `kernel-install add` line again: in dracut-ng's sources, the `crypt` module does not pull in the
  `systemd-cryptsetup` module, whose check fails in a host-only build that does not see the LUKS device (read in
  the sources, not seen on Fedora).
- If an image for Option B or C is readable by others (`-rw-r--r--`), the plugin of
  [Keeping kernels in /boot](#keeping-kernels-in-boot) did not run; fix it and run the `kernel-install add` line
  again. Never copy such an image out of the pool.

## Configuring the system

Why: the base configuration of any Fedora system: locale, keyboard, time zone, host name, the ESP in `/etc/fstab`,
accounts and services.

Locale, console keyboard, time zone and host name in one command. Replace `KEYMAP` (for example `us` or `de`), `ZONE`
(for example `Europe/Berlin`; `timedatectl list-timezones` on the live system lists them) and `HOSTNAME`:

```
(chroot) root # systemd-firstboot --force --locale=en_US.UTF-8 --keymap=KEYMAP --timezone=ZONE --hostname=HOSTNAME
(chroot) root # echo 'XKBLAYOUT=LAYOUT' >> /etc/vconsole.conf
(chroot) root # kernel-install add "${KVER}" "/usr/lib/modules/${KVER}/vmlinuz"
```

- `systemd-firstboot` writes `KEYMAP=` into `/etc/vconsole.conf`, which the console of Fedora's initramfs uses.
  `XKBLAYOUT=` names the same layout in XKB's terms (replace `LAYOUT`, for example `us` or `de`): `localectl` reports
  it, and the Rust runtime of ZFSBootMenu reads it ([Choosing the runtime](#choosing-the-runtime)).
- Fedora has proposed replacing the kernel's text console with **kmscon**, which takes its layout from `localectl`'s
  XKB settings rather than from `KEYMAP=`. This guide could not confirm whether Fedora 45's `@core` installs kmscon;
  [First boot](#first-boot) shows how to check.
- The `kernel-install add` line builds Fedora's initramfs again, so that it carries `/etc/vconsole.conf`: a
  passphrase prompt or emergency shell in it then uses your keyboard layout.

The ESP in `/etc/fstab`. Only the ESP goes there; ZFS mounts its datasets itself:

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

The root password, and a user for daily work (replace `USERNAME`) who may use `sudo` (group `wheel`):

```
(chroot) root # passwd
(chroot) root # useradd -m -G wheel USERNAME
(chroot) root # passwd USERNAME
```

Services. The network: NetworkManager configures wired interfaces with DHCP by itself; for Wi-Fi, use `nmtui` or
`nmcli` after the first boot. Name resolution: `systemd-resolved`. Time: Fedora's `chrony` from `@core`, if installed.

```
(chroot) root # systemctl enable NetworkManager.service systemd-resolved.service
```

**Check:**

```
(chroot) root # cat /etc/locale.conf /etc/vconsole.conf /etc/hostname
(chroot) root # lsinitrd -f etc/vconsole.conf "/boot/initramfs-${KVER}.img"
(chroot) root # readlink /etc/localtime
(chroot) root # findmnt --verify --tab-file /etc/fstab
(chroot) root # id USERNAME
(chroot) root # systemctl is-enabled NetworkManager.service systemd-resolved.service
```

`LANG=en_US.UTF-8`, `KEYMAP=` and `XKBLAYOUT=` with your layout (in the file and, from `lsinitrd`, in the
initramfs; dracut's `i18n` module copies it in host-only mode, and this guide could not confirm what it does
otherwise), the host name, a link into `/usr/share/zoneinfo/` with your zone,
no errors from `findmnt --verify` for the ESP line, the user in group `wheel`, and `enabled` twice. If
`systemd-firstboot` reported an error for one setting, set that one alone again (for example
`systemd-firstboot --force --keymap=us`).

## Installing ZFSBootMenu

Everything in this section runs **inside the chroot**, as root. ZFSBootMenu is built from this repository, because
upstream ZFSBootMenu cannot unlock LUKS volumes; for the same steps in all three options, Options A and B use this
build too.

### Dependencies

Why: the build needs a Rust compiler, and the image needs the tools that go into it.

```
(chroot) root # dnf install -y git make gcc cargo rust binutils dracut systemd-boot-unsigned \
  kexec-tools fzf mbuffer less efibootmgr dosfstools util-linux kmod procps-ng ncurses \
  xkeyboard-config libX11-common
```

| Package | Why |
|---|---|
| `git`, `make` | fetch the source and run the build |
| `gcc` | the C compiler and linker Rust needs to link the programs |
| `cargo`, `rust` | Fedora's Rust toolchain; this repository needs `rustc` 1.88 or later |
| `binutils` | `strip` for `make install`, `strings` for `generate-zbm`, `objcopy` for a Check |
| `dracut` | builds ZFSBootMenu's image (already installed as Fedora's initramfs generator) |
| `systemd-boot-unsigned` | the EFI stub `/usr/lib/systemd/boot/efi/linuxx64.efi.stub`; it installs no boot loader |
| `kexec-tools` | starts Fedora's kernel from ZFSBootMenu |
| `fzf` | the menu of the default (shell) runtime |
| `mbuffer`, `less` | optional tools of the image; without them the build warns (`column` is in `util-linux`) |
| `efibootmgr` | the firmware boot entries |
| `dosfstools`, `util-linux`, `kmod`, `procps-ng`, `ncurses` | tools of the image; a Fedora system has most of them |
| `xkeyboard-config`, `libX11-common` | keyboard layouts and compose rules for the optional Rust runtime |

If `dnf` stops with `No match for argument` for one of the optional names (`mbuffer`, `xkeyboard-config`,
`libX11-common`), run the command again without it; the build only warns. For `systemd-boot-unsigned`, find the
package that has the stub with `dnf provides /usr/lib/systemd/boot/efi/linuxx64.efi.stub`.

**Option C.** `cryptsetup` is already installed; it is what puts LUKS support into ZFSBootMenu's image. A dracut build
without it leaves LUKS out without a message.

**Check:**

```
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # cargo --version && rustc --version
(chroot) root # command -v dracut lsinitrd kexec fzf efibootmgr strings objcopy
```

The stub exists, `rustc` is 1.88 or later, and every program is found. Option C: `command -v cryptsetup dmsetup
udevadm` finds all three.

### Building and installing

Why: `make` builds the programs `zbm`, `brush`, `generate-zbm` and `zbm-kcl`; `make install` installs them with the
dracut module and the configuration.

```
(chroot) root # git clone --single-branch --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2 /usr/src/zbm
(chroot) root # make -C /usr/src/zbm
(chroot) root # make -C /usr/src/zbm install
(chroot) root # du -sh /usr/src/zbm/target && rm -rf /usr/src/zbm/target
```

The last line removes Cargo's build directory, which is no longer needed once the programs are installed. `/usr/src`
is part of the boot environment, so the build directory would otherwise be kept in the `@install` snapshot and in
every later one. The next `make` builds from scratch.

> **Warning**
> The repository's default branch, `master`, holds the upstream layout without the LUKS code. Always use
> `claude/zbm-refactor`. Do not run `make core dracut`, which older guides show: it installs neither `zbm` nor
> `generate-zbm`.

- The Rust crates are in the clone (`vendor/`), so the build needs no network. The repository's `rust-toolchain.toml`
  names a newer compiler, but only `rustup` reads that file; Fedora's `cargo` builds with its own `rustc`.
- `make` runs `cargo build --release --locked`; it takes several minutes.
- `make install` puts `generate-zbm` and `zbm-kcl` in `/usr/bin`; `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
  the shell runtime in `/usr/share/zfsbootmenu`; the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in
  `/usr/lib/dracut/modules.d`; the mkinitcpio hooks in `/usr/lib/initcpio`; `config.yaml`, `mkinitcpio.conf` and
  `dracut.conf.d/` in `/etc/zfsbootmenu`; manual pages; and examples in `/usr/share/examples/zfsbootmenu`.
- `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files it
  installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files with other names there are kept. This guide therefore writes
  `config.yaml` after `make install`, and keeps its own dracut settings in files of its own.
- The dracut module `zfsbootmenu-measured` is also included in Fedora's own initramfs from now on. It only copies
  the TPM event log that images with the Rust runtime pass on, and does nothing otherwise.

**Check:**

```
(chroot) root # git -C /usr/src/zbm branch --show-current
(chroot) root # generate-zbm --showver
(chroot) root # ls -l /usr/bin/generate-zbm /usr/bin/zbm-kcl /usr/libexec/zfsbootmenu/
(chroot) root # ls /usr/lib/dracut/modules.d/ | grep zfsbootmenu
```

The branch is `claude/zbm-refactor`, `generate-zbm` prints a version, `zbm` and `brush` are in
`/usr/libexec/zfsbootmenu`, and both dracut modules are listed.

### Configuring generate-zbm

Why: `generate-zbm` builds ZFSBootMenu's image as one EFI file on the ESP; its configuration says where and with
which command line.

**Options A and B:**

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

**Option C** (the shell fills in the LUKS UUID, so `EOF` must not be quoted):

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

| Key | Value | Meaning |
|---|---|---|
| `Global.ManageImages` | `true` | `generate-zbm` does nothing while this is `false`, the installed default |
| `Global.BootMountPoint` | `/boot/efi` | the ESP; `generate-zbm` mounts it from `/etc/fstab` if needed |
| `Global.DracutConfDir` | `/etc/zfsbootmenu/dracut.conf.d` | dracut settings of **ZFSBootMenu's** image (1) |
| `Global.PreHooksDir`, `Global.PostHooksDir` | as shown | programs run before and after a build |
| `Global.InitCPIOConfig` | as shown | used only with mkinitcpio; kept from the installed file |
| `Components.Enabled` | `false` | no separate kernel and initramfs; only the EFI file |
| `EFI.Enabled` | `true` | build the EFI file: kernel, initramfs and command line in one |
| `EFI.ImageDir` | `/boot/efi/EFI/zbm` | where the EFI file is written |
| `EFI.Versions` | `false` | one image and one `-backup` copy with fixed names, so the firmware entries never change |
| `Kernel.Prefix` | `vmlinuz` | fixes the image names to `vmlinuz.EFI` and `vmlinuz-backup.EFI` (2) |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's own** kernel, built into the EFI file |

1. Never the system's `/etc/dracut.conf.d`.
2. Fedora's kernel name would give the same; the key keeps it so.

| Option | `Kernel.CommandLine` |
|---|---|
| A, B | `quiet loglevel=0 zbm.luks=off` |
| C | `quiet loglevel=0 zbm.luks=UUID=<LUKS UUID>` |

- Options A and B: `zbm.luks=off` means: do not unlock any LUKS volume. Without it ZFSBootMenu would offer every
  LUKS volume it finds, for example on a USB disk plugged in at boot.
- Option C: unlock only this container. The UUID is that of the **LUKS container** (`cryptsetup luksUUID`), not the
  pool's GUID and not a file system UUID; it is opened as `luks-<UUID>`.

Options you may add later: `zbm.timeout=SECONDS` changes the countdown (default 10; `-1` always shows the menu, `0`
boots at once); `loglevel=7` shows ZFSBootMenu's messages for troubleshooting.

ZFSBootMenu's dracut configuration: build an image that does not depend on the machine it was built on. The first
image is built inside a chroot on the live system, whose kernel and loaded drivers are not those of the installed
system, and dracut's host-only mode picks drivers from the running system. The project's build container does the
same.

**FILE** `/etc/zfsbootmenu/dracut.conf.d/generic.conf`

```
# Build an image that does not depend on the machine it was built on: the first image is built
# inside a chroot on a live system, whose kernel and loaded drivers are not those of the installed system.
hostonly="no"
```

The image is larger this way; the 1 GiB ESP has room. Do not add dracut's `crypt` module here: ZFSBootMenu unlocks
LUKS volumes itself.

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

The **shell runtime** (Bash, with the `fzf` menu) is the default and is enough for all three options; skip this
section to keep it. The **Rust runtime** (the program `zbm`, with a new menu) is opt-in and will replace the shell
runtime after a beta. Only the Rust runtime has the locked menu, measured boot, TPM2 PIN tokens that systemd 262
enrolled with `--tpm2-with-pin=yes` (see
[Option C only, optional: TPM2 with a PIN](#option-c-only-optional-tpm2-with-a-pin)), and keyboard layouts other
than US at the passphrase prompt. To choose it:

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

Keyboard: with the shell runtime the passphrase prompt uses the US layout. With the Rust runtime, either add
`zbm.console=drm` to `CommandLine` (the console on the display then types with the image's first layout), or press
**Ctrl-K** at the prompt to choose a layout. The image gets the layouts of `zfsbootmenu_keyboard_layouts`; when it is
unset, the layout of `XKBLAYOUT` in `/etc/vconsole.conf` (written in
[Configuring the system](#configuring-the-system)), then `us`, `de`, `fr` and `gb`. To name the layouts yourself:

```
(chroot) root # echo 'zfsbootmenu_keyboard_layouts="de us"' >> /etc/zfsbootmenu/dracut.conf.d/runtime.conf
```

`zbm.console=drm` needs the kernel's `evdev` module and the display's DRM driver in the image; without them
ZFSBootMenu stays on the kernel console without a message. The installed `omit-drivers.conf` leaves `drm`, `i915`
and `amdgpu` out; a driver named in `add_drivers` is taken off that list. Add them in a file of your own, which
`make install` does not overwrite, for example `/etc/zfsbootmenu/dracut.conf.d/console.conf` with
`add_drivers+=" evdev "`, plus `drm` and your display's DRM driver (such as `i915`, `amdgpu` or `simpledrm`) when
Fedora's kernel builds them as modules; `grep -E 'CONFIG_DRM(_I915|_AMDGPU|_SIMPLEDRM)?=' /usr/lib/modules/*/config`
shows `=m` for a module. This guide has not built such an image on Fedora.

**Check:**

```
(chroot) root # cat /etc/zfsbootmenu/dracut.conf.d/*.conf | grep -E 'hostonly|zfsbootmenu_'
```

`hostonly="no"`, and `zfsbootmenu_runtime=rust` only if you chose it.

### Building and checking the image

Why: build the image, then build it again so that the backup entry has an image to start from the beginning.

```
(chroot) root # generate-zbm --dry-run --debug
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

- `--dry-run` prints one `Would ...` line for each change and makes none; it names the kernel it would use
  (`/boot/vmlinuz-${KVER}`). `--debug` prints every command.
- The first run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`; the second renames it to `vmlinuz-backup.EFI` and writes a
  new one.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | `make install` was not run |
| `failed to install essential executable '<name>'` | a required program is missing (1) |
| `Unable to find UEFI stub loader at default locations` | `systemd-boot-unsigned` is not installed |
| `ManageImages not enabled, no action taken` | `config.yaml` was overwritten by `make install`; write it again |
| `Unable to locate libgcc_s.so` | the GCC runtime library is missing (`libgcc`) |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (2) |

1. Often `kexec`, or `fzf` for the shell runtime.
2. The SPL module's `spl_hostid` parameter differs from `/etc/hostid` (the check is skipped while the parameter is
   0). The image takes `/etc/hostid`, which must be `00bab10c` (`od -A n -t x4 /etc/hostid`).

**Check** the image, without a reboot:

```
(chroot) root # ls -l /boot/efi/EFI/zbm/
(chroot) root # df -h /boot/efi
(chroot) root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
(chroot) root # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/cryptsetup$|etc/hostid|zfs\.ko|nvme'
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'cryptsetup-keys\.d|\.key$|etc/zfsbootmenu/crypttab'
(chroot) root # rm /run/zbm-initrd.img
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `ls` | `vmlinuz.EFI` and `vmlinuz-backup.EFI` | the same | the same |
| `zbm-kcl` prints | `quiet loglevel=0 zbm.luks=off` | the same | `quiet loglevel=0 zbm.luks=UUID=` and the UUID |
| first `grep` lists | `etc/hostid`, `zfs.ko` (possibly compressed), `nvme` | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

With the Rust runtime, `zbm-kcl` also prints `init_on_free=1`. The second `grep` must print nothing: no key file
and no crypttab may be in ZFSBootMenu's image. If it prints a line, do not boot this image; find out why first
(a key file named in `/etc/dracut.conf` or in `/usr/lib/dracut/dracut.conf.d/`). If `cryptsetup` is listed for
Options A or B, that is fine: it is in the image whenever the package is installed, and `zbm.luks=off` keeps it
unused. The `nvme` line lists the driver module only if Fedora's kernel builds it as a module (see the
`CONFIG_BLK_DEV_NVME` Check of
[Configuring the initramfs](#configuring-the-initramfs)). `df` shows room for at least one more copy of the image,
which the next step makes.

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

Why: restore Fedora's usual name resolution, mark the file system for an SELinux relabel, take a snapshot of the
fresh system to return to, and export the pool cleanly, which every later import expects.

The files of the new system were written from the live system, so most of them carry no SELinux label or a wrong
one. `fixfiles -F onboot` makes the first boot relabel every file and then reboot once by itself, as the OpenZFS
documentation for Fedora does. Fedora's SELinux policy labels ZFS file systems from their extended attributes, like
ext4 or XFS.

```
(chroot) root # ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
(chroot) root # fixfiles -F onboot
(chroot) root # rm /root/install.env
(chroot) root # exit
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

The snapshot `@install` contains the relabel request too, so a rollback to it relabels again at the next boot.

**Check:**

```
root # zpool list
root # ls /dev/mapper
```

`zpool list` reports *no pools available*. Option C: `/dev/mapper` contains only `control` (and the live system's
own devices, such as `live-rw` or `live-base`, if it uses them). If the export fails because the pool is busy, leave
any shell that is still inside `/mnt` and repeat the unmount and export commands.

Then reboot and remove the live medium:

```
root # reboot
```

## First boot

From here on, `root #` means root on the **installed system**.

What you see with the default (shell) runtime:

| Step | Option A | Option B | Option C |
|---|---|---|---|
| 1 | | | the LUKS prompt (1); type the LUKS passphrase |
| 2 | the countdown (2) | the same | the same |
| 3 | | the screen clears; `Enter passphrase for 'zroot':`; type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Fedora's kernel | the same | the same |
| 5 | Fedora relabels the file systems (3) and reboots by itself | the same | the same |
| 6 | steps 1 to 4 again; Fedora starts to the login prompt | the same, no second prompt | the same, no second prompt |

1. `Unlock LUKS volume <device>` and `Passphrase (leave empty to skip):`.
2. `Booting zroot/ROOT/fedora in 10 seconds`, with `[RETURN] boot now` and `[ESCAPE] boot menu`.
3. This takes a few minutes.

- **Enter** boots at once; **Escape** during the countdown opens the menu, which lists `zroot/ROOT/fedora`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: `zfs load-key` allows three attempts. After three wrong passphrases ZFSBootMenu opens its emergency shell
  with `unable to load key for zroot/ROOT/fedora`. Type `exit` there; the menu then asks again.
- With the Rust runtime the same questions appear as dialogs (`Unlock <volume>` for Option C, a ZFS key dialog for
  Option B), and the countdown reads `Booting <environment> in <N> s. Press any key to choose another.`
- The relabel happens once. Later boots go straight from step 4 to the login prompt.

If Fedora asks for a passphrase a second time, or stops in an emergency shell, see
[The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock].

Log in as root and check the system:

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status zroot
root # hostid
root # getenforce
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
root # systemctl --failed
root # localectl status
root # rpm -q kmscon
```

**Option B:**

```
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/fedora
```

**Option C:**

```
root # LUKS_UUID="$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)"
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `/proc/cmdline` | the three parameters (1) | the same | the same, with the `luks.*` parameters (2) |
| `/` | `zroot/ROOT/fedora` | the same | the same |
| `zpool status` | `ONLINE`, on `nvme0n1p2` | the same | `ONLINE`, on `luks-<UUID>` |
| `hostid` | `00bab10c` | the same | the same |
| `getenforce` | `Enforcing` | the same | the same |
| encryption | | see (3) | `type: LUKS2`, `sector size: 4096`, `flags: discards` |
| asked for the passphrase | never | once per boot, by ZFSBootMenu | once per boot, by ZFSBootMenu |
| `systemctl --failed` | `0 loaded units listed` | the same | the same |

1. `quiet`, `root=zfs:zroot/ROOT/fedora` and `spl.spl_hostid=0x00bab10c`.
2. `luks.crypttab=no`, `luks.uuid=` and `luks.key=`.
3. `zroot`: `aes-256-gcm`, encryption root `zroot`, `file:///etc/zfs/zroot.key`, `available`.
   `zroot/ROOT/fedora`: `aes-256-gcm`, encryption root `zroot`, key location `none` (only an encryption root has
   one), `available`.

`localectl status` shows your layout as `VC Keymap` and `X11 Layout`. If `rpm -q kmscon` reports it installed, the
text consoles use kmscon, which takes the `X11 Layout`; if a console types with the wrong layout, run
`localectl set-x11-keymap LAYOUT` and reboot. This guide could not confirm whether Fedora 45 installs kmscon.

`BootCurrent` names the entry the firmware used. If it is the number of the `ZFSBootMenu` entry, the firmware honours
entries and `\EFI\BOOT\BOOTX64.EFI` is only a reserve. If it is another number, the firmware started the copy; the
post-hook `50-bootx64.sh` of [Creating the firmware boot entries](#creating-the-firmware-boot-entries) keeps it
current, so check after each `generate-zbm` that it ran (see
[Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image)).

Check the network and the kernel set-up once more on the running system:

```
root # resolvectl status | head -n 5
root # dnf versionlock list
root # kernel-install inspect | grep -i -E 'layout|boot root'
```

Name resolution works through `systemd-resolved`, the kernel lock is in place, and the layout is `other` with the
boot root `/boot`.

Once the system has booted, do these three things:

1. **Option C:** make the first LUKS header backup:
   [Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header).
2. Enable the monthly scrub and TRIM: [Scrubbing and trimming](#scrubbing-and-trimming).
3. Keep the live medium until the system has booted several times, including once after a kernel update.

## Usage

### Updating the system

Take a snapshot of the boot environment before large updates;
[Boot environments, snapshots and rollback](#boot-environments-snapshots-and-rollback) shows how to go back:

```
root # zfs snapshot zroot/ROOT/fedora@pre-update-$(date +%Y%m%d)
root # dnf upgrade --refresh
```

What happens on its own, and what you must do:

| Update | What happens | What you do |
|---|---|---|
| a **kernel** within the lock | `kernel-install` runs (1) | check, below |
| **OpenZFS** (`zfs`, `zfs-dkms`, `zfs-dracut`) | module for the running kernel only (2) | `kernel-install add-all` |
| **cryptsetup** (Option C) | nothing for ZFSBootMenu's image | `generate-zbm` |
| **dracut**, **systemd** | nothing | optionally `kernel-install add-all` to use them at once |

1. DKMS builds the ZFS module, dracut builds the initramfs with your configuration (key file included), and
   `95-zfsbootmenu.install` puts both into `/boot`, sets mode `0600` and runs `generate-zbm`.
2. The `zfs-dkms` package removes the old module from **every** kernel and builds the new one only for the running
   kernel. Run `kernel-install add-all` before rebooting or choosing another kernel.

After an OpenZFS update, rebuild everything for every installed kernel with one command. `kernel-install add-all`
runs the same plugins as a kernel installation for each kernel in `/usr/lib/modules`: DKMS builds the new module,
dracut builds the initramfs, and the plugin runs `generate-zbm` once, for the newest kernel:

```
root # kernel-install add-all
```

**Check** after every update that brought a kernel or OpenZFS, before rebooting:

```
root # dkms status
root # for k in /boot/vmlinuz-*; do v="${k#/boot/vmlinuz-}"; echo "${v}: $(modinfo -k "${v}" -F version zfs)"; done
root # ls -l /boot/initramfs-*.img /boot/efi/EFI/zbm/
```

Every kernel shows the installed ZFS version, every initramfs is `-rw-------`, and `vmlinuz.EFI` has the time of the
update. If a kernel has no module, see [The ZFS module is missing for a kernel](#the-zfs-module-is-missing-for-a-kernel)
before you reboot.

**Moving to a newer kernel series.** The lock of
[Holding the kernel at a version OpenZFS supports](#holding-the-kernel-at-a-version-openzfs-supports) holds Fedora's
kernel at the newest series the installed OpenZFS supports. To see whether newer kernels are being held back, and
what OpenZFS supports now:

```
root # dnf list --upgrades 'kernel*'
root # rpm -q --requires zfs-dkms | grep 'kernel-devel <='
root # dnf versionlock list
```

`dnf list --upgrades` ignores the lock, so it shows the kernels the lock holds back. When an OpenZFS update raised
the `kernel-devel <=` limit above the locked value, raise the lock to the new limit and update again:

```
root # KERNEL_MAX="$(rpm -q --requires zfs-dkms | awk '$1 == "kernel-devel" && $2 == "<=" {print $3; exit}')"
root # sed -i "s/^value = .*/value = \"${KERNEL_MAX}\"/" /etc/dnf/versionlock.toml
root # sed -i "s/up to [^;]*;/up to ${KERNEL_MAX};/" /etc/dnf/versionlock.toml
root # dnf versionlock list
root # dnf upgrade --refresh
```

Never remove the lock while a kernel series is newer than OpenZFS supports. `zfs-dkms` conflicts with such a
kernel, so `dnf` would refuse or skip the kernel update, and other transactions may stop on the conflict (see
[dnf refuses a kernel or ZFS update](#dnf-refuses-a-kernel-or-zfs-update)). The lock keeps those kernels out of
DNF's transactions.

**A desktop.** The installation has no graphical desktop. To install Fedora Workstation's GNOME desktop on top:

```
root # dnf install -y @workstation-product-environment
root # systemctl set-default graphical.target
```

**Check** that the desktop brought no boot loader with it; this guide could not confirm what the group pulls in:

```
root # rpm -q grub2-common grub2-efi-x64 shim-x64 systemd-boot
root # kernel-install inspect | grep -i layout
root # ls /boot/efi/EFI /boot/loader
```

`rpm` reports every package as `not installed`, the layout is `other`, `/boot/efi/EFI` holds only `BOOT` and `zbm`,
and `/boot/loader` does not exist. If a boot loader was installed, remove it with `dnf remove` (keeping the packages
the desktop needs), and check [Keeping kernels in /boot](#keeping-kernels-in-boot) again.

**A new Fedora release.** Upgrading to Fedora 46 with `dnf system-upgrade` needs the OpenZFS repository for Fedora 46
and an OpenZFS that supports Fedora 46's kernel. This guide does not cover it. Take a snapshot first; ZFSBootMenu can
boot it if the upgrade fails.

### Regenerating the ZFSBootMenu image

ZFSBootMenu's image contains the kernel, the ZFS module and tools, cryptsetup and the other tools of **this system at
the time of the build**. Rebuild it after updates of the ZFS packages, of the kernel, of cryptsetup, and after
changing `/etc/zfsbootmenu/`:

```
root # generate-zbm
```

The plugin `/etc/kernel/install.d/95-zfsbootmenu.install` already runs `generate-zbm` after each new kernel and after
`kernel-install add-all`. It is this guide's own: the repository has no hook for Fedora, and it has not been run by
the project.

- Each run keeps the previous image as `vmlinuz-backup.EFI`, which the `ZFSBootMenu (Backup)` entry boots. The backup
  is replaced by the previous image at every build, so after two bad builds in a row it is bad too. A copy under
  another name with its own entry survives that:

  ```
  root # cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/zbm/known-good.EFI
  root # efibootmgr -c -d /dev/nvme0n1 -p 1 -L "ZFSBootMenu (known good)" -l '\EFI\ZBM\KNOWN-GOOD.EFI'
  ```

  `efibootmgr -c` puts that entry first; restore the order with `efibootmgr -o` (the numbers of `ZFSBootMenu`,
  `ZFSBootMenu (Backup)`, then the new one, separated by commas).
- `generate-zbm` uses the newest kernel in `/boot` and needs the ZFS module for it. Rebuild **before** running
  `zpool upgrade` or raising `compatibility`.
- The post-hook `50-bootx64.sh` refreshes the firmware's fallback copy `\EFI\BOOT\BOOTX64.EFI` after every build.
  `cmp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI` prints nothing when the two are the same;
  `generate-zbm --debug` also prints `## Processing hook: ...50-bootx64.sh`. The `known-good.EFI` copy above is never
  refreshed; that is its purpose.

**Check** the plugin by running it as a kernel update would, for the newest installed kernel:

```
root # KVER="$(ls /usr/lib/modules | sort -V | tail -n 1)"
root # kernel-install --verbose add "${KVER}" "/usr/lib/modules/${KVER}/vmlinuz"
root # ls -l /boot/efi/EFI/zbm/ "/boot/initramfs-${KVER}.img"
```

The verbose output names `40-dkms.install`, `50-dracut.install` and `95-zfsbootmenu.install`, `vmlinuz.EFI` has the
current time, and the initramfs is `-rw-------`.

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Snapshots of
it are taken on the running system (`zfs snapshot zroot/ROOT/fedora@NAME`) or in ZFSBootMenu.

To get into the menu, press **Escape** during the countdown (shell runtime) or any key (Rust runtime).

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
| **MOD+C** | clone | a new boot environment that depends on the snapshot; to boot an earlier state for a while |
| **MOD+X** | clone and promote | a new boot environment that keeps the snapshots; the old one can be destroyed |
| **Enter** | duplicate | an independent copy (`zfs send`, `zfs recv`); uses as much space as the snapshot refers to |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

**Rust runtime.** Press **2** or **s** for the snapshots of the selected environment, then **r** to roll back; a
review page shows what is lost, and the change is made once the target's name is typed. **?** lists the keys of
every view.

Example: going back after a bad update.

1. Before the update, on the running system: `zfs snapshot zroot/ROOT/fedora@pre-update`.
2. After a bad update, reboot, press **Escape** in the countdown, select `zroot/ROOT/fedora`, press **MOD+S**, select
   `@pre-update`, press **MOD+R** and confirm.
3. Or, to try the old state without losing the new one, press **MOD+C** (clone) instead and boot the clone.

- Rollback and the other changes need the pool imported read/write; ZFSBootMenu imports read-only and re-imports when
  needed. In Option B that re-import needs the key again; the key source set in
  [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties) saves the second prompt.
- A rollback of the boot environment does not touch `zroot/home`.
- A boot environment you create by hand needs `mountpoint=/` and `canmount=noauto`; it inherits
  `org.zfsbootmenu:commandline` from `zroot/ROOT`.
- Snapshots and clones contain their `/boot`, so each boots with the kernel and initramfs (and, in Options B and C,
  the key file) it had when the snapshot was taken. After the key file or the passphrase was replaced, an older
  snapshot's initramfs holds the old one, and its system asks for the passphrase or stops in an emergency shell.
- The kernel lock and the `kernel-install` set-up are files in the boot environment too: a rollback brings back the
  ones of the snapshot.

On the running system `zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the
`org.zfsbootmenu:commandline` property that every boot environment inherits; see `man zbm-kcl`. Without a dataset
name `zbm-kcl` acts on the root file system, `zroot/ROOT/fedora`, and a change there sets a property of its own on
that boot environment only.

### Changing the passphrase

**Option A.** Nothing to change.

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
> Rebuild Fedora's initramfs now. It holds a copy of the old file, and OpenZFS's dracut module does not fall back to
> a prompt for a `file://` key location: with the old file the next boot stops in an emergency shell.

```
root # kernel-install add-all
root # ls -l /boot/initramfs-*.img
root # for i in /boot/initramfs-*.img; do \
  lsinitrd -f etc/zfs/zroot.key "$i" | cmp - /etc/zfs/zroot.key && echo "$i ok"; done
```

Every image is still `-rw-------`, and the loop prints `ok` for every image: each holds the new file. If an image
is not `ok`, run `kernel-install add-all` again before rebooting. ZFSBootMenu's image holds no key; `add-all`
rebuilds it as well, which does no harm.

**Option C.**

```
root # cryptsetup luksChangeKey /dev/disk/by-partlabel/zroot-luks
```

It asks for the old passphrase, then the new one twice. The key file's slot is not affected, and no image needs
rebuilding. Make a new header backup afterwards: old backups still accept the old passphrase. A second passphrase as
a reserve: `cryptsetup luksAddKey /dev/disk/by-partlabel/zroot-luks`.

### Replacing the key file

**Option A.** There is none.

**Option B.** The key file **is** the passphrase; [Changing the passphrase](#changing-the-passphrase) replaces both.

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
root # kernel-install add-all
root # for i in /boot/initramfs-*.img; do \
  lsinitrd -f etc/cryptsetup-keys.d/zroot.key "$i" | cmp - zroot.key && echo "$i ok"; done
```

`luksAddKey` asks for the passphrase. Rebuilding the initramfs is required: it contains a copy of the key file.
Continue only when the loop prints `ok` for every image and `ls -l /boot/initramfs-*.img` shows them all
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

Skip this section for Options A and B. (Option B has no separate header: the wrapped keys are stored in the pool's
metadata.)

A damaged LUKS header makes the data unrecoverable, even with the right passphrase. Store a header backup **away
from this SSD**:

```
root # cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks \
  --header-backup-file /PATH/ON/USB/zroot-luks-header.img
```

- Restore: `cryptsetup luksHeaderRestore /dev/disk/by-partlabel/zroot-luks --header-backup-file FILE`.
- A header backup holds the key slots as they were: whoever has it and an old passphrase (or the old key file) can
  open the container. Keep it offline, for example on an encrypted USB stick, and make a new one after every change
  of passphrase, key file or token.

### Option C only, optional: TPM2 with a PIN

Skip this section for Options A and B, and if you are content with the passphrase.

ZFSBootMenu can unlock the container with the computer's TPM 2.0 and a PIN instead of the passphrase, through a
token that `systemd-cryptenroll` writes into the LUKS2 header. The passphrase stays in key slot 0 as a fallback.
[ZFS Pools on LUKS Volumes](../../general/luks.rst) describes the method in full; the points that matter on
Fedora 45, whose systemd is 262:

- With systemd 262 and later, `--tpm2-with-pin=yes` hardens the PIN with **Argon2id**, sized by a benchmark to take
  about 2 seconds on the enrolling machine, with up to half its memory. ZFSBootMenu derives it at every boot, on one
  core; fix the costs when enrolling to keep that short.
- Only the **Rust runtime** opens such tokens ([Choosing the runtime](#choosing-the-runtime)). The shell runtime, and
  initramfs images with systemd older than 262, cannot read Argon2id tokens; for them, enroll with
  `--tpm2-with-pin=direct`, which keeps the older PBKDF2 hardening.
- PCR 7 (the Secure Boot state) stays the same across ZFSBootMenu, kernel and initramfs updates. Secure Boot is
  disabled on this system, and changing that later changes PCR 7: enroll again then. `--tpm2-pcrs=` (no PCRs)
  together with a PIN also works on systems without Secure Boot.
- The image gets the TPM2 tools only if they are installed when it is built.

With the Rust runtime chosen:

```
root # dnf install -y tpm2-tools
root # systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 --tpm2-with-pin=yes \
  --tpm2-argon2id-memory=1G --tpm2-argon2id-iterations=3 /dev/disk/by-partlabel/zroot-luks
root # generate-zbm
```

`systemd-cryptenroll` asks for an existing passphrase, then for the new PIN twice.

**Check:**

```
root # cryptsetup luksDump /dev/disk/by-partlabel/zroot-luks | grep -A 3 -i tokens
```

A `systemd-tpm2` token is listed. At the next boot ZFSBootMenu asks for the PIN instead of the passphrase. The second
unlock, by Fedora's initramfs, is meant to use the key file named by `luks.key=` as before. This guide could not
confirm whether systemd-cryptsetup in Fedora's initramfs tries the new token first. If Fedora asks for the PIN a
second time after `kexec`, type it; to go back to the passphrase alone, remove the token and rebuild the image:

```
root # systemd-cryptenroll --wipe-slot=tpm2 /dev/disk/by-partlabel/zroot-luks
root # generate-zbm
```

Make a new header backup after adding or removing a token.

### Scrubbing and trimming

```
root # zpool scrub zroot
root # zpool trim zroot
root # zpool status -t zroot
```

A scrub reads and checks every block; a full TRIM complements `autotrim`. OpenZFS 2.4.4 installs the systemd timers
`zfs-scrub-monthly@.timer` and `zfs-trim-monthly@.timer`, and its `zfs` package for Fedora packages them. Enable
both for the pool:

```
root # systemctl enable --now zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
root # systemctl list-timers 'zfs-*'
```

**Check:** `systemctl list-timers` lists both timers with their next run.

Option C: TRIM reaches the SSD because the flag is in the LUKS header; `lsblk -D` shows non-zero discard sizes for
`luks-<UUID>`. The SSD's own health: `dnf install -y nvme-cli`, then `nvme smart-log /dev/nvme0`.

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

- `make install` overwrites `config.yaml`, hence the copy and the restore. Your `generic.conf` and `runtime.conf`
  are kept.
- `make install` writes the version into two files of the checkout, and `git pull` refuses to run when the update
  changes either of them. The `git checkout --` line restores both; `git status --short` must then print nothing.
- `claude/zbm-refactor` is a development branch: read `git -C /usr/src/zbm log` after the pull, before building.
- If the repository raises its minimum Rust version above Fedora's `rustc`, `make` says so; wait for Fedora's update.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm it works before relying on it; the
backup entry still boots the previous image.

### Relaxing the pool compatibility

A step most readers can skip. The pool was created with `compatibility=openzfs-2.2-linux`. The OpenZFS 2.4 of this
system knows newer features; to enable them, first rebuild ZFSBootMenu's image, then raise the set:

```
root # generate-zbm
root # zpool set compatibility=openzfs-2.3-linux zroot
root # zpool upgrade zroot
```

- Update every system that imports the pool before enabling new features, starting with ZFSBootMenu, and check that
  your rescue medium's ZFS is new enough. Use the newest set that the installed ZFS **and** the rescue medium both
  know (`ls /usr/share/zfs/compatibility.d/`). This guide raises the set one step at a time: once your rescue
  medium has OpenZFS 2.4 as well, repeat the three commands with `openzfs-2.4-linux`.
- When ZFSBootMenu's ZFS is older than the pool's features it shows *Unsupported features detected* and refuses
  read/write operations, or cannot import the pool at all.
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

- Check that Secure Boot is disabled ([Preparing the NVMe SSD](#preparing-the-nvme-ssd)).
- Choose *ZFSBootMenu*, or the NVMe SSD itself, in the firmware's boot menu (often F8, F11, F12 or Esc).
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
- Options A and B have `zbm.luks=off`: nothing to unlock; look at the first two points.

### Option C: ZFSBootMenu never asks for the passphrase (the image lacks cryptsetup)

ZFSBootMenu never asks for a passphrase and keeps retrying the pool import. In its recovery shell
`command -v cryptsetup` prints nothing; with the shell runtime `zluks` is missing too, while with the Rust runtime
`zluks` exists but cannot open the container. `cryptsetup` was not installed when the image was built, and dracut
then leaves LUKS out without a message. Follow
[Repairing the system from the live medium](#repairing-the-system-from-the-live-medium), run
`dnf install -y cryptsetup` in the chroot, run `generate-zbm`, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

After ZFSBootMenu started Fedora's kernel, Fedora's initramfs could not use the key file:

- **Option B**: `zfs load-key` fails and the boot stops in dracut's emergency shell; OpenZFS's dracut module does not
  ask for the passphrase when `keylocation` is a file.
- **Option C**: systemd-cryptsetup asks for the passphrase (on the screen, or through Plymouth's prompt).

Type the passphrase where one is asked for. In the emergency shell (Option B), `zfs load-key -L prompt zroot` loads
the key and `exit` lets the boot go on; this guide could not confirm that Fedora's initramfs then continues, and the
live medium (below) always works. Then, on the running system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitrd "/boot/initramfs-$(uname -r).img" | grep -E 'zroot\.key|systemd-cryptsetup$'
```

Causes, in order of likelihood:

- the key file is not in the initramfs: `/etc/dracut.conf.d/10-zfs.conf` is missing or misspelled. Fix it and run
  `kernel-install add-all`;
- Option C: the command line lacks the `luks.*` parameters, or carries another UUID than
  `cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks`;
- Option C: `systemd-cryptsetup` is not in the initramfs (the `crypt` module is missing from `add_dracutmodules`);
- Option B: the passphrase was changed without rebuilding the initramfs
  ([Changing the passphrase](#changing-the-passphrase));
- an old snapshot or clone was booted, whose initramfs holds an older key.

If ZFSBootMenu **itself** asks for a second volume in Option C, `zbm.luks=` is missing or wrong in its command line,
and it offers every LUKS volume it finds, for example on a USB disk. `zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI` shows
the command line; fix `Kernel.CommandLine` and run `generate-zbm`.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment, for pairs such as `vmlinuz-<version>` and `initramfs-<version>.img`. In the
recovery shell (Option B: `zfs load-key -L prompt zroot` first; the key file named by `keylocation` is not in
ZFSBootMenu's image):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/fedora
root # ls -l MOUNTPOINT/boot
```

`mount_zfs` prints the mount point to use for `MOUNTPOINT`. Causes:

- `/boot` holds no `vmlinuz-*`: the plugin `95-zfsbootmenu.install` is missing or not executable, or a boot loader
  package changed where kernels go (look for `/boot/<machine-id>/` or `/boot/efi/<machine-id>/`). Repair from the live
  medium, check [Keeping kernels in /boot](#keeping-kernels-in-boot), and run `kernel-install add-all` in the chroot;
- a kernel has no initramfs beside it;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

- In ZFSBootMenu the default import policy adopts the host ID of the pool when it cannot import it with its own, so
  this usually resolves itself. Otherwise, in the recovery shell: `zpool import -f -N zroot`, then `exit`.
- In Fedora's emergency shell: `zpool import -f -N zroot`, then `exit`.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; run
  `kernel-install add-all`. The usual cause is a live system that imported the pool and was shut down without
  `zpool export`. Always export the pool on a rescue medium.

### The recovery shell and its tools

| Command | Use |
|---|---|
| `zfsbootmenu` (or `exit`) | back to the menu |
| `zpool`, `zfs` | the ZFS tools of the image; `zpool import`, `zpool status` |
| `set_rw_pool POOL`, `set_ro_pool POOL` | re-import the pool read/write or read-only |
| `mount_zfs DATASET` | mount a file system at a temporary place and print it |
| `zfs-chroot DATASET` | chroot into a boot environment, to repair it (1) |
| `zsnapshots DATASET` | the snapshot browser |
| `zkexec DATASET KERNEL INITRAMFS` | boot a chosen kernel and initramfs directly |
| `mount_esp DEVICE` | mount an ESP |
| `zluks` and its sub-commands (2) | Option C: unlock, list and close LUKS volumes |
| `logs`, `zlogtail`, `zreport`, `zbmcmdline` | logs, a report for bug reports, ZFSBootMenu's command line |
| `help` | the built-in documentation |
| `reboot`, `poweroff`, `firmware-setup` | leave |

1. Or to rebuild its initramfs; read/write only after `set_rw_pool`.
2. `zluks`, `zluks list`, `zluks status`, `zluks open DEVICE [NAME]` and `zluks close`; `cryptsetup` and `dmsetup`
   are there as well.

### Repairing the system from the live medium

Boot the Fedora live image, become root and install ZFS as in [Installation media](#installation-media) (Option C:
cryptsetup too). Then:

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
```

**Option C:**

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${POOL_PART}")"
root # cryptsetup open "${POOL_PART}" "luks-${LUKS_UUID}"
```

**All options.** First give the live system the installed system's host ID, so that the pool is imported as by its
own host, and a forgotten export does not leave it marked as used by another system:

```
root # zgenhostid -f 0x00bab10c
root # zpool import -f -N -R /mnt zroot
```

**Option B:**

```
root # zfs load-key -L prompt zroot
```

**All options.**

```
root # zfs mount zroot/ROOT/fedora
root # zfs mount zroot/home
root # mount "${ESP_PART}" /mnt/boot/efi
root # mount --rbind --make-rslave /dev /mnt/dev
root # mount --rbind --make-rslave /proc /mnt/proc
root # mount --rbind --make-rslave /sys /mnt/sys
root # mount -t tmpfs tmpfs /mnt/run
root # rm -f /mnt/etc/resolv.conf
root # cp -L /etc/resolv.conf /mnt/etc/resolv.conf
root # chroot /mnt /bin/bash --login
```

`-f` is needed because the pool was last used by the installed system; the stored TRIM flag is applied by
`cryptsetup open` without options. The live system's ZFS module must be at least as new as the pool's features.
Repair what is needed; before leaving the chroot, restore the link with
`ln -sf ../run/systemd/resolve/stub-resolv.conf /etc/resolv.conf`, and run `fixfiles -F onboot` if you created or
changed files. Then leave the chroot and finish as in [Finalizing the installation](#finalizing-the-installation),
without the snapshot: unmount, `zpool export zroot`, and for Option C `cryptsetup close`.

### The ZFS module is missing for a kernel

`modinfo -k VERSION zfs` prints `modinfo: ERROR: Module zfs not found.` for a kernel in `/boot`, or `dkms status`
shows it as not installed. The kernel's development files were missing when DKMS ran, an OpenZFS update removed the
old module from every kernel and built the new one only for the running kernel, or the kernel is newer than the
installed OpenZFS supports. After an OpenZFS update, `kernel-install add-all` fixes every kernel at once. For one
kernel, replace `VERSION` with its version:

```
root # rpm -q --requires zfs-dkms | grep 'kernel-devel <='
root # dnf install -y "kernel-devel-VERSION"
root # dkms autoinstall -k VERSION
root # kernel-install add VERSION /usr/lib/modules/VERSION/vmlinuz
```

If `VERSION` is above the `kernel-devel <=` limit, DKMS cannot build the module: the lock was missing or changed.
Boot an older kernel (choose it with **MOD+K** in ZFSBootMenu), restore the lock as in
[Holding the kernel at a version OpenZFS supports](#holding-the-kernel-at-a-version-openzfs-supports), and remove the
unsupported kernel with `dnf remove kernel-core-VERSION`. A build log is in `/var/lib/dkms/zfs/*/build/make.log`.

### dnf refuses a kernel or ZFS update

`dnf upgrade` reports a conflict or a problem with `kernel-uname-r` or `kernel-devel` and `zfs-dkms`, or lists
kernel packages as skipped. `zfs-dkms` requires an installed kernel and its development files within the range
OpenZFS supports, and conflicts with any newer `kernel-devel` or kernel (`kernel-uname-r`). Check the lock and the
range:

```
root # dnf versionlock list
root # rpm -q --requires zfs-dkms | grep kernel
root # rpm -q kernel-core kernel-devel
```

Every installed `kernel-core` needs a `kernel-devel` of the same version, and at least one of them must be within the
range. If the lock is missing, write it again as in
[Holding the kernel at a version OpenZFS supports](#holding-the-kernel-at-a-version-openzfs-supports) (with
`KERNEL_MAX` from the second command). Meanwhile, `dnf upgrade --exclude='kernel*'` updates everything else.

### SELinux denials after the first boot

Logins fail, or services fail with `Permission denied` although the files are there, and
`ausearch -m avc -ts recent` (package `audit`) lists denials. Usually the relabel did not run or was interrupted.
Request it again and reboot:

```
root # fixfiles -F onboot
root # reboot
```

If you cannot log in, choose the boot environment in ZFSBootMenu, press **MOD+E** and add `enforcing=0` to the command
line for this boot only, then run the two commands. Do not keep SELinux off: the relabel fixes the labels.

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption leaves the pool's structure readable without the passphrase: dataset and snapshot names, the
  dataset hierarchy, properties (including `org.zfsbootmenu:commandline`), file sizes and holes, and deduplication
  tables. File and volume data, file attributes, ACLs, permission bits and directory listings are encrypted.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Fedora's initramfs in
  `/boot`, also inside the pool. It is safe only there. Any copy of the boot environment that is not encrypted
  carries it in clear: a `zfs send` without `-w` (raw), a file-level backup of `/etc` or `/boot`, an initramfs copied
  to the ESP. On the running system, root can read the passphrase.
- Changing the passphrase does not change the key the data is encrypted with: OpenZFS rewraps the same master key,
  and `zfs change-key` "does not necessarily protect existing or newly-written data". It is no cure for a passphrase
  someone learned together with a copy of the disk.
- **Never** place encryption keys inside a ZFSBootMenu image.

**Option C.**

- The ESP is not encrypted and holds only ZFSBootMenu's image and its copies. They contain no key: plain key files
  are never copied into the image.
- The key file opens the container. It is safe because `/etc/cryptsetup-keys.d` and the initramfs in `/boot` are
  inside the encrypted pool. It stops being safe the moment an initramfs with the key is written anywhere else: the
  ESP, an unencrypted partition, a backup, or a unified kernel image. Snapshots and `zfs send` streams of the root
  dataset contain it too; treat them like the pool.
- A LUKS header backup contains the key slots as they were: an old passphrase still opens a restored header. Store
  backups offline and replace them after key changes.
- ZFSBootMenu closes the container before `kexec`, which wipes the volume key from kernel memory.
- TRIM pass-through reveals which blocks are unused, not their content.

**All options.**

- **The ESP is not encrypted.** Anyone with access to the disk, or root on the system, can replace ZFSBootMenu's
  image with one that records the passphrase. Encryption protects data at rest, for example a stolen powered-off
  laptop, not against someone who can modify the computer and wait for you to type the passphrase. Secure Boot with
  signed images, or measured boot with a TPM, narrows that; neither is set up by this article. (`contrib/zbm-sign.pl`,
  a `generate-zbm` post-hook described in [Building ZFSBootMenu](../../general/building.rst), signs images; the ZFS
  module built by DKMS would need signing too.)
- Once the pool is unlocked, the recovery shell and the command-line editor are open to anyone at the keyboard.
- Hooks run as root in ZFSBootMenu; install only hooks you have read.
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed
  before updating, keep the backup entries, and keep the rescue medium.
- The kernel lock keeps the kernel at an older series until OpenZFS supports the next one. The 7.2 series is already
  end of life upstream: until an OpenZFS release supports Linux 7.3, the system runs a kernel without security
  updates. Watch `dnf list --upgrades 'kernel*'`, `rpm -q --requires zfs-dkms | grep 'kernel-devel <='` and
  OpenZFS's releases, and move on as in [Updating the system](#updating-the-system) as soon as you can.

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
  `zfsbootmenu-luks(7)` and `zluks(8)` ([zfsbootmenu-luks.7](../../man/zfsbootmenu-luks.7.rst),
  [zluks.8](../../man/zluks.8.rst))
- [Gentoo Linux on a LUKS-encrypted ZFS root](../gentoo/zfs-luks-v4.md), the model of this guide's form
- [The threat model](../../dev/threat-model.md)
- Fedora: [Fedora Workstation](https://fedoraproject.org/workstation/), `kernel-install(8)`, `dracut.conf(5)`,
  `dnf5-versionlock(8)`, `systemd-cryptsetup-generator(8)` for the `luks.*` parameters, `systemd-cryptenroll(1)`,
  `systemd-firstboot(1)`, `fixfiles(8)`
- OpenZFS: [OpenZFS on Fedora][openzfs-fedora] (the repository and its kernel support) and
  [Fedora Root on ZFS][openzfs-fedora-root] (with rEFInd instead of ZFSBootMenu, and another layout), and the OpenZFS
  documentation (<https://openzfs.github.io/openzfs-docs/>)
- `gdisk(8)`, `cryptsetup(8)`

[openzfs-fedora]: https://openzfs.github.io/openzfs-docs/Getting%20Started/Fedora/index.html
[openzfs-fedora-root]: https://openzfs.github.io/openzfs-docs/Getting%20Started/Fedora/Root%20on%20ZFS.html
