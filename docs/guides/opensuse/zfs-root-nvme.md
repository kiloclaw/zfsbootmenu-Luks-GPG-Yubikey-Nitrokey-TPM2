# openSUSE Tumbleweed on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install openSUSE Tumbleweed from scratch on a **single NVMe SSD** with a **ZFS root
pool**. The system boots with the ZFSBootMenu build from this repository (branch `claude/zbm-refactor`). Tumbleweed is
installed with `zypper --root` from the openSUSE Tumbleweed live image; OpenZFS comes from the openSUSE Build Service
project `filesystems` as prebuilt kernel module packages; Tumbleweed's own initramfs is built by **dracut**.

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Tumbleweed's initramfs then unlocks the pool
again with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from openSUSE's package sources (suse-module-tools, kernel-source), OpenZFS's documentation for
> openSUSE, dracut's sources and this repository. The project has not run it from start to finish. `claude/zbm-refactor`
> is a development branch, not a release. The project's VM tests boot ZFSBootMenu images built with dracut and a test
> system whose initramfs is built with dracut, with unencrypted pools, ZFS native encryption and LUKS; they do not
> install openSUSE Tumbleweed. Keep a live medium at hand until the system has booted several times, and read
> [What the project has tested](#what-the-project-has-tested) before you start.

This guide is for **Tumbleweed only**. openSUSE Leap 16.1 (released in November 2026) has other repositories, another
kernel and other package versions, and this guide could not confirm that the `filesystems` project builds ZFS for it;
do not follow it for Leap.

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
  - [Before the kernel: /etc/fstab and the boot loader packages][before-kernel]
  - [Installing the kernel and OpenZFS](#installing-the-kernel-and-openzfs)
  - [Locking the kernel and ZFS together](#locking-the-kernel-and-zfs-together)
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
  - [No ZFS module for the kernel](#no-zfs-module-for-the-kernel)
  - [zypper reports a problem with the kernel or ZFS packages][zypper-problem]
- [Security notes](#security-notes)
- [See also](#see-also)

[no-cryptsetup]: #option-c-zfsbootmenu-never-asks-for-the-passphrase-the-image-lacks-cryptsetup
[second-unlock]: #the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs
[hostid-refused]: #the-pool-is-refused-wrong-host-id-or-last-used-by-another-system
[before-kernel]: #before-the-kernel-etcfstab-and-the-boot-loader-packages
[zypper-problem]: #zypper-reports-a-problem-with-the-kernel-or-zfs-packages

## Overview

### Choosing an option

| | Option A | Option B | Option C |
|---|---|---|---|
| Pool | not encrypted | ZFS native encryption (`aes-256-gcm`) | inside a LUKS2 container |
| Passphrase typed | never | once, in ZFSBootMenu | once, in ZFSBootMenu |
| Second unlock (system) | none | key file `/etc/zfs/zroot.key` | key file `/etc/cryptsetup-keys.d/zroot.key` |
| Partition 2 type | `BF00` | `BF00` | `8309` |
| Visible without passphrase | everything | names, sizes, properties | the container's size and LUKS header |
| Extra software | none | none | `cryptsetup` |

- The second unlock is done by Tumbleweed's own initramfs, after ZFSBootMenu has started its kernel.
- Option B leaves dataset names, sizes, properties and snapshot names visible without the passphrase, but not file
  contents or file names ([Security notes](#security-notes)).
- Option C needs `cryptsetup` on the live system, in the new system and in ZFSBootMenu's image.

Option B stores the **passphrase itself** in its key file: OpenZFS has one key per encryption root, and the file
named by the `keylocation` property lets the system load it without asking. Option C stores a **different, random**
key in a second LUKS key slot. Both could be combined; this guide does not.

### Boot process

1. The UEFI firmware starts **ZFSBootMenu**, one EFI file on the EFI system partition (ESP).
2. **Option C**: ZFSBootMenu reads `zbm.luks=` from its kernel command line, asks for the **LUKS passphrase** and
   opens the container.
3. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/opensuse` with a countdown of 10 seconds.
   When the countdown ends, the pool's `bootfs` is booted; a key press opens the menu instead.
4. **Option B**: the `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in
   ZFSBootMenu's image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime this
   prompt comes **after** the countdown.
5. ZFSBootMenu loads Tumbleweed's kernel (`/boot/vmlinuz-<version>-default`) and initramfs
   (`/boot/initrd-<version>-default`) from `/boot` **inside the boot environment** and starts the kernel with
   `kexec`. It passes the command line stored in the ZFS property `org.zfsbootmenu:commandline`, the root argument
   `root=zfs:zroot/ROOT/opensuse` and the host ID. **Option C**: before `kexec` it exports the pool and closes the
   container, which wipes the volume key from memory.
6. Tumbleweed's initramfs (dracut, with systemd and OpenZFS's `zfs` module) starts in the new kernel:
   - **Option A**: it imports the pool and mounts the root file system.
   - **Option B**: it imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the initramfs**,
     without asking.
   - **Option C**: systemd-cryptsetup unlocks the container again with the key file
     `/etc/cryptsetup-keys.d/zroot.key` **inside the initramfs**, without asking; then the pool is imported.

Why a second unlock (Options B and C): `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. The boot environment's own initramfs must unlock the pool again, exactly as it would without
ZFSBootMenu. ZFSBootMenu does not replace Tumbleweed's initramfs.

### Where the keys are

**Option A.** There are none.

**Option B.**

- The **ZFS passphrase**: in your memory. Typed into ZFSBootMenu at every boot, and into a live system for repairs.
- The **same passphrase in `/etc/zfs/zroot.key`**: inside the encrypted pool, and in the copy of it in Tumbleweed's
  initramfs in `/boot`, also inside the pool. Read by Tumbleweed's initramfs after `kexec`.

**Option C.**

- The **LUKS passphrase** (key slot 0): in your memory. Typed into ZFSBootMenu at every boot, and into a live system
  for repairs.
- The **random key file `/etc/cryptsetup-keys.d/zroot.key`** (key slot 1): inside the encrypted pool, and in
  Tumbleweed's initramfs in `/boot`, also inside the pool. Read by Tumbleweed's initramfs after `kexec`.

The key file in Tumbleweed's initramfs is safe **only because** the file and the initramfs both live inside the
encrypted pool (`/etc` and `/boot`); they cannot be read before the passphrase has been typed. ZFSBootMenu's own image
lives on the unencrypted ESP and never contains a key.

### What the project has tested

From [Testing](../../luks/testing.rst):

- The VM tests boot ZFSBootMenu images built **with dracut** by the project's build container (Ubuntu 26.10,
  Linux 7.3, OpenZFS 2.4.4, cryptsetup 2.8.7, systemd 261, dracut 112), in QEMU. The images and the system they boot
  are built in that Ubuntu container, not on openSUSE.
- The system they boot is a test payload, not an installed distribution: an Ubuntu kernel with a **dracut** initramfs
  that holds OpenZFS's `zfs` module and systemd-cryptsetup, and a minimal root file system.
- For this guide that means: both generators are the tested ones (dracut for ZFSBootMenu's image, dracut with systemd
  for the system's initramfs). Tumbleweed's kernel, the `filesystems` project's ZFS packages, openSUSE's dracut and
  its default settings, and the kernel package scripts this guide relies on are **not** covered.
- The suite runs with the Rust runtime by default. The shell runtime, which this guide uses unless you choose
  otherwise, was last run in full on 20 October 2026.
- **Option A**: unencrypted pools are booted with the Rust runtime in every run, and with the shell runtime when its
  tests are named.
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
- **Secure Boot disabled** in the firmware: the ZFSBootMenu image built here is not signed, and the ZFS module of the
  `filesystems` project is signed with that project's key, not openSUSE's (the OpenZFS guide for Tumbleweed names the
  package `zfs-ueficert`, which asks MOK to enroll that key at the next boot). How to check is shown in
  [Preparing the NVMe SSD](#preparing-the-nvme-ssd).
- An internet connection, wired if possible.
- At least 8 GiB of RAM is advised: the live system keeps the packages it installs in memory.
- Free space on the SSD for the build: a few GiB for the clone of this repository and Cargo's build directory (not
  measured; `du -sh /usr/src/zbm` after the build shows it).

### Installation media

The facts this guide depends on, and how to check each on your system. This guide could not open openSUSE's own web
sites or package indexes from where it was written; the versions below are therefore not given, and every one of them
is checked by a command before it matters.

- **Release model:** rolling, a new snapshot of the whole distribution every few days.
  Check: `grep -E '^(NAME|VERSION_ID)=' /etc/os-release`.
- **Kernel:** the package `kernel-default`. The kernel is `/boot/vmlinuz-<version>-default`, a link into
  `/usr/lib/modules/<version>-default/`; its initramfs is `/boot/initrd-<version>-default`. Check: `uname -r`.
- **OpenZFS:** not in Tumbleweed's own repositories. The Build Service project `filesystems` builds `zfs` and the
  kernel module package `zfs-kmp-default`, marked experimental. Check: `zypper search -s -x zfs zfs-kmp-default`.
- **Kernels the ZFS module fits:** one. The version of `zfs-kmp-default` names the kernel it was built for, as
  `<zfs version>_k<kernel version>`; see below.
- **System initramfs:** dracut, with systemd in the initramfs. openSUSE's defaults build host-only images named
  `initrd-<version>`. Check: `cat /usr/lib/dracut/dracut.conf.d/01-dist.conf`.
- **`rustc`:** Tumbleweed's `rust` package follows the Rust releases; this repository needs 1.88 or later.
  Check: `zypper info rust`.
- **EFI stub:** the package `systemd-boot` (installing it installs no boot loader).
  Check: `zypper search -f /usr/lib/systemd/boot/efi/linuxx64.efi.stub`.
- **Package manager:** `zypper`; Tumbleweed is updated with `zypper dup`. Check: `zypper --version`.

> **Important**
> The ZFS kernel module comes as a prebuilt package for **one** kernel version, and Tumbleweed updates its kernel
> often. The `filesystems` project rebuilds the module after each new kernel, but not at the same moment, and a
> kernel newer than OpenZFS supports gets no module at all. A system that boots a kernel without its ZFS module
> cannot import its pool. OpenZFS 2.4.4 (21 August 2026) supports Linux up to 7.2; the supported range of the newest
> release is in its notes at <https://github.com/openzfs/zfs/releases>. This guide therefore **locks** the kernel and
> the ZFS packages together
> ([Locking the kernel and ZFS together](#locking-the-kernel-and-zfs-together)) and updates them only as a pair
> ([Updating the system](#updating-the-system)).

Download the **openSUSE Tumbleweed GNOME live image** for x86_64 (a file named like
`openSUSE-Tumbleweed-GNOME-Live-x86_64-Current.iso`) from <https://get.opensuse.org/tumbleweed/>, where the live
images are listed besides the installation images. Write it to a USB stick and boot it in **UEFI** mode. Connect to
the network and open a terminal. The KDE and Xfce live images work too; only the command that stops automounting
below is GNOME's. The OpenZFS guide for Tumbleweed says: if prompted, log in with the user name `live` and the password
`live`; on the images it describes, `sudo -i` asks for no password. This guide could not confirm either on the current
images.

Stop the desktop from mounting old partitions of the SSD (run this as the live session's user, before becoming
root):

```
user $ gsettings set org.gnome.desktop.media-handling automount false
```

Become root and look at the kernel of the live system and at the ZFS module packages the `filesystems` project
offers:

```
user $ sudo -i
root # zypper addrepo https://download.opensuse.org/repositories/filesystems/openSUSE_Tumbleweed/filesystems.repo
root # zypper --gpg-auto-import-keys refresh
root # uname -r
root # zypper search -s -x kernel-default zfs-kmp-default
```

- `zypper addrepo` adds the `filesystems` project, as the OpenZFS documentation for openSUSE does;
  `--gpg-auto-import-keys` accepts the project's signing key.
- `uname -r` prints the live system's kernel, for example `7.2.11-1-default`. The `zfs-kmp-default` lines show
  versions such as `2.4.4_k7.2.11_1-1.1`: the part after `_k` is the kernel the module was built for, with `_`
  instead of `-` (`7.2.11_1` means `7.2.11-1`). The `kernel-default` lines show the kernels in the repositories, such
  as `7.2.11-1.1`, whose kernel version is the part before the last dot (`7.2.11-1`). These are examples of the form
  only; this guide could not read the current versions.
- If `zypper search` lists no `kernel-default` at all, the live system has no online Tumbleweed repository; add it
  with `zypper addrepo https://download.opensuse.org/tumbleweed/repo/oss/ repo-oss` and refresh again.
- You need **two** matches. For the live system: a `zfs-kmp-default` for the kernel of `uname -r`. For the new
  system: a `zfs-kmp-default` for the newest `kernel-default` in the repositories. Usually both are the same kernel.
- If the module is built for a **newer** kernel than the live system runs, download the newest live image (the file
  named `Current` follows the snapshots) and start again. If it is built for an **older** kernel than the newest
  `kernel-default`, the `filesystems` project has not caught up yet; wait a few days, or until the next OpenZFS
  release supports the new kernel. There is no way around a missing module.
- Tumbleweed also has a `kernel-longterm` package. Whether the `filesystems` project builds a matching
  `zfs-kmp-longterm` today could not be confirmed (a 2024 forum thread reported that it did not work then); this
  guide does not describe that path. `zypper search -s kernel-longterm 'zfs-kmp-*'` shows what exists. If a
  `zfs-kmp-longterm` names the same kernel after `_k` as a `kernel-longterm`, the guide's commands work with
  `kernel-longterm` and `zfs-kmp-longterm` in place of `kernel-default` and `zfs-kmp-default` everywhere (installing,
  locks, updates, and `-longterm$` instead of `-default$` when setting `KVER`); the files are then
  `/boot/vmlinuz-<version>-longterm` and `/boot/initrd-<version>-longterm`, which ZFSBootMenu also finds.

Install ZFS and the partitioning tool for the running kernel:

```
root # zypper install zfs zfs-kmp-default gptfdisk nvme-cli dosfstools
root # modprobe zfs
```

> **Warning**
> Read the list of packages `zypper install` shows before you answer `y`. If it lists a **`kernel-default`** to
> install, the module does not fit the running kernel: answer `n` and see the previous list.

- `gptfdisk` is openSUSE's package for `gdisk`.
- If `modprobe zfs` reports `Module zfs not found`, the installed module is for another kernel: compare
  `rpm -q zfs-kmp-default` with `uname -r`. If it reports that the module is not supported, openSUSE's module policy
  refuses modules from outside its own kernel packages; `modprobe --allow-unsupported-modules zfs` loads it once.
  This guide could not confirm whether current Tumbleweed refuses them.

**Option C** also needs cryptsetup, which the live image normally has; this installs it if not:

```
root # zypper install cryptsetup
```

### Conventions

| Notation | Meaning |
|---|---|
| `user $` | run as the live session's own user, before becoming root |
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new openSUSE Tumbleweed system** |
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
root # rpm -q gptfdisk
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
root # cryptsetup --version
```

- `ls` lists files; otherwise the live system booted in legacy BIOS mode.
- `zfs version` prints two lines, `zfs-...` (the tools) and `zfs-kmod-...` (the module), with the same version, 2.2
  or later. The pool in this guide needs OpenZFS 2.2 or later.
- `rpm -q gptfdisk` prints `gptfdisk-1.0.9` or a later version (gdisk itself has no `--version` option).
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
root # export BE=opensuse
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the ESP; NVMe partition names have a `p` before the number |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `opensuse` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}` |
| `POOL_DEV` | see below | A B C | the device `zpool create` gets |
| `LUKS_UUID` | see below | C | the UUID of the LUKS container; exists only after `luksFormat` |

`POOL_DEV` and `LUKS_UUID` are set later, not here:

- `POOL_DEV` is `${POOL_PART}` for Options A and B, set in [Creating the ZFS pool](#creating-the-zfs-pool), and
  `/dev/mapper/luks-${LUKS_UUID}` for Option C, set in
  [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container).
- `LUKS_UUID` is `$(cryptsetup luksUUID "${POOL_PART}")`, set in the same Option C section.

The name in `BE` is free; ZFSBootMenu does not read it. It recognizes the distribution from `/etc/os-release` inside
the boot environment.

Some paths are written literally, because files cannot expand variables. If you change `POOL`, change `zroot` in
these paths too: `/etc/zfs/zroot.key` (Option B), `/etc/cryptsetup-keys.d/zroot.key` (Option C), and every later
command that names `zroot` after the installation. If you change `BE`, change `opensuse` in the commands after the
installation as well.

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
root # : "${DISK:?set the variables first}" "${ESP_PART:?}" "${POOL_PART:?}"
root # lsblk -o NAME,SIZE,FSTYPE,MOUNTPOINTS "${DISK}"
root # swapoff -a
root # zpool export -a
root # for part in "${DISK}"p*; do [ -b "${part}" ] && zpool labelclear -f "${part}"; done
root # for part in "${DISK}"p*; do [ -b "${part}" ] && wipefs -a "${part}"; done
root # blkdiscard -f "${DISK}"
root # wipefs -a "${DISK}"
```

- The first line stops with `DISK: set the variables first` (or names another variable) when the variables are not
  set, for example after a reboot of the live system; set them as in [Setting variables](#setting-variables).
- `lsblk` must show no mount point; unmount anything the live system mounted from the SSD (`umount PATH`).
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
| 1 | `${ESP_PART}` | 1 GiB | `EF00` | `EFI` | the ESP, FAT32, mounted at `/boot/efi`; holds ZFSBootMenu |
| 2 (A, B) | `${POOL_PART}` | the rest | `BF00` | `zroot` | the ZFS pool |
| 2 (C) | `${POOL_PART}` | the rest | `8309` | `zroot-luks` | the LUKS2 container that holds the pool |

The devices are `/dev/nvme0n1p1` and `/dev/nvme0n1p2`. gdisk names the types `EFI system partition` (`EF00`),
`Solaris root` (`BF00`) and `Linux LUKS` (`8309`).

The ESP holds only ZFSBootMenu, but several copies of it: the image, its backup, a copy at `\EFI\BOOT\BOOTX64.EFI`,
and any copy you keep yourself. 1 GiB leaves room for all of them. No Tumbleweed kernel or initramfs is ever stored on
the ESP: they stay in `/boot` inside the pool. The type `BF00` is a label for people and tools; ZFS ignores it.

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
  `zypper install parted && partprobe "${DISK}"`, or reboot the live system, set the variables again and repeat
  the installation of ZFS from [Installation media](#installation-media).
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
root # : "${POOL_PART:?set the variables first}"
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
  Using the same name everywhere keeps the device path recorded in the pool the same in ZFSBootMenu and in
  Tumbleweed.
- `--allow-discards --persistent` stores the TRIM flag **in the LUKS2 header**, so the container passes TRIM requests
  through wherever it is opened (needed for `autotrim=on`): in ZFSBootMenu, in Tumbleweed's initramfs and on a live
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
normal import by another host while the pool was not exported. ZFSBootMenu, Tumbleweed's initramfs and the running
system all import this pool, so they must agree. Write a fixed host ID now; it is copied into the new system later.

```
root # zgenhostid -f 0x00bab10c
```

**2. Option B: the passphrase file.** `zpool create` reads the passphrase from the file named by `keylocation`, and
the same file later lets Tumbleweed's initramfs load the key without asking.

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

**All options.** Make sure `POOL_DEV` is set; this line stops with an error if it is not:

```
root # : "${POOL:?set the variables first}" "${POOL_DEV:?set POOL_DEV first}"
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
| `compatibility=openzfs-2.2-linux` | only features OpenZFS 2.2 knows: live systems with ZFS 2.2 or later import it |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | everything the pool mounts during the installation appears below `/mnt` |
| `encryption=aes-256-gcm` (B) | likely the fastest cipher on modern x86_64 hardware |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file of step 2; also in Tumbleweed's initramfs, in the pool |

The compatibility can be relaxed later ([Relaxing the pool compatibility](#relaxing-the-pool-compatibility)). In
Option B the pool's root dataset is the encryption root, so every dataset created later inherits the encryption,
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
| `zroot/ROOT/opensuse` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, with `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/opensuse` | the boot environment ZFSBootMenu boots after the countdown |

- `canmount=noauto` is required on every file system with `mountpoint=/`, and it is not inherited: each boot
  environment you create later needs it set again.
- Keep `/usr`, `/etc` and `/var` in the boot environment: they change together with every package update (the RPM
  database is in `/usr/lib/sysimage/rpm`, and the kernel itself is in `/usr/lib/modules`). Add datasets only for user
  data (`/srv`, for example), if you want them.

> **Important**
> `/boot` is a directory of the root dataset, not a file system of its own and not the ESP. ZFSBootMenu reads
> Tumbleweed's kernel and initramfs from there, and in Options B and C the initramfs with the key file is then inside
> the encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/opensuse`.

### Setting the ZFSBootMenu properties

Why: ZFSBootMenu reads the kernel command line for Tumbleweed from a ZFS property. It is set on `zroot/ROOT`, so
every future boot environment inherits it.

**Options A and B.** Tumbleweed's initramfs needs nothing on the command line: Option B finds the key file through
the dataset's `keylocation`.

```
root # zfs set org.zfsbootmenu:commandline="quiet" "${POOL}/ROOT"
```

**Option C.** Tumbleweed's initramfs unlocks the container from three kernel parameters, without a crypttab. The
command line is built in two steps to keep the lines short:

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

**All options.** Do not put `root=` into this property: ZFSBootMenu always adds the root argument itself. Tumbleweed's
`/etc/os-release` names `opensuse-tumbleweed`, which is not among the distributions with a special prefix, so
ZFSBootMenu adds `root=zfs:zroot/ROOT/opensuse`, which OpenZFS's dracut module reads; there is no need to set
`org.zfsbootmenu:rootprefix`. Do not add `spl.spl_hostid=` either; ZFSBootMenu sets it. Tumbleweed's own installer
usually adds `splash=silent`, which shows Plymouth's graphical boot screen if Plymouth is installed; add it to the
property if you want that.

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
(Option C). The second prints `zroot/ROOT/opensuse` for Option B and `-` for Options A and C.

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

`/mnt` is `zroot/ROOT/opensuse` and `/mnt/home` is `zroot/home`. Option B: the key status is `available` (Options A
and C print `-`).

## Installing the base system

Why: `zypper --root` installs a minimal Tumbleweed into the mounted boot environment, from the same repositories the
live system uses.

Give the new system the live system's devices and kernel interfaces, and a `/run` in memory, before installing:
package scripts run inside the new system and need them.

```
root # mkdir -p /mnt/dev /mnt/proc /mnt/sys /mnt/run
root # mount --rbind --make-rslave /dev /mnt/dev
root # mount --rbind --make-rslave /proc /mnt/proc
root # mount --rbind --make-rslave /sys /mnt/sys
root # mount -t tmpfs tmpfs /mnt/run
```

Copy the live system's repository definitions, which include the `filesystems` repository added in
[Installation media](#installation-media), and show what the new system will use:

```
root # mkdir -p /mnt/etc/zypp/repos.d
root # cp /etc/zypp/repos.d/*.repo /mnt/etc/zypp/repos.d/
root # zypper --root /mnt lr -u
```

The list must contain Tumbleweed's `oss` repository (often named `repo-oss` or `openSUSE-Tumbleweed-Oss`), its
`non-oss` and `update` repositories if the live system has them, and `filesystems`, each with an `http` or `https`
URI and `Enabled` set to `Yes`. Remove any repository whose URI starts with `cd:`, `dvd:`, `iso:` or `hd:` with
`zypper --root /mnt removerepo NAME`; it belongs to the live medium.

Only if the list lacks Tumbleweed's online repositories, add them (the first two as in the OpenZFS guide for
Tumbleweed; the URI of the update repository could not be confirmed, so compare it with `zypper lr -u` on any
Tumbleweed system):

```
root # zypper --root /mnt addrepo https://download.opensuse.org/tumbleweed/repo/oss/ repo-oss
root # zypper --root /mnt addrepo https://download.opensuse.org/tumbleweed/repo/non-oss/ repo-non-oss
root # zypper --root /mnt addrepo https://download.opensuse.org/update/tumbleweed/ repo-update
```

Install the base system:

```
root # zypper --root /mnt --gpg-auto-import-keys refresh
root # zypper --root /mnt install -y -t pattern enhanced_base
root # zypper --root /mnt install -y zypper nano sudo NetworkManager glibc-locale-base kbd \
  timezone dracut
```

- `--gpg-auto-import-keys` imports the repositories' signing keys into the new system.
- `enhanced_base` is the pattern both the OpenZFS and the older ZFSBootMenu guides for openSUSE install: a console
  system with the usual tools. If `zypper` reports that the pattern is not found, `zypper --root /mnt search -t
  pattern` lists the patterns of the current snapshot; choose `base` and add the tools you want. The OpenZFS guide
  warns that `base` installs `busybox-grep`, which blocks `kernel-default`: remove it
  (`zypper --root /mnt remove busybox-grep`) before [Kernel and ZFS](#kernel-and-zfs).
- The second command adds `zypper` itself, an editor, `sudo`, NetworkManager for the network, the English locales
  (`glibc-locale-base`), console keymaps (`kbd`), time zones and `dracut`. A desktop can be added after the first
  boot ([Updating the system](#updating-the-system)).
- If the pattern pulls in a kernel already, package scripts print that the initrd cannot be built yet ("Please run
  dracut ... as soon as your system is complete"). That is expected; [Kernel and ZFS](#kernel-and-zfs) builds it.

Give the new system a working name resolution for the installation. The original `/etc/resolv.conf` of the new
system (on Tumbleweed usually a link into `/run`, which is empty until the system boots) is kept aside and put back in
[Finalizing the installation](#finalizing-the-installation):

```
root # cp -P /mnt/etc/resolv.conf /mnt/root/resolv.conf.orig
root # rm -f /mnt/etc/resolv.conf
root # cp -L /etc/resolv.conf /mnt/etc/resolv.conf
```

If the first command reports that `/mnt/etc/resolv.conf` does not exist, continue: there is nothing to put back.

**Check:**

```
root # grep -E '^(NAME|ID|ID_LIKE)=' /mnt/etc/os-release
root # rpm --root /mnt -q zypper dracut NetworkManager
root # ls /mnt
```

`NAME="openSUSE Tumbleweed"`, `ID="opensuse-tumbleweed"` and an `ID_LIKE` with `opensuse` and `suse`; the three
packages are installed; and `/mnt` holds a root file system (`etc`, `usr`, `var` and so on, with `bin`, `lib` and
`sbin` as links into `usr`).

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
with mode `----------`, and the file shows the values you set (Option C: with the LUKS UUID).

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
(chroot) root # echo "${POOL} ${BE} ${DISK}"
(chroot) root # ls /sys/firmware/efi/efivars | head -n 3
(chroot) root # zypper refresh
```

The first prints `zroot opensuse /dev/nvme0n1`; the second lists EFI variables; `zypper refresh` reads every
repository, including `filesystems`, without errors.

## Kernel and ZFS

Why: Tumbleweed's kernel, and the ZFS module package built for it, make up the system ZFSBootMenu will boot.

### Before the kernel: /etc/fstab and the boot loader packages

Why: the kernel package's scripts build the initramfs only when `/etc/fstab` exists (otherwise they assume an image
build and print "Please run dracut ... as soon as your system is complete"), and they hand kernels to a boot loader
tool when one is installed. This system has no boot loader besides ZFSBootMenu.

The ESP is the only entry in `/etc/fstab`; ZFS mounts its datasets itself:

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

`generate-zbm` mounts the ESP from this entry when it is not mounted. `umask=0077` makes the ESP readable by root
only.

How Tumbleweed's kernel packages place their files, from the scripts of `suse-module-tools` and the kernel package:

- The kernel itself is `/usr/lib/modules/<version>-default/vmlinuz`. The package script links it as
  `/boot/vmlinuz-<version>-default` and builds `/boot/initrd-<version>-default` with dracut. If both files exist at
  that moment, it also points the links `/boot/vmlinuz` and `/boot/initrd` at the kernel installed last (otherwise it
  prints "ERROR: cannot create symlinks"; ZFSBootMenu does not need the links). ZFSBootMenu finds all of these inside
  the boot environment.
- The scripts build the initramfs with the generator named by `INITRD_GENERATOR` in `/etc/sysconfig/bootloader`:
  dracut when it is unset or `dracut`, mkosi-initrd when it is `mkosi`. This guide needs dracut.
- When `sdbootutil` is installed and reports a systemd-boot installation on the ESP, the scripts hand the kernels to
  it instead, and it copies kernels and initramfs images **onto the ESP**. When `perl-Bootloader` is installed, they
  ask it to add a boot loader entry. Neither may happen here.

> **Warning**
> Do not install `grub2`, `grub2-x86_64-efi`, `shim`, `sdbootutil` or `perl-Bootloader`, and do not run
> `bootctl install` or `sdbootutil install`. They install another boot loader on the ESP, or copy kernels and
> initramfs images (with the key file of Options B and C) onto the unencrypted ESP.

Check that none of them is installed, then lock them, so that no later `zypper install` or pattern pulls them in:

```
(chroot) root # rpm -q grub2 grub2-x86_64-efi shim sdbootutil perl-Bootloader
(chroot) root # zypper addlock grub2 'grub2-*' shim sdbootutil perl-Bootloader
```

`rpm -q` must print `package ... is not installed` five times. If one of the packages is installed, remove it with
`zypper remove PACKAGE` (read the list of packages that would go with it before answering `y`), then add the locks.
A lock on a package that is not installed keeps it from being installed; a later command that needs one of them
stops with a `Problem:` instead, and you choose the solution that does not install it.

**Check:**

```
(chroot) root # cat /etc/fstab
(chroot) root # zypper locks
(chroot) root # grep -s '^INITRD_GENERATOR' /etc/sysconfig/bootloader
(chroot) root # [ -e /.buildenv ] && echo "remove /.buildenv" || echo "no /.buildenv"
```

- `/etc/fstab` holds the ESP line.
- `zypper locks` lists `grub2`, `grub2-*`, `shim`, `sdbootutil` and `perl-Bootloader`.
- The `grep` prints nothing or `INITRD_GENERATOR="dracut"`. If it prints `mkosi`, change the value to `dracut` in
  `/etc/sysconfig/bootloader`.
- The last command prints `no /.buildenv`.

### Installing the kernel and OpenZFS

Why: the kernel, OpenZFS's tools with their dracut module, and the ZFS module package for this kernel, in one
transaction.

Check first that the module package fits the newest kernel, as on the live system:

```
(chroot) root # zypper search -s -x kernel-default zfs-kmp-default
```

The newest `kernel-default` (for example `7.2.11-1.1`, kernel `7.2.11-1`) must have a `zfs-kmp-default` whose
version contains `_k` and the same kernel (`_k7.2.11_1`). If not, stop here and see the
[Installation media](#installation-media) section for what to do.

> **Warning**
> Read the list of packages `zypper install` shows before you answer `y`: it must install `kernel-default` and
> `zfs-kmp-default` for the **same** kernel. Otherwise answer `n`. To be sure of the kernel, you can name its version,
> for example `kernel-default-7.2.11-1.1`, as the search printed it.

```
(chroot) root # zypper install kernel-default zfs zfs-kmp-default ucode-intel ucode-amd
(chroot) root # KVER="$(ls /usr/lib/modules | grep -- '-default$' | sort -V | tail -n 1)"
(chroot) root # echo "export KVER=\"${KVER}\"" >> /root/install.env
(chroot) root # systemctl enable zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target zfs-zed.service
```

- `ucode-intel` and `ucode-amd` are the CPU microcode. dracut puts the one for this CPU at the start of the initramfs
  file; ZFSBootMenu loads only this one initramfs file per kernel, so microcode must be inside it.
- The kernel's package script runs `depmod` and dracut for the new kernel; the module package's script makes the ZFS
  module known to the kernel and rebuilds the initramfs when it holds ZFS. The first initramfs cannot unlock anything
  yet; [Configuring the initramfs](#configuring-the-initramfs) builds it again.
- `KVER` names the installed kernel, for example `7.2.11-1-default`; it is added to `/root/install.env` for the
  Checks that follow.
- The ZFS units are enabled explicitly: this guide could not confirm which of them openSUSE's presets enable.

**Check:**

```
(chroot) root # echo "${KVER}"
(chroot) root # rpm -q kernel-default zfs zfs-kmp-default
(chroot) root # modinfo -k "${KVER}" -F version zfs
(chroot) root # ls -l /boot
(chroot) root # ls -A /boot/efi
(chroot) root # ls /usr/lib/dracut/modules.d/ | grep -i zfs
(chroot) root # systemctl is-enabled zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target \
  zfs-zed.service
(chroot) root # od -A n -t x4 /etc/hostid
(chroot) root # rpm -qa 'kernel-firmware*' | head -n 3
```

- `KVER` is one version ending in `-default`, and `rpm -q` shows one kernel and one module package for it.
- `modinfo` prints the ZFS version that `zfs version` printed on the live system. If it reports `Module zfs not
  found`, see [No ZFS module for the kernel](#no-zfs-module-for-the-kernel).
- `/boot` holds `vmlinuz-${KVER}` (a link to `../usr/lib/modules/${KVER}/vmlinuz`), `initrd-${KVER}` with mode
  `-rw-------`, the links `vmlinuz` and `initrd` (if present), `System.map-${KVER}`, `config-${KVER}`,
  `sysctl.conf-${KVER}`, and the directory `efi`. `/boot/efi` is empty.
- The dracut modules list a directory such as `90zfs`. The `filesystems` project's package specification lists dracut
  files in the `zfs` package itself; this guide could not read the package. If nothing is listed,
  `zypper search zfs` shows whether a separate dracut package exists; install it.
- `systemctl is-enabled` prints `enabled` five times, and the host ID is `00bab10c`.
- `rpm -qa 'kernel-firmware*'` lists firmware packages (`kernel-default` recommends them, and zypper installs
  recommended packages by default). If it lists nothing, `zypper search kernel-firmware` shows the names; install
  `kernel-firmware-all` or `kernel-firmware`, whichever exists (this guide could not confirm the current name).
  Without firmware, Wi-Fi and graphics may not work; the NVMe SSD does.

If `/boot` has no `initrd-${KVER}`, the package script did not build it (for example because `/etc/fstab` was
missing); [Configuring the initramfs](#configuring-the-initramfs) builds it.

### Locking the kernel and ZFS together

Why: `zypper dup` would otherwise install a new Tumbleweed kernel as soon as the snapshot has one, whether or not a
ZFS module exists for it, and the next boot would choose that kernel. Locks keep the packages that must fit together
at their installed versions: the kernel, the ZFS packages and the ZFS libraries the `zfs` package depends on.
[Updating the system](#updating-the-system) shows how to move them on.

```
(chroot) root # zypper addlock kernel-default 'zfs*' 'libzfs*' 'libzpool*' 'libnvpair*' 'libuutil*'
```

The quotes keep the shell from expanding the patterns; zypper matches them against package names. The library names
(`libzfs6`, `libzpool6`, `libnvpair3`, `libuutil3` and the like) could not be confirmed from the `filesystems`
project; the Check compares them with what is installed.

`zypper dup` then updates everything else and leaves these alone. It may report a problem when another
package wants a newer version of one of them; see
[zypper reports a problem with the kernel or ZFS packages][zypper-problem].
This locking scheme is this guide's own; it is not documented by openSUSE or OpenZFS and has not been run by the
project.

**Check:**

```
(chroot) root # zypper locks
(chroot) root # rpm -qa | grep -E 'zfs|zpool|nvpair|uutil' | sort
```

`zypper locks` lists the boot loader locks and `kernel-default`, `zfs*`, `libzfs*`, `libzpool*`, `libnvpair*` and
`libuutil*`. Every package that `rpm -qa` prints matches one of these patterns; if one does not, add a lock for it
too, and use it in every `removelock` and `addlock` line of this guide.

The pattern `zfs*` also keeps uninstalled packages such as `zfs-ueficert` from being installed; lift it with
`zypper removelock 'zfs*'` for that, and lock again.

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Why: Tumbleweed's initramfs must unlock the container again after `kexec`; a random key in a second key slot lets it
do so without asking.

Install cryptsetup in the new system. It is also what puts LUKS support into ZFSBootMenu's image later. The
initramfs unlocks with `systemd-cryptsetup`, which must be installed too:

```
(chroot) root # zypper install -y cryptsetup
(chroot) root # ls /usr/lib/systemd/systemd-cryptsetup
```

If `ls` reports that the file does not exist, install the package that has it: `zypper install systemd-cryptsetup`.
This guide could not confirm whether Tumbleweed's systemd ships it in its main package or in that separate one.

```
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
(chroot) root # ls -l /etc/cryptsetup-keys.d/zroot.key /usr/lib/systemd/systemd-cryptsetup
(chroot) root # cryptsetup open --test-passphrase --verbose --key-file /etc/cryptsetup-keys.d/zroot.key "${POOL_PART}"
(chroot) root # cryptsetup luksDump "${POOL_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The key file is `-r--------` and owned by root, `systemd-cryptsetup` exists, cryptsetup reports
`Key slot 1 unlocked.`, and two key slots (`0` and `1`) are listed.

## Configuring the initramfs

Why: Tumbleweed's initramfs must contain OpenZFS's dracut module, the NVMe driver and, for Options B and C, the key
file, and it must be readable by root only. Settings go into a file of their own in `/etc/dracut.conf.d/`, never into
`/etc/dracut.conf`.

openSUSE's dracut defaults (in `/usr/lib/dracut/dracut.conf.d/01-dist.conf`) build **host-only** images, which hold
only what this computer needs, and with `hostonly_cmdline="yes"` they store kernel parameters dracut detects. The
image is built on this computer, so host-only is kept; the files below switch off the stored parameters (ZFSBootMenu
passes the command line) and add the NVMe driver explicitly, because host-only detection does not always see through
ZFS and LUKS to the disk.

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

**Option C.** Recent dracut versions (dracut-ng) keep systemd-cryptsetup, which reads the `luks.*` parameters, in a
module of its own (`71systemd-cryptsetup` in dracut-ng's sources), and the `crypt` module does not pull it in. In a
host-only build, as here, dracut adds that module by itself only when it sees a LUKS device below the file systems,
which it may not see through ZFS; so the file names it. ([ZFS Pools on LUKS Volumes](../../general/luks.rst) and the
project's tests list only `zfs crypt`: they build generic images, which include every module that applies.) List
dracut's modules first:

```
(chroot) root # ls /usr/lib/dracut/modules.d/ | grep crypt
```

If the list contains a directory ending in `systemd-cryptsetup`, write the file as shown. If it does not, remove the
word `systemd-cryptsetup` from the `add_dracutmodules` line: older dracut versions include it in the `crypt` module.

**FILE** `/etc/dracut.conf.d/10-zfs.conf`

```
nofsck="yes"
# Do not store rd.luks.* or other parameters detected in the chroot; the luks.* parameters are used
hostonly_cmdline="no"
# ZFS root, and systemd-cryptsetup for the luks.* kernel parameters
add_dracutmodules+=" zfs crypt systemd-cryptsetup "
# Host-only detection does not always see through ZFS and LUKS to the NVMe SSD
add_drivers+=" nvme "
# The key file named by luks.key=; the initramfs lives inside the encrypted pool
install_items+=" /etc/cryptsetup-keys.d/zroot.key "
```

Make sure no crypttab is present; the `luks.*` parameters replace it:

```
(chroot) root # [ -e /etc/crypttab ] && mv /etc/crypttab /etc/crypttab.unused
```

**All options.** Build the initramfs of every installed kernel again:

```
(chroot) root # dracut -f --regenerate-all
```

- This is the command openSUSE's own package scripts run after a dracut update. For each kernel it writes
  `/boot/initrd-<version>`. Future kernel and ZFS module updates rebuild the initramfs with the same files.
- dracut writes the image with the file mode creation mask `077`, so the file is readable by root only.
- OpenZFS's dracut module also copies `/etc/hostid` and `/etc/zfs/zpool.cache` into the initramfs.

> **Warning**
> `generate-zbm` builds ZFSBootMenu's image with its own dracut directory, but dracut still reads `/etc/dracut.conf`
> and `/usr/lib/dracut/dracut.conf.d/`. Never name a key file in either: it would end up in the ZFSBootMenu image on
> the unencrypted ESP. Keep `install_items` lines for keys in `/etc/dracut.conf.d/` only.

**Check:**

```
(chroot) root # ls -l "/boot/initrd-${KVER}"
(chroot) root # lsinitrd "/boot/initrd-${KVER}" | grep -E 'zfs\.ko|zroot\.key|etc/hostid|zpool\.cache|nvme'
(chroot) root # lsinitrd "/boot/initrd-${KVER}" | grep -E 'systemd-cryptsetup$|cryptsetup-generator|dm-crypt|dm_crypt'
(chroot) root # lsinitrd -f etc/crypttab "/boot/initrd-${KVER}"
(chroot) root # lsinitrd "/boot/initrd-${KVER}" | grep -i -E 'microcode|ucode'
(chroot) root # grep CONFIG_BLK_DEV_NVME= "/usr/lib/modules/${KVER}/config"
(chroot) root # ls -A /boot/efi
```

| | Option A | Option B | Option C |
|---|---|---|---|
| mode of the initramfs | `-rw-------` | **`-rw-------`** | **`-rw-------`** |
| first `grep` lists | four files (below) | those, `etc/zfs/zroot.key` | those, `etc/cryptsetup-keys.d/zroot.key` |
| second `grep` lists | anything or nothing | anything or nothing | `systemd-cryptsetup`, its generator, `dm-crypt.ko` |
| `etc/crypttab` | nothing | nothing | **nothing** |

The four files every option has: `zfs.ko` (possibly with `.xz` or `.zst`), `etc/hostid`, `etc/zfs/zpool.cache` and
`nvme`.

- The microcode `grep` lists `GenuineIntel.bin` or `AuthenticAMD.bin` (in the early part of the image). If it lists
  nothing, run `zypper install ucode-intel ucode-amd` and the `dracut` line again.
- `CONFIG_BLK_DEV_NVME=m` means the NVMe driver is a module, which must be listed above; `=y` means it is built into
  the kernel, and then no `nvme.ko` is listed.
- `/boot/efi` prints nothing: no kernel or initramfs was put on the unencrypted ESP.
- Option C: `dm-crypt.ko` may be compressed (`.xz`, `.zst`). It is not listed when
  `grep CONFIG_DM_CRYPT= /usr/lib/modules/${KVER}/config` prints `=y` (built into the kernel). If it is missing
  otherwise, add `add_drivers+=" dm_crypt "` to `10-zfs.conf` and run the `dracut` line again.
- Option C: if `systemd-cryptsetup` is missing, check that `/usr/lib/systemd/systemd-cryptsetup` exists
  ([Option C only: creating the LUKS key file](#option-c-only-creating-the-luks-key-file)) and that the module
  names of the `add_dracutmodules` line exist in `/usr/lib/dracut/modules.d/`, then run the `dracut` line again.
- If an image for Option B or C is readable by others (`-rw-r--r--`), run `chmod 0600 /boot/initrd-*` and find out
  why before you continue (for example a `umask` line in a dracut configuration file). Never copy such an image out of
  the pool.

## Configuring the system

Why: the base configuration of any Tumbleweed system: locale, keyboard, time zone, host name, accounts and services.
The ESP is already in `/etc/fstab` ([Before the kernel: /etc/fstab and the boot loader packages][before-kernel]).

Locale, console keymap, time zone and host name. Replace `KEYMAP` (for example `us` or `de`), `ZONE` (for example
`Europe/Berlin`; `ls /usr/share/zoneinfo` lists them) and `HOSTNAME`:

```
(chroot) root # echo 'LANG=en_US.UTF-8' > /etc/locale.conf
(chroot) root # echo 'KEYMAP=KEYMAP' > /etc/vconsole.conf
(chroot) root # ln -sf ../usr/share/zoneinfo/ZONE /etc/localtime
(chroot) root # echo 'HOSTNAME' > /etc/hostname
```

The root password, and a user for daily work (replace `USERNAME`) in the group `wheel`. On openSUSE the group comes
from the package `system-group-wheel`, which a minimal installation may lack; the first line installs it if the group
does not exist (this guide could not confirm whether the pattern installs it):

```
(chroot) root # getent group wheel || zypper install -y system-group-wheel
(chroot) root # passwd
(chroot) root # useradd -m -G wheel USERNAME
(chroot) root # passwd USERNAME
```

openSUSE's `sudo` configuration has long asked **every** user for the **root** password (`Defaults targetpw`),
whatever their groups. The package `sudo-policy-wheel-auth-self` lets members of `wheel` use their own password
instead. This guide could not confirm the current default of Tumbleweed; this shows whether `targetpw` is set:
`grep -rs targetpw /etc/sudoers /etc/sudoers.d /usr/etc/sudoers /usr/etc/sudoers.d`. To use your own password with
`sudo`:

```
(chroot) root # zypper install -y sudo-policy-wheel-auth-self
```

Services. NetworkManager configures wired interfaces with DHCP by itself; for Wi-Fi, use `nmtui` or `nmcli` after the
first boot.

```
(chroot) root # systemctl enable NetworkManager.service
```

If `systemctl is-enabled wicked.service` prints `enabled`, the pattern installed openSUSE's other network service as
well; run `systemctl disable wicked.service`, so that only one of them manages the network.

**Security module.** Tumbleweed's installer enables SELinux on new installations; a system installed with
`zypper --root`, as here, gets only what the pattern pulls in, and this guide does not set up SELinux or AppArmor. The
files of the new system were written from the live system and carry no SELinux labels. If an SELinux configuration
was installed, keep SELinux from blocking anything until you have set it up with openSUSE's documentation:

```
(chroot) root # [ -f /etc/selinux/config ] && sed -i 's/^SELINUX=enforcing/SELINUX=permissive/' /etc/selinux/config
```

**Check:**

```
(chroot) root # cat /etc/locale.conf /etc/vconsole.conf /etc/hostname
(chroot) root # readlink /etc/localtime
(chroot) root # findmnt --verify --tab-file /etc/fstab
(chroot) root # id USERNAME
(chroot) root # sudo -l -U USERNAME
(chroot) root # systemctl is-enabled NetworkManager.service
(chroot) root # grep -s '^SELINUX=' /etc/selinux/config
```

`LANG=en_US.UTF-8`, `KEYMAP=` with your keymap, the host name, a link into `/usr/share/zoneinfo/` with your zone,
no errors from `findmnt --verify` for the ESP line, the user in group `wheel`, `sudo -l` listing what the user may run
(`(ALL) ALL` or similar), and `enabled`. The last command prints
nothing, `SELINUX=permissive` or `SELINUX=disabled`, never `SELINUX=enforcing`.

## Installing ZFSBootMenu

Everything in this section runs **inside the chroot**, as root. ZFSBootMenu is built from this repository, because
upstream ZFSBootMenu cannot unlock LUKS volumes; for the same steps in all three options, Options A and B use this
build too.

### Dependencies

Why: the build needs a Rust compiler, and the image needs the tools that go into it.

```
(chroot) root # zypper install -y --no-recommends git make gcc cargo rust binutils dracut systemd-boot \
  kexec-tools fzf mbuffer less efibootmgr dosfstools util-linux kmod procps ncurses-utils \
  xkeyboard-config libX11-data
```

| Package | Why |
|---|---|
| `git`, `make` | fetch the source and run the build |
| `gcc` | the C compiler and linker Rust needs to link the programs |
| `cargo`, `rust` | Tumbleweed's Rust toolchain; this repository needs `rustc` 1.88 or later |
| `binutils` | `strip` for `make install`, `strings` for `generate-zbm`, `objcopy` for a Check |
| `dracut` | builds ZFSBootMenu's image (already installed as Tumbleweed's initramfs generator) |
| `systemd-boot` | the EFI stub `linuxx64.efi.stub`; the package installs no boot loader on the ESP by itself |
| `kexec-tools` | starts Tumbleweed's kernel from ZFSBootMenu |
| `fzf` | the menu of the default (shell) runtime |
| `mbuffer`, `less` | optional tools of the image; without them the build warns (`column` is in `util-linux`) |
| `efibootmgr` | the firmware boot entries |
| `dosfstools`, `util-linux`, `kmod`, `procps`, `ncurses-utils` | tools of the image (`tput` is in `ncurses-utils`) |
| `xkeyboard-config`, `libX11-data` | keyboard layouts and compose rules for the optional Rust runtime |

`--no-recommends` installs only what these packages require: recommended packages of `systemd-boot` could bring
boot loader tools this system must not have, and the locks of
[Before the kernel: /etc/fstab and the boot loader packages][before-kernel] stop the ones named there.
If `zypper` stops with `No provider of ... found` for one of the optional names (`mbuffer`, `xkeyboard-config`,
`libX11-data`), run the command again without it; the build only warns. For the stub, `zypper search -f
/usr/lib/systemd/boot/efi/linuxx64.efi.stub` names the package that has it, if it is not `systemd-boot`. Never run
`bootctl install`: the stub is all this system needs from systemd-boot.

**Option C.** `cryptsetup` is already installed; it is what puts LUKS support into ZFSBootMenu's image. A dracut build
without it leaves LUKS out without a message.

**Check:**

```
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # cargo --version && rustc --version
(chroot) root # command -v dracut lsinitrd kexec fzf efibootmgr strings objcopy
(chroot) root # rpm -q sdbootutil grub2 perl-Bootloader
(chroot) root # ls -A /boot/efi
```

The stub exists, `rustc` is 1.88 or later, every program is found, the three packages are `not installed`, and
`/boot/efi` is still empty. Option C: `command -v cryptsetup dmsetup udevadm` finds all three. If `rpm -q` shows one
of the three packages as installed, the lock was missing: remove it as in
[Before the kernel: /etc/fstab and the boot loader packages][before-kernel] and add the locks.

### Building and installing

Why: `make` builds the programs `zbm`, `brush`, `generate-zbm` and `zbm-kcl`; `make install` installs them with the
dracut module and the configuration.

```
(chroot) root # git clone --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2 /usr/src/zbm
(chroot) root # make -C /usr/src/zbm
(chroot) root # make -C /usr/src/zbm install
```

> **Warning**
> The repository's default branch, `master`, holds the upstream layout without the LUKS code. Always use
> `claude/zbm-refactor`. Do not run `make core dracut`, which older guides show: it installs neither `zbm` nor
> `generate-zbm`.

- The Rust crates are in the clone (`vendor/`), so the build needs no network. The repository's `rust-toolchain.toml`
  names a newer compiler, but only `rustup` reads that file; Tumbleweed's `cargo` builds with its own `rustc`.
- `make` runs `cargo build --release --locked`; it takes several minutes.
- `make install` puts `generate-zbm` and `zbm-kcl` in `/usr/bin`; `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
  the shell runtime in `/usr/share/zfsbootmenu`; the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in
  `/usr/lib/dracut/modules.d`; the mkinitcpio hooks in `/usr/lib/initcpio`; `config.yaml`, `mkinitcpio.conf` and
  `dracut.conf.d/` in `/etc/zfsbootmenu`; manual pages; and examples in `/usr/share/examples/zfsbootmenu`.
- `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files it
  installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files with other names there are kept. This guide therefore writes
  `config.yaml` after `make install`, and keeps its own dracut settings in files of its own.
- The dracut module `zfsbootmenu-measured` is also included in Tumbleweed's own initramfs from now on. It only copies
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

Optional: Cargo's build directory is no longer needed and takes most of the space; delete it before the snapshot of
[Finalizing the installation](#finalizing-the-installation), so that the snapshot does not keep it. The next update
of ZFSBootMenu then builds from scratch.

```
(chroot) root # rm -rf /usr/src/zbm/target
```

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
| `Global.DracutConfDir` | `/etc/zfsbootmenu/dracut.conf.d` | dracut settings of **ZFSBootMenu's** image only |
| `Global.PreHooksDir`, `Global.PostHooksDir` | as shown | programs run before and after a build |
| `Global.InitCPIOConfig` | as shown | used only with mkinitcpio; kept from the installed file |
| `Components.Enabled` | `false` | no separate kernel and initramfs; only the EFI file |
| `EFI.Enabled` | `true` | build the EFI file: kernel, initramfs and command line in one |
| `EFI.ImageDir` | `/boot/efi/EFI/zbm` | where the EFI file is written |
| `EFI.Versions` | `false` | one image and one `-backup` copy with fixed names, so the firmware entries never change |
| `Kernel.Prefix` | `vmlinuz` | fixes the image names to `vmlinuz.EFI` and `vmlinuz-backup.EFI` |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's own** kernel, built into the EFI file |

`DracutConfDir` is never the system's `/etc/dracut.conf.d`. Tumbleweed's kernel name would give the same image names
as `Kernel.Prefix`; the key keeps them so.

`Kernel.CommandLine`, per option:

- **Options A and B:** `quiet loglevel=0 zbm.luks=off`. `zbm.luks=off` means: do not unlock any LUKS volume. Without
  it ZFSBootMenu would offer every LUKS volume it finds, for example on a USB disk plugged in at boot.
- **Option C:** `quiet loglevel=0 zbm.luks=UUID=<LUKS UUID>`: unlock only this container. The UUID is that of the
  **LUKS container** (`cryptsetup luksUUID`), not the pool's GUID and not a file system UUID; it is opened as
  `luks-<UUID>`.

`generate-zbm` builds the image with the kernel of the highest version in `/boot`, here `vmlinuz-${KVER}` (the link
`/boot/vmlinuz` names the same kernel). The ZFS module for that kernel must be installed, which the lock of
[Locking the kernel and ZFS together](#locking-the-kernel-and-zfs-together) keeps so.

Options you may add later: `zbm.timeout=SECONDS` changes the countdown (default 10; `-1` always shows the menu, `0`
boots at once); `loglevel=7` shows ZFSBootMenu's messages for troubleshooting.

ZFSBootMenu's dracut configuration: build an image that does not depend on the machine it was built on. The first
image is built inside a chroot on the live system, whose kernel and loaded drivers are not those of the installed
system, and openSUSE's dracut default is host-only. The project's build container does the same.

**FILE** `/etc/zfsbootmenu/dracut.conf.d/generic.conf`

```
# Build an image that does not depend on the machine it was built on: the first image is built
# inside a chroot on a live system, whose kernel and loaded drivers are not those of the installed system.
hostonly="no"
```

dracut reads the files of its configuration directories in the order of their names, so `generic.conf` comes after
openSUSE's `01-dist.conf` and overrides its `hostonly="yes"`. The image is larger this way; the 1 GiB ESP has room. Do
not add dracut's `crypt` module here: ZFSBootMenu unlocks LUKS volumes itself.

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
runtime after a beta. Only the Rust runtime has the locked menu, measured boot, TPM2 PIN tokens that systemd 262 or
later enrolled with `--tpm2-with-pin=yes`, and keyboard layouts other than US at the passphrase prompt. To choose it:

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

Keyboard: with the shell runtime the passphrase prompt uses the US layout. With the Rust runtime, either

- add `zbm.console=drm` to `CommandLine` (the console on the display then types with the image's first layout); the
  image needs the `evdev` module and a DRM driver (`simpledrm` on the firmware's framebuffer is enough), for example
  with `add_drivers+=" evdev "` in a file in `/etc/zfsbootmenu/dracut.conf.d/`. Without them ZFSBootMenu stays on the
  kernel console, with the US layout;
- or, on the default kernel console, press **Ctrl-K** at the prompt to choose a layout.

The image gets the layouts of `zfsbootmenu_keyboard_layouts`; when it is unset, the layout of `XKBLAYOUT` in
`/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`. This guide wrote only `KEYMAP=` there, so name your layout, for
example:

```
(chroot) root # echo 'zfsbootmenu_keyboard_layouts="de us"' >> /etc/zfsbootmenu/dracut.conf.d/runtime.conf
```

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
  (`/boot/vmlinuz-${KVER}` or the link `/boot/vmlinuz`). `--debug` prints every command.
- The first run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`; the second renames it to `vmlinuz-backup.EFI` and writes a
  new one.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | `make install` was not run |
| `failed to install essential executable '<name>'` | a required program is missing, often `kexec` or `fzf` |
| `Unable to find UEFI stub loader at default locations` | `systemd-boot` (the stub) is not installed |
| `ManageImages not enabled, no action taken` | `config.yaml` was overwritten by `make install`; write it again |
| `Unable to locate libgcc_s.so` | the GCC runtime library is missing (`libgcc_s1`) |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (see below) |

`fzf` is needed by the shell runtime only. For the host ID message: the SPL module's `spl_hostid` parameter differs from
`/etc/hostid` (the check is skipped while the parameter is 0). The image takes `/etc/hostid`, which must be `00bab10c`
(`od -A n -t x4 /etc/hostid`).

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
| `zbm-kcl` prints | `quiet loglevel=0 zbm.luks=off` | the same | `quiet loglevel=0 zbm.luks=UUID=` and the LUKS UUID |
| first `grep` lists | `etc/hostid`, `zfs.ko`, the `nvme` driver | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

`zfs.ko` may be compressed. The second `grep` must print nothing: no key file and no crypttab may be in ZFSBootMenu's
image. If it prints a line, do not boot this image; find out why first (a key file named in `/etc/dracut.conf` or in
`/usr/lib/dracut/dracut.conf.d/`). If `cryptsetup` is listed for Options A or B, that is fine: it is in the image
whenever the package is installed, and `zbm.luks=off` keeps it unused. The `nvme` line lists the driver module only if
Tumbleweed's kernel builds it as a module (see the `CONFIG_BLK_DEV_NVME` Check of [Configuring the
initramfs](#configuring-the-initramfs)). `df` shows room for at least one more copy of the image, which the next step
makes.

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

Why: put back Tumbleweed's own name resolution, take a snapshot of the fresh system to return to, and export the pool
cleanly, which every later import expects.

```
(chroot) root # if [ -e /root/resolv.conf.orig ] || [ -L /root/resolv.conf.orig ]; then \
  mv -f /root/resolv.conf.orig /etc/resolv.conf; fi
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

- The first line puts the original `/etc/resolv.conf` back, if there was one; it prints nothing either way.
- The snapshot `@install` is the first thing to roll back to.

**Check:**

```
root # zpool list
root # ls /dev/mapper
```

`zpool list` reports *no pools available*. Option C: `/dev/mapper` contains only `control` (and the live system's
own devices, if it uses any). If the export fails because the pool is busy, leave any shell that is still inside
`/mnt` and repeat the unmount and export commands.

Then reboot and remove the live medium:

```
root # reboot
```

## First boot

From here on, `root #` means root on the **installed system**.

What you see with the default (shell) runtime:

| Step | Option A | Option B | Option C |
|---|---|---|---|
| 1 | | | `Unlock LUKS volume <device>` and `Passphrase (leave empty to skip):`. Type the LUKS passphrase |
| 2 | the countdown `Booting zroot/ROOT/opensuse in 10 seconds` | the same | the same |
| 3 | | the screen clears; `Enter passphrase for 'zroot':`. Type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Tumbleweed's kernel | the same | the same |
| 5 | Tumbleweed starts to the login prompt | the same, **without** asking again | the same, **without** asking again |

- The countdown line also shows `[RETURN] boot now` and `[ESCAPE] boot menu`. **Enter** boots at once; **Escape**
  during the countdown opens the menu, which lists `zroot/ROOT/opensuse`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- The kernel list of the boot environment (**MOD+K** in the menu) shows each kernel twice: as
  `/boot/vmlinuz-<version>-default` and as the link `/boot/vmlinuz` with `/boot/initrd`, which name the kernel
  installed last. Either boots the same files; the default is the one with the version.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: `zfs load-key` allows three attempts. After three wrong passphrases ZFSBootMenu opens its emergency shell
  with `unable to load key for zroot/ROOT/opensuse`. Type `exit` there; the menu then asks again.
- With the Rust runtime the same questions appear as dialogs (`Unlock <volume>` for Option C, a ZFS key dialog for
  Option B), and the countdown reads `Booting <environment> in <N> s. Press any key to choose another.`

If Tumbleweed asks for a passphrase a second time, or stops in an emergency shell, see
[The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock].

Log in as root and check the system:

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status zroot
root # hostid
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
root # systemctl --failed
root # cat /sys/kernel/security/lsm
```

**Option B:**

```
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/opensuse
```

Both datasets show `aes-256-gcm`, the encryption root `zroot` and the key status `available`. Only the encryption
root has a key location: `zroot` shows `file:///etc/zfs/zroot.key`, and `zroot/ROOT/opensuse` shows `none`.

**Option C:**

```
root # LUKS_UUID="$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)"
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

- `/proc/cmdline` holds `quiet`, `root=zfs:zroot/ROOT/opensuse` and `spl.spl_hostid=0x00bab10c`; Option C also
  `luks.crypttab=no`, `luks.uuid=` and `luks.key=`.
- `/` is `zroot/ROOT/opensuse`, and `hostid` prints `00bab10c`.
- `zpool status` shows the pool `ONLINE`, on `nvme0n1p2` (Options A and B) or on `luks-<UUID>` (Option C).
- Option B: `aes-256-gcm`, `file:///etc/zfs/zroot.key` and `available`. Option C: `type: LUKS2`, `sector size: 4096`
  and `flags: discards`.
- The passphrase was asked for never (Option A), or once per boot, by ZFSBootMenu (Options B and C).
- `systemctl --failed` prints `0 loaded units listed`.

`/sys/kernel/security/lsm` lists the kernel's active security modules; this guide set up neither SELinux nor AppArmor
(see [Configuring the system](#configuring-the-system)).

`BootCurrent` names the entry the firmware used. If it is the number of the `ZFSBootMenu` entry, the firmware honours
entries and `\EFI\BOOT\BOOTX64.EFI` is only a reserve. If it is another number, the firmware started the copy; the
post-hook `50-bootx64.sh` of [Creating the firmware boot entries](#creating-the-firmware-boot-entries) keeps it
current, so check after each `generate-zbm` that it ran (see
[Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image)).

Check the network and the locks once more on the running system:

```
root # nmcli general status
root # zypper locks
```

NetworkManager reports `connected` (on a wired network), and the locks of the installation are listed (the boot
loader packages, `kernel-default` and the ZFS patterns).

Once the system has booted, do these three things:

1. **Option C:** make the first LUKS header backup:
   [Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header).
2. Enable the monthly scrub and TRIM: [Scrubbing and trimming](#scrubbing-and-trimming).
3. Keep the live medium until the system has booted several times, including once after a kernel update.

## Usage

### Updating the system

Tumbleweed is updated with `zypper dup`. Take a snapshot of the boot environment before every update;
[Boot environments, snapshots and rollback](#boot-environments-snapshots-and-rollback) shows how to go back:

```
root # zfs snapshot zroot/ROOT/opensuse@pre-update-$(date +%Y%m%d)
root # zypper refresh
root # zypper dup
```

> **Important**
> Nothing runs `generate-zbm` for you. After every `zypper dup` that updated dracut, systemd or cryptsetup, run
> `dracut -f --regenerate-all` and then `generate-zbm`; the table and the paragraph after it say why.

With the locks of [Locking the kernel and ZFS together](#locking-the-kernel-and-zfs-together), this updates
everything except the kernel and ZFS. What happens on its own, and what you must do:

| Update | What happens | What you do |
|---|---|---|
| ordinary packages | installed | nothing |
| **dracut**, **systemd** | see below | `dracut -f --regenerate-all`, then `generate-zbm` |
| **cryptsetup** (Option C) | nothing for ZFSBootMenu's image | `generate-zbm` |
| **kernel and ZFS** | held back by the locks | the procedure below |

Both images contain files of dracut and systemd: dracut's package scripts rebuild Tumbleweed's initramfs of every
kernel, with your files in `/etc/dracut.conf.d/`; this guide could not confirm that systemd's scripts do.
`rpm -qa --last | head -n 30` lists the packages of the last update, newest first.

**Moving the kernel and ZFS on together.** Check whether the `filesystems` project has a module for the newest
kernel, as during the installation:

```
root # zypper refresh
root # zypper search -s -x kernel-default zfs-kmp-default
```

If the newest `kernel-default` (for example `7.3.1-1.1`, kernel `7.3.1-1`) has a `zfs-kmp-default` with the same
kernel after `_k` (`_k7.3.1_1`), lift the locks for one update, and lock again:

```
root # zfs snapshot zroot/ROOT/opensuse@pre-kernel-$(date +%Y%m%d)
root # zypper removelock kernel-default 'zfs*' 'libzfs*' 'libzpool*' 'libnvpair*' 'libuutil*'
root # zypper dup
root # zypper addlock kernel-default 'zfs*' 'libzfs*' 'libzpool*' 'libnvpair*' 'libuutil*'
```

Before you answer `y`, read the list `zypper dup` shows: it must install the new `kernel-default` **and** the new
`zfs-kmp-default` for it. If it installs a kernel without the module, answer `n` and lock again.

**Check** before rebooting:

```
root # for m in /usr/lib/modules/*-default; do v="${m##*/}"; echo "${v}: $(modinfo -k "${v}" -F version zfs 2>&1)"; done
root # ls -l /boot/initrd-* /boot/vmlinuz /boot/initrd
root # dracut -f --regenerate-all
root # generate-zbm
root # ls -l /boot/efi/EFI/zbm/
root # zypper locks
```

Every kernel prints a ZFS version (the newest one the new version), every initramfs is `-rw-------`, the links (if
present) point at the new kernel, `dracut` rebuilds every initramfs (the update may also have changed dracut or
systemd), `generate-zbm` builds without an error and `vmlinuz.EFI` has the current time, and the locks are back. If the
newest kernel has no module, see [No ZFS module for the kernel](#no-zfs-module-for-the-kernel) before you reboot.
Tumbleweed normally keeps the previous kernel as well (`multiversion.kernels` in `/etc/zypp/zypp.conf`;
`grep multiversion /etc/zypp/zypp.conf` shows the setting, and this guide did not check its default), and
ZFSBootMenu's kernel list (**MOD+K**) can start it.

**A desktop.** The installation has no graphical desktop. To install GNOME, for example:

```
root # zypper install -t pattern gnome
root # systemctl set-default graphical.target
```

Read the list of packages first: no package of the warning in
[Before the kernel: /etc/fstab and the boot loader packages][before-kernel] may be among them.

### Regenerating the ZFSBootMenu image

ZFSBootMenu's image contains the kernel, the ZFS module and tools, cryptsetup and the other tools of **this system at
the time of the build**. Rebuild it after updates of the ZFS packages, of the kernel, of cryptsetup, of dracut and
systemd, and after changing `/etc/zfsbootmenu/`:

```
root # generate-zbm
```

openSUSE has no hook that runs `generate-zbm` after updates, and this repository has none for it either; this guide
proposes none. Run it by hand, as the Important note in [Updating the system](#updating-the-system) says. Kernel and
ZFS updates happen only in the locked procedure there, which ends with `generate-zbm`.

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

**Check** after every build:

```
root # ls -l /boot/efi/EFI/zbm/
root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
```

`vmlinuz.EFI` has the current time, and the command line is the one of
[Configuring generate-zbm](#configuring-generate-zbm).

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Snapshots of
it are taken on the running system (`zfs snapshot zroot/ROOT/opensuse@NAME`) or in ZFSBootMenu.

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
| **MOD+C** | clone | a new boot environment that depends on the snapshot; for booting an earlier state for a while |
| **MOD+X** | clone and promote | a new boot environment that keeps the snapshots; the old one can be destroyed |
| **Enter** | duplicate | an independent copy (`zfs send`, `zfs recv`); uses the space the snapshot refers to |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

**Rust runtime.** Press **2** or **s** for the snapshots of the selected environment, then **r** to roll back; a
review page shows what is lost, and the change is made once the target's name is typed. **?** lists the keys of
every view.

Example: going back after a bad update.

1. Before the update, on the running system: `zfs snapshot zroot/ROOT/opensuse@pre-update`.
2. After a bad update, reboot, press **Escape** in the countdown, select `zroot/ROOT/opensuse`, press **MOD+S**,
   select `@pre-update`, press **MOD+R** and confirm.
3. Or, to try the old state without losing the new one, press **MOD+C** (clone) instead and boot the clone.

- Rollback and the other changes need the pool imported read/write; ZFSBootMenu imports read-only and re-imports when
  needed. In Option B that re-import needs the key again; the key source set in
  [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties) saves the second prompt.
- A rollback of the boot environment does not touch `zroot/home`.
- A boot environment you create by hand needs `mountpoint=/` and `canmount=noauto`; it inherits
  `org.zfsbootmenu:commandline` from `zroot/ROOT`.
- Snapshots and clones contain their `/boot` and `/usr/lib/modules`, so each boots with the kernel, ZFS module and
  initramfs (and, in Options B and C, the key file) it had when the snapshot was taken. After the key file or the
  passphrase was replaced, an older snapshot's initramfs holds the old one, and its system asks for the passphrase or
  stops in an emergency shell.
- The zypper locks, the RPM database and `/etc/dracut.conf.d/` are files in the boot environment too: a rollback
  brings back the ones of the snapshot. After a rollback across a kernel update, run `generate-zbm` so that
  ZFSBootMenu's image again uses the kernel and ZFS of the running system.

`zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the `org.zfsbootmenu:commandline` property
that every boot environment inherits; see `man zbm-kcl`. Without a dataset name `zbm-kcl` acts on the root file system,
`zroot/ROOT/opensuse`, and a change there sets a property of its own on that boot environment only.

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
> Rebuild Tumbleweed's initramfs now. It holds a copy of the old file, and OpenZFS's dracut module does not fall back
> to a prompt for a `file://` key location: with the old file the next boot stops in an emergency shell.

```
root # dracut -f --regenerate-all
root # ls -l /boot/initrd-*
```

Every image is still `-rw-------`. ZFSBootMenu's image holds no key and needs no rebuild.

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
root # dracut -f --regenerate-all
root # for i in /boot/initrd-*; do \
  lsinitrd -f etc/cryptsetup-keys.d/zroot.key "$i" | cmp - zroot.key && echo "$i ok"; done
```

`luksAddKey` asks for the passphrase. Rebuilding the initramfs is required: it contains a copy of the key file.
Continue only when the loop prints `ok` for every image and `ls -l /boot/initrd-*` shows them all
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
  of passphrase or key file.

### Scrubbing and trimming

```
root # zpool scrub zroot
root # zpool trim zroot
root # zpool status -t zroot
```

A scrub reads and checks every block; a full TRIM complements `autotrim`. OpenZFS ships the systemd timers
`zfs-scrub-monthly@.timer` and `zfs-trim-monthly@.timer`; this guide could not confirm that the `filesystems`
project's packages install them, so look first, and enable them if they are listed:

```
root # systemctl list-unit-files 'zfs-*monthly@.timer'
root # systemctl enable --now zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

If they are not listed, run the three commands above by hand once a month.

Option C: TRIM reaches the SSD because the flag is in the LUKS header; `lsblk -D` shows non-zero discard sizes for
`luks-<UUID>`. The SSD's own health: `nvme smart-log /dev/nvme0` (package `nvme-cli`).

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
- If the repository raises its minimum Rust version above Tumbleweed's `rustc`, `make` says so; wait for
  Tumbleweed's update.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm it works before relying on it; the
backup entry still boots the previous image.

### Relaxing the pool compatibility

A step most readers can skip. The pool was created with `compatibility=openzfs-2.2-linux`. A newer OpenZFS on this
system knows newer features; to enable them, first rebuild ZFSBootMenu's image, then raise the set:

```
root # generate-zbm
root # zpool set compatibility=openzfs-2.3-linux zroot
root # zpool upgrade zroot
```

- Update every system that imports the pool before enabling new features, starting with ZFSBootMenu, and check that
  your rescue medium's ZFS is new enough. Use the newest set that the installed ZFS **and** the rescue medium both
  know (`ls /usr/share/zfs/compatibility.d/`); a step at a time is the cautious way.
- When ZFSBootMenu's ZFS is older than the pool's features it shows *Unsupported features detected* and refuses
  read/write operations, or cannot import the pool at all.
- One `generate-zbm` run refreshes `vmlinuz.EFI` and, through the post-hook, `BOOTX64.EFI`; `vmlinuz-backup.EFI`
  is then the previous image. Before `zpool upgrade`, boot the new image once and run `generate-zbm` a second time,
  so that the backup holds the new ZFS too. A `known-good.EFI` copy, if you made one, is never updated: copy the new
  image over it, or delete it and its entry (`efibootmgr -b XXXX -B`).
- A snapshot of the boot environment keeps an older ZFS module; after `zpool upgrade`, rolling back across the ZFS
  update that brought the new features leaves a system that may not import its own pool.

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
`zypper install cryptsetup` in the chroot, run `generate-zbm`, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

After ZFSBootMenu started Tumbleweed's kernel, Tumbleweed's initramfs could not use the key file:

- **Option B**: `zfs load-key` fails and the boot stops in dracut's emergency shell; OpenZFS's dracut module does not
  ask for the passphrase when `keylocation` is a file.
- **Option C**: systemd-cryptsetup asks for the passphrase (on the screen, or through Plymouth's prompt).

Type the passphrase where one is asked for. In the emergency shell (Option B), `zfs load-key -L prompt zroot` loads
the key and `exit` lets the boot go on; this guide could not confirm that Tumbleweed's initramfs then continues, and
the live medium (below) always works. Then, on the running system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitrd "/boot/initrd-$(uname -r)" | grep -E 'zroot\.key|systemd-cryptsetup$'
```

Causes, in order of likelihood:

- the key file is not in the initramfs: `/etc/dracut.conf.d/10-zfs.conf` is missing or misspelled. Fix it and run
  `dracut -f --regenerate-all`;
- Option C: the command line lacks the `luks.*` parameters, or carries another UUID than
  `cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks`;
- Option C: `systemd-cryptsetup` is not in the initramfs (the `systemd-cryptsetup` or `crypt` module is missing from
  `add_dracutmodules`, or `/usr/lib/systemd/systemd-cryptsetup` is not installed);
- Option B: the passphrase was changed without rebuilding the initramfs
  ([Changing the passphrase](#changing-the-passphrase));
- an old snapshot or clone was booted, whose initramfs holds an older key.

If ZFSBootMenu **itself** asks twice in Option C, `zbm.luks=` is missing from its command line, and it offers every
LUKS volume it finds.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment, for pairs such as `vmlinuz-<version>-default` and
`initrd-<version>-default`. In the recovery shell (Option B: `zfs load-key -L prompt zroot` first; the key file is
not in ZFSBootMenu's image):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/opensuse
root # ls -l MOUNTPOINT/boot MOUNTPOINT/usr/lib/modules
```

`mount_zfs` prints the mount point to use for `MOUNTPOINT`. Causes:

- `/boot` holds no `vmlinuz-*`, or the links point at a missing `/usr/lib/modules/<version>/vmlinuz`: the kernel
  package scripts did not run, or `sdbootutil` handled the kernel and put it on the ESP (look for files below
  `/boot/efi` other than `EFI/zbm` and `EFI/BOOT`). Repair from the live medium, remove the boot loader packages of
  [Before the kernel: /etc/fstab and the boot loader packages][before-kernel], and run
  `zypper install -f kernel-default` in the chroot;
- a kernel has no initramfs beside it (`/etc/fstab` was missing when it was installed): run
  `dracut -f --regenerate-all` in the chroot;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

- In ZFSBootMenu the default import policy adopts the host ID of the pool when it cannot import it with its own, so
  this usually resolves itself. Otherwise, in the recovery shell: `zpool import -f -N zroot`, then `exit`.
- In Tumbleweed's emergency shell: `zpool import -f -N zroot`, then `exit`.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; run
  `dracut -f --regenerate-all` and `generate-zbm`. The usual cause is a live system that imported the pool and was
  shut down without `zpool export`. Always export the pool on a rescue medium.

### The recovery shell and its tools

| Command | Use |
|---|---|
| `zfsbootmenu` (or `exit`) | back to the menu |
| `zpool`, `zfs` | the ZFS tools of the image; `zpool import`, `zpool status` |
| `set_rw_pool POOL`, `set_ro_pool POOL` | re-import the pool read/write or read-only |
| `mount_zfs DATASET` | mount a file system at a temporary place and print it |
| `zfs-chroot DATASET` | chroot into a boot environment to repair it (read/write only after `set_rw_pool`) |
| `zsnapshots DATASET` | the snapshot browser |
| `zkexec DATASET KERNEL INITRAMFS` | boot a chosen kernel and initramfs directly |
| `mount_esp DEVICE` | mount an ESP |
| `zluks` (`list`, `status`, `open DEVICE [NAME]`, `close`) | Option C: unlock, list and close LUKS volumes |
| `cryptsetup`, `dmsetup` | Option C: the LUKS tools themselves |
| `logs`, `zlogtail`, `zreport`, `zbmcmdline` | logs, a report for bug reports, ZFSBootMenu's command line |
| `help` | the built-in documentation |
| `reboot`, `poweroff`, `firmware-setup` | leave |

### Repairing the system from the live medium

Boot the Tumbleweed live image, become root and install ZFS as in [Installation media](#installation-media) (Option C:
cryptsetup too). The live image's kernel must have a ZFS module, as during the installation; any live system with
OpenZFS 2.2 or later can import the pool too. Then:

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

**All options.** Give the live system the installed system's host ID first, so that the import does not write the
live system's own ID into the pool (the cause named in
[The pool is refused: wrong host ID, or last used by another system][hostid-refused]); use the value of the installed
system if you changed it:

```
root # zgenhostid -f 0x00bab10c
root # hostid
root # zpool import -N -R /mnt zroot
```

`hostid` must print `00bab10c`. If the import refuses the pool because it "was last accessed by another system", or
was not exported, repeat it as `zpool import -f -N -R /mnt zroot`.

**Option B:**

```
root # zfs load-key -L prompt zroot
```

**All options.**

```
root # zfs mount zroot/ROOT/opensuse
root # zfs mount zroot/home
root # mount "${ESP_PART}" /mnt/boot/efi
root # mount --rbind --make-rslave /dev /mnt/dev
root # mount --rbind --make-rslave /proc /mnt/proc
root # mount --rbind --make-rslave /sys /mnt/sys
root # mount -t tmpfs tmpfs /mnt/run
root # cp -P /mnt/etc/resolv.conf /mnt/root/resolv.conf.orig
root # rm -f /mnt/etc/resolv.conf
root # cp -L /etc/resolv.conf /mnt/etc/resolv.conf
root # chroot /mnt /bin/bash --login
```

The stored TRIM flag is applied by `cryptsetup open` without options. The live system's ZFS module must be at least as
new as the pool's features. Repair what is needed; before leaving the chroot, put the name resolution back with
`mv -f /root/resolv.conf.orig /etc/resolv.conf`. Then leave the chroot and finish as in
[Finalizing the installation](#finalizing-the-installation), without the snapshot: unmount, `zpool export zroot`, and
for Option C `cryptsetup close`.

### No ZFS module for the kernel

`modinfo -k VERSION zfs` prints `modinfo: ERROR: Module zfs not found.` for a kernel in `/usr/lib/modules`, or dracut
stops with `Failed to find module 'zfs'`. The installed `zfs-kmp-default` was built for another kernel. Replace
`VERSION` with the kernel version, such as `7.3.1-1-default`:

```
root # rpm -q kernel-default zfs-kmp-default
root # zypper search -s -x zfs-kmp-default
```

- If a `zfs-kmp-default` for that kernel exists in the repository, install it, then rebuild:

  ```
  root # zypper removelock 'zfs*' 'libzfs*' 'libzpool*' 'libnvpair*' 'libuutil*'
  root # zypper install zfs zfs-kmp-default
  root # zypper addlock 'zfs*' 'libzfs*' 'libzpool*' 'libnvpair*' 'libuutil*'
  root # dracut -f --regenerate-all
  root # generate-zbm
  ```
- If none exists, do not boot that kernel. Remove it while the old kernel and its module are still installed:
  `zypper removelock kernel-default`, `zypper remove kernel-default-VERSION_OF_THE_PACKAGE` (as `rpm -q
  kernel-default` prints it), `zypper addlock kernel-default`. Then run `generate-zbm`, so that ZFSBootMenu's image
  uses a kernel with a module. If you already booted it, choose the older kernel with **MOD+K** in ZFSBootMenu.

### zypper reports a problem with the kernel or ZFS packages

`zypper dup` stops with `Problem: ...` and a list of solutions that name `kernel-default`, `zfs`, `zfs-kmp-default`
or one of the ZFS libraries (`libzfs...`, `libzpool...`, `libnvpair...`, `libuutil...`). Another package wants a newer
version of a locked one. Read the solutions: choose the one that keeps the installed version (worded as not
installing, or keeping, the newer one), never one that removes `zfs`, `zfs-kmp-default` or `kernel-default`. If none
fits, answer `c` to cancel; nothing is changed. Then check whether the kernel and ZFS can move on together, as in
[Updating the system](#updating-the-system), and run `zypper dup` again afterwards. This guide could not confirm
which problems current Tumbleweed reports with these locks, nor whether the service that removes old kernels
(`purge-kernels.service`) fails while `kernel-default` is locked; `systemctl status purge-kernels.service` after the
first kernel update shows it.

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption leaves the pool's structure readable without the passphrase: dataset and snapshot names, the
  dataset hierarchy, properties (including `org.zfsbootmenu:commandline`), file sizes and holes, and deduplication
  tables. File and volume data, file attributes, ACLs, permission bits and directory listings are encrypted.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Tumbleweed's initramfs in
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
  dataset contain it too; treat them like the pool. This is why `sdbootutil`, which copies initramfs images to the
  ESP, must not be installed on this system.
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
  module of the `filesystems` project would need its key enrolled as well.)
- Once the pool is unlocked, the recovery shell and the command-line editor are open to anyone at the keyboard.
- Hooks run as root in ZFSBootMenu; install only hooks you have read.
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed
  before updating, keep the backup entries, and keep the rescue medium.
- The lock keeps the kernel at its installed version until the `filesystems` project has a ZFS module for a newer one.
  Tumbleweed does not update an older kernel; until the kernel moves on, the system runs without the kernel's newest
  security fixes. Check for a matching module regularly ([Updating the system](#updating-the-system)).
- The `filesystems` project is a Build Service project, not one of Tumbleweed's official repositories, and marks its
  ZFS packages as experimental. Its signing key is trusted for every package it offers.
- This guide sets up neither SELinux nor AppArmor (see [Configuring the system](#configuring-the-system)).

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
- openSUSE: [Tumbleweed downloads](https://get.opensuse.org/tumbleweed/), `zypper(8)` (locks, `dup`),
  `dracut.conf(5)`, `systemd-cryptsetup-generator(8)` for the `luks.*` parameters, and the scripts of
  [suse-module-tools](https://github.com/openSUSE/suse-module-tools) that install kernels and build the initramfs
- OpenZFS: [OpenZFS on openSUSE][openzfs-opensuse] (the `filesystems` repository) and
  its page *openSUSE Tumbleweed Root on ZFS* (with GRUB instead of ZFSBootMenu, and another layout), and the
  OpenZFS documentation (<https://openzfs.github.io/openzfs-docs/>)
- `gdisk(8)`, `cryptsetup(8)`

[openzfs-opensuse]: https://openzfs.github.io/openzfs-docs/Getting%20Started/openSUSE/index.html
