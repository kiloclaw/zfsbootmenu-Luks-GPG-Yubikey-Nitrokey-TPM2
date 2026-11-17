# Arch Linux on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install Arch Linux from scratch on a **single NVMe SSD** with a **ZFS root pool**. The
system boots with the ZFSBootMenu build from this repository (branch `claude/zbm-refactor`). Arch is installed with
`pacstrap` from the official Arch ISO, to which ZFS is added from the **archzfs** repository. Arch's own initramfs is
built by **mkinitcpio**; dracut is installed only to build ZFSBootMenu's image.

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Arch's initramfs then unlocks the pool again
with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from the documentation and sources of Arch Linux, archzfs, OpenZFS and this repository. The
> project has not run it from start to finish. `claude/zbm-refactor` is a development branch, not a release.
> The project's VM tests boot ZFSBootMenu images built with dracut, but only a test system whose initramfs is
> built with dracut: Arch Linux's mkinitcpio initramfs, which this guide uses, is not covered by them.
> Keep a live medium at hand until the system has booted several times, and read
> [What the project has tested](#what-the-project-has-tested) before you start.

> **Important**
> Upstream ZFSBootMenu cannot unlock LUKS volumes; its LUKS support exists only in this repository. The AUR packages
> `zfsbootmenu` and `zfsbootmenu-efi-bin` build or ship **upstream** ZFSBootMenu: do not install them. This guide
> builds ZFSBootMenu from this repository for all three options, so that the steps are the same.

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
  - [Releasing the SSD](#releasing-the-ssd)
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
  - [The live system cannot build the ZFS module](#the-live-system-cannot-build-the-zfs-module)
  - [pacman -Syu stops on zfs-linux-lts](#pacman--syu-stops-on-zfs-linux-lts)
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
| Second unlock, by Arch's initramfs | none | key file (1) | key file (2) |
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
3. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/arch` with a countdown of 10 seconds.
   When the countdown ends, the pool's `bootfs` is booted; a key press opens the menu instead.
4. **Option B**: the `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in
   ZFSBootMenu's image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime this
   prompt comes **after** the countdown.
5. ZFSBootMenu loads Arch's kernel `/boot/vmlinuz-linux-lts` and its initramfs `/boot/initramfs-linux-lts.img` from
   `/boot` **inside the boot environment** and starts the kernel with `kexec`. It passes the command line stored in
   the ZFS property `org.zfsbootmenu:commandline`, the root argument `zfs=zroot/ROOT/arch` and the host ID.
   **Option C**: before `kexec` it exports the pool and closes the container, which wipes the volume key from
   memory.
6. Arch's initramfs (mkinitcpio, with the archzfs `zfs` hook) starts in the new kernel:
   - **Option A**: the `zfs` hook imports the pool and mounts the root file system.
   - **Option B**: the `zfs` hook imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the
     initramfs**, without asking.
   - **Option C**: the `encrypt` hook unlocks the container again with the key file
     `/etc/cryptsetup-keys.d/zroot.key` **inside the initramfs**, without asking; then the `zfs` hook imports the
     pool.

Why a second unlock (Options B and C): `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. The boot environment's own initramfs must unlock the pool again, exactly as it would without
ZFSBootMenu. ZFSBootMenu does not replace Arch's initramfs.

### Where the keys are

**Option A.** There are none.

| Key | Option | Stored where | Used by |
|---|---|---|---|
| ZFS passphrase | B | your memory | ZFSBootMenu at every boot; a live system for repairs |
| The same passphrase in `/etc/zfs/zroot.key` | B | in the pool (see below) | Arch's initramfs |
| LUKS passphrase (key slot 0) | C | your memory | ZFSBootMenu at every boot; a live system for repairs |
| Key file `/etc/cryptsetup-keys.d/zroot.key` (slot 1) | C | in the pool (see below) | Arch's initramfs |

"In the pool" means twice: the file itself in `/etc`, and a copy of it in Arch's initramfs in `/boot`. Both
directories are inside the encrypted pool.

The key file in Arch's initramfs is safe **only because** the file and the initramfs both live inside the encrypted
pool (`/etc` and `/boot`); they cannot be read before the passphrase has been typed. ZFSBootMenu's own image lives on
the unencrypted ESP and never contains a key.

### What the project has tested

From [Testing](../../luks/testing.rst):

- The VM tests boot ZFSBootMenu images built **with dracut** by the project's build container (Ubuntu 26.10,
  Linux 7.3, OpenZFS 2.4.4, cryptsetup 2.8.7, systemd 261, dracut 112), in QEMU.
- The system they boot is a test payload, not an installed distribution: an Ubuntu kernel with a **dracut**
  initramfs and a minimal root file system. "Boot environments whose initramfs is not built by dracut (for example
  mkinitcpio or initramfs-tools)" are listed as [not covered](../../luks/testing.rst#not-covered).
- For this guide that means: building ZFSBootMenu's image with dracut is the tested path, but here it is built on
  Arch, with Arch's `linux-lts` kernel and Arch's dracut, which the tests do not use (the OpenZFS version from
  archzfs, 2.4.4 at the time of writing, is the same as the tests'). Arch's **mkinitcpio** initramfs, which does the
  second unlock in Options B and C, is **not covered**. Images built **with** mkinitcpio are not covered either;
  this guide does not build one.
- The suite runs the **Rust runtime** by default. The **shell runtime**, which this guide uses unless you choose
  otherwise in [Choosing the runtime](#choosing-the-runtime), runs only when its tests are named; its last full run
  was on 20 October 2026.
- **Option A**: unencrypted pools are booted with the Rust runtime in the suite; the shell runtime's tests run only
  when named.
- **Option B**: ZFSBootMenu's passphrase prompt is tested. The single-prompt arrangement, with the key file in the
  system's initramfs, is **not**: the test payload's initramfs holds no ZFS key file. It is the design described in
  [Native Encryption](../../general/native-encryption.rst).
- **Option C**: the design of this guide (NVMe, 4 KiB sectors, the TRIM flag in the LUKS2 header, `zbm.luks=UUID=`,
  a key file in the system's initramfs) is tested, but with a dracut initramfs and `luks.*` parameters, not with
  mkinitcpio's `encrypt` hook and `cryptdevice=`.
- Real hardware and Secure Boot are not covered.

## Requirements

### Hardware

- An x86_64 computer with an NVMe SSD that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware: the ZFSBootMenu image built here is not signed, and the Arch ISO does not
  boot with Secure Boot enabled either. How to check is shown in [Preparing the NVMe SSD](#preparing-the-nvme-ssd).
- An internet connection, wired if possible.
- At least 8 GiB of RAM. The live system keeps everything it installs in a RAM disk, which this guide enlarges to
  4 GiB, and it builds the ZFS module.
- Free space on the new pool for the build: about 1.1 GB for the clone of this repository (most of it the Rust
  crates in `vendor/`), and 1 to 2 GB more for Cargo's build directory. The second figure is an estimate, not a
  measurement; `du -sh /usr/src/zbm` after the build shows what it used.

### Installation media

The official Arch ISO contains no ZFS: OpenZFS is not in Arch's repositories. ZFS for Arch comes from the
third-party **archzfs** repository, which the OpenZFS documentation points to. This guide boots the official ISO and
adds ZFS to the running live system.

The facts this guide depends on, and how to check each on your system:

| Fact | Value (November 2026) | Check |
|---|---|---|
| Live medium | the official Arch ISO (1) | `cat /version` |
| Tools on the ISO | `gdisk`, `cryptsetup`, `nvme`, `mkfs.vfat`, `arch-chroot` and more (2) | `pacman -Q gptfdisk` |
| archzfs repository | the `experimental` release of archzfs on GitHub (3) | `pacman -Si zfs-utils` |
| OpenZFS in archzfs | 2.4.4 (4) | `pacman -Si zfs-linux-lts` |
| Kernels OpenZFS 2.4.4 supports | Linux 4.18 to 7.2 (its `META` file) | step 4 below |
| System initramfs | mkinitcpio, with the busybox hooks `udev`, `encrypt` and archzfs's `zfs` | `mkinitcpio --version` |
| `rustc` | Arch's `rust` package; 1.88 or later is needed (5) | `pacman -Si rust` |
| EFI stub | `/usr/lib/systemd/boot/efi/linuxx64.efi.stub` (6) | `pacman -Qo` on the file |

1. Released monthly; it boots with systemd-boot in UEFI mode and gives root the `zsh` shell.
2. Also `pacstrap` and `partprobe`; from the packages gptfdisk, cryptsetup, nvme-cli, dosfstools,
   arch-install-scripts and parted.
3. `https://github.com/archzfs/archzfs/releases/download/experimental`, signed with key
   `3A9917BF0DED5C13F69AC68FABEC0A1208037BE9`; the old `archzfs.com` repository is legacy.
4. `zfs-utils`, `zfs-dkms`, `zfs-linux` (for `linux` 7.2.15) and `zfs-linux-lts` (for `linux-lts` 6.18.63).
5. `rust-version` in `Cargo.toml`. Arch's current version was not checked for this guide.
6. Believed to be in Arch's `systemd` package (not confirmed).

The versions in the table change with every update; the Check commands show the current ones.

Download the current ISO from <https://archlinux.org/download/>, verify it as described there, write it to a USB
stick, and boot it in **UEFI** mode.

**1. Enlarge the live system's RAM disk.** Everything the live system installs is kept in a RAM disk of 256 MiB by
default (the archiso boot parameter `cow_spacesize`), too small for the ZFS build. In the ISO's boot menu, select
*Arch Linux install medium (x86_64, UEFI)*, press `e`, add ` cow_spacesize=4G` to the end of the line, and press
Enter. You get a root shell (`zsh`) without logging in. Connect to the network (a wired connection works at once;
for Wi-Fi use `iwctl`).

**2. Pin the package database to the ISO's date.** The ZFS module is built with DKMS for the ISO's running kernel,
which needs the kernel headers of exactly that version and a compiler of the same time. The Arch Linux Archive keeps
the repositories as they were on each day; point the live system at the day the ISO was built, and keep the current
mirror list for later.

First make sure that the current mirror list names servers and that nothing will rewrite it:

```
root # systemctl is-active reflector.service
root # grep -c '^Server' /etc/pacman.d/mirrorlist
```

Since archiso release 87 (October 2025) the ISO no longer starts `reflector.service` at boot, and its mirror list
has the HTTPS mirrors enabled; the first command then prints `inactive`. If it prints `activating` (an older ISO, or
a service someone started), wait until it prints `inactive` or `failed`: reflector rewrites the mirror list when it
finishes. The second command must print a number above `0`. If it prints `0`, write a mirror list with
`reflector --protocol https --latest 20 --sort rate --save /etc/pacman.d/mirrorlist` and run both commands again.
Then:

```
root # cat /version
root # uname -r
root # cp /etc/pacman.d/mirrorlist /etc/pacman.d/mirrorlist.current
root # ISO_DATE="$(tr . / < /version)"
root # echo "Server = https://archive.archlinux.org/repos/${ISO_DATE}/\$repo/os/\$arch" > /etc/pacman.d/mirrorlist
```

`/version` holds the ISO's date, such as `2026.11.01`; the last two commands write a mirror list with one line,
`Server = https://archive.archlinux.org/repos/2026/11/01/$repo/os/$arch`.

**3. Add the archzfs repository.** The section and the key are those the archzfs project publishes in the notes of
its [`experimental` release](https://github.com/archzfs/archzfs/releases/tag/experimental):

```
root # cat >> /etc/pacman.conf <<'EOF'

[archzfs]
SigLevel = Required
Server = https://github.com/archzfs/archzfs/releases/download/experimental
EOF
root # pacman-key --recv-keys 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
root # pacman-key --finger 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
root # pacman-key --lsign-key 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
```

Retrieving a key from a key server does not prove whose it is. Before `--lsign-key`, compare the fingerprint that
`--finger` prints with the one in the
[archzfs-keyring repository](https://github.com/archzfs/archzfs-keyring/tree/master/keyring/packager/archzfs) and on
the release page. The release notes also describe `SigLevel = Never`; do not use it for the repository that supplies
a kernel module, which runs as root. An `archzfs-keyring` package is announced but not finished at the time of
writing.

**4. Check the kernel, then install the kernel headers, the compiler and ZFS.** First read the database, and compare
the ISO's kernel with the range of kernels that archzfs's OpenZFS release supports (from that release's `META` file):

```
root # pacman -Sy
root # uname -r
root # ZFS_VER="$(pacman -Si zfs-dkms | sed -n 's/^Version *: \([0-9.]*\).*/\1/p')"
root # curl -fsS "https://raw.githubusercontent.com/openzfs/zfs/zfs-${ZFS_VER}/META" | grep '^Linux-'
```

The last command prints `Linux-Maximum` and `Linux-Minimum` (`7.2` and `4.18` for OpenZFS 2.4.4). Continue only if
the major and minor version that `uname -r` prints (`7.2` in `7.2.3-arch1-1`) is not above `Linux-Maximum`;
otherwise use an older ISO now (see the Note below).

The headers come from the archive by their exact version: the package version is `uname -r` with the first `-arch`
written `.arch` (kernel `7.2.3-arch1-1`, package `7.2.3.arch1-1`). Installing `zfs-dkms` builds the module, which
takes several minutes:

```
root # HDR_VER="$(uname -r | sed 's/-arch/.arch/')"
root # pacman -U "https://archive.archlinux.org/packages/l/linux-headers/linux-headers-${HDR_VER}-x86_64.pkg.tar.zst"
root # pacman -S --needed base-devel zfs-dkms
root # modprobe zfs
```

`zfs-dkms` pulls in `zfs-utils` (the tools, among them `zpool`, `zfs` and `zgenhostid`) and `dkms`.

**5. Restore the current mirror list**, which `pacstrap` copies into the new system later:

```
root # cp /etc/pacman.d/mirrorlist.current /etc/pacman.d/mirrorlist
```

> **Note**
> These live-system steps follow the archzfs release notes, OpenZFS's `META` file and the layout of the Arch Linux
> Archive; the project has not run them. Two things can make them fail, with opposite remedies:
>
> - The ISO's kernel is newer than `Linux-Maximum` (step 4): use the previous month's ISO from
>   <https://archive.archlinux.org/iso/>, or an older one whose kernel is in range.
> - archzfs's current `zfs-utils` needs newer libraries than the ISO's date offers: use the **newest** ISO.
>
> If neither works, use one of the alternatives below, and see
> [The live system cannot build the ZFS module](#the-live-system-cannot-build-the-zfs-module).

Alternatives, not covered by this guide: an Arch ISO you build yourself with archiso's `releng` profile and the
archzfs packages (Arch Wiki: *Archiso*), or a third-party ISO with ZFS included, such as the monthly builds of the
`r-maerz/archlinux-lts-zfs` project on GitHub (`linux-lts` with `zfs-linux-lts`). This guide could not confirm the
state of that project. On such an ISO, skip steps 1 to 5; everything else in this guide is the same.

**Check:**

```
root # zfs version
root # uname -r
root # grep '^Linux-' /usr/src/zfs-*/META
root # dkms status
root # grep -c '^Server' /etc/pacman.d/mirrorlist
root # grep -c archive.archlinux.org /etc/pacman.d/mirrorlist
```

- `zfs version` prints `zfs-2.4...` (the tools) and `zfs-kmod-2.4...` (the module), or later versions. The pool in
  this guide needs 2.2 or later.
- The kernel version that `uname -r` prints is between `Linux-Minimum` and `Linux-Maximum`, and `dkms status` shows
  the `zfs` module `installed` for it.
- The mirror list is the current one again: the first `grep` prints a number above `0`, the second prints `0`.

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new Arch Linux system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |
| `<Enter>` | in a transcript: press Enter without typing anything |
| **Option A.** / **Option B.** / **Option C.** | the next paragraph and commands are for that option only |
| **All options.** | the text is for every option again |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

The live system's root shell is `zsh`. Every command in this guide works there; the few that need `bash` call it
explicitly. Inside the chroot the shell is `bash`.

## Preparing the NVMe SSD

Everything in this section runs on the live system. First check that it booted in UEFI mode, that ZFS works, and
that Secure Boot is off.

**Check:**

```
root # ls /sys/firmware/efi
root # zfs version
root # pacman -Q gptfdisk
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
root # cryptsetup --version
```

- `ls` lists files; otherwise the live system booted in legacy BIOS mode.
- `zfs version` prints version 2.2 or later for the tools and the module.
- `pacman -Q gptfdisk` (the package that has `gdisk`) prints version 1.0.9 or later.
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
root # export BE=arch
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the EFI system partition (1) |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `arch` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}` |
| `POOL_DEV` | see (2) | A B C | the device `zpool create` gets |
| `LUKS_UUID` | see (3) | C | the UUID of the LUKS container |

1. NVMe partitions have a `p` before the number.
2. Options A and B: `${POOL_PART}`, set in [Creating the ZFS pool](#creating-the-zfs-pool). Option C:
   `/dev/mapper/luks-${LUKS_UUID}`, set in
   [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container).
3. `$(cryptsetup luksUUID "${POOL_PART}")`; it exists only after `luksFormat`.

The name in `BE` is free; ZFSBootMenu does not read it. It recognizes Arch from `/etc/os-release` inside the boot
environment (`ID=arch`).

Some paths are written literally, because files cannot expand variables. If you change `POOL`, change `zroot` in
these paths too: `/etc/zfs/zroot.key` (Option B), `/etc/cryptsetup-keys.d/zroot.key` (Option C), and every later
command that names `zroot` after the installation.

> **Tip**
> If the terminal is closed, set these variables again before continuing. For Option C, set `LUKS_UUID` and
> `POOL_DEV` again as well; see
> [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container).
>
> If the live system is **rebooted**, it forgets everything it did, because it runs from RAM. Repeat the steps of
> [Installation media](#installation-media) and set the variables again. If the SSD has not been erased yet, repeat
> [Releasing the SSD](#releasing-the-ssd) too. If the pool already exists, also:
>
> 1. run `zgenhostid -f 0x00bab10c` again ([Creating the ZFS pool](#creating-the-zfs-pool), step 1);
> 2. **Option B**: write `/etc/zfs/zroot.key` again with the **same** passphrase (the `bash -c` command of step 2 of
>    that section, then `chmod 000 /etc/zfs/zroot.key`);
> 3. **Option C**: open the container again, as shown at the end of
>    [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container);
> 4. repeat [Mounting the pool below /mnt](#mounting-the-pool-below-mnt), which also writes
>    `/etc/zfs/zpool.cache` again (if the import is refused because the pool was not exported, add `-f` to
>    `zpool import`). If `/mnt/boot/efi` was mounted before, mount it again:
>    `mount "${ESP_PART}" /mnt/boot/efi`.

**Check:**

```
root # lsblk -o NAME,SIZE,MODEL,SERIAL "${DISK}"
```

The model and serial number are those of the SSD to erase.

### Releasing the SSD

Why: the live system may have taken parts of an old installation on the SSD into use: a swap partition, a ZFS pool,
LVM volumes (the ISO activates them on its own), an opened LUKS container or a RAID array. While anything uses the
SSD, `nvme format`, `blkdiscard`, `wipefs` and gdisk fail with "Device or resource busy", or leave the kernel with
the old partition table.

```
root # lsblk -o NAME,TYPE,FSTYPE,MOUNTPOINTS "${DISK}"
root # swapoff -a
root # zpool export -a
root # vgchange -an
```

- Unmount anything that `lsblk` shows with a mount point (`umount PATH`).
- `swapoff -a` stops every swap space, `zpool export -a` releases any pool the live system imported ("no pools
  available to export" is not an error), and `vgchange -an` deactivates every LVM volume group the live system
  activated, also on other disks; the live system needs none of them. "No volume groups found" is not an error.
- If `lsblk` shows rows of the type `crypt` or `raid*` below the SSD, close them by their names in the `NAME`
  column: `cryptsetup close NAME` for `crypt`, `mdadm --stop /dev/NAME` for `raid*`.

**Check:**

```
root # lsblk -o NAME,TYPE,MOUNTPOINTS "${DISK}"
```

Only rows of the types `disk` and `part` are listed, and none has a mount point.

### Optional: switching to 4 KiB sectors

Why: many NVMe SSDs ship with 512-byte logical sectors and also support 4096-byte sectors, which can be slightly
faster. Do this only after [Releasing the SSD](#releasing-the-ssd). Show the formats the SSD supports:

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

The SSD must be released first ([Releasing the SSD](#releasing-the-ssd)); after a reboot of the live system, repeat
that step.

```
root # for part in "${DISK}"p*; do [ -b "${part}" ] && zpool labelclear -f "${part}"; done
root # for part in "${DISK}"p*; do [ -b "${part}" ] && wipefs -a "${part}"; done
root # blkdiscard -f "${DISK}"
root # wipefs -a "${DISK}"
```

- The two loops clear signatures **inside** old partitions: ZFS keeps four labels per device and LUKS2 two headers,
  and wiping only the partition table would leave them where a new partition may find them. `zpool labelclear`
  prints an error for a partition that holds no ZFS label; that is expected. On an SSD without partitions, `zsh`
  answers each loop with `no matches found`; that is not an error either.
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
| 1 | `${ESP_PART}` (`/dev/nvme0n1p1`) | 1 GiB | `EF00` EFI system partition | `EFI` | the ESP, FAT32, at `/boot/efi` |
| 2, Options A and B | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `BF00` Solaris root | `zroot` | the ZFS pool |
| 2, Option C | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `8309` Linux LUKS | `zroot-luks` | the LUKS2 container |

The ESP holds only ZFSBootMenu, but several copies of it: the image, its backup, a copy at `\EFI\BOOT\BOOTX64.EFI`,
and any copy you keep yourself. 1 GiB leaves room for all of them. No Arch kernel or initramfs is ever stored on the
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
- If gdisk ends with "Warning: The kernel is still using the old partition table", run `partprobe "${DISK}"`, or
  reboot the live system and repeat the live-system steps and the variables.
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
  older than 1.0.9. For Options A and B that does no harm. For Option C, see the steps below.
- `wipefs` prints nothing. If it prints a line, run `wipefs -a "${POOL_PART}"`.

**Option C, only if the `echo` did not print `0`.** Compute an aligned last sector for partition 2 from the last
usable sector that gdisk reports (512-byte sectors):

```
root # LAST="$(gdisk -l "${DISK}" | sed -n 's/.*last usable sector is \([0-9]*\).*/\1/p')"
root # echo $(( (LAST + 1) / 2048 * 2048 - 1 ))
```

Then run `gdisk "${DISK}"` and type: `d`, `2` (delete partition 2); `n`, `<Enter>`, `<Enter>`, the number the `echo`
printed at the `Last sector` prompt, `8309`; `c`, `2`, `zroot-luks`; `p` to compare; `w`, `y`. Run
`udevadm settle` and repeat this Check.

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
- The container is opened as `luks-<UUID>`, the name ZFSBootMenu gives volumes it is told to open by UUID. Arch's
  initramfs opens it under the same name later. Using one name everywhere keeps the device path recorded in the
  pool the same in ZFSBootMenu and in Arch.
- `--allow-discards --persistent` stores the TRIM flag **in the LUKS2 header**, so the container passes TRIM requests
  through wherever it is opened (needed for `autotrim=on`): in ZFSBootMenu, in Arch's initramfs and on a live
  system, without any further option.

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
normal import by another host while the pool was not exported. ZFSBootMenu, Arch's initramfs and the running system
all import this pool, so they must agree. Write a fixed host ID now; it is copied into the new system later.

```
root # zgenhostid -f 0x00bab10c
```

ZFSBootMenu passes this host ID to Arch's kernel as `spl.spl_hostid=0x00bab10c`. That matters on Arch: the archzfs
`zfs` hook does not copy `/etc/hostid` into the initramfs, so without it Arch's initramfs would import with another
host ID.

**2. Option B: the passphrase file.** `zpool create` reads the passphrase from the file named by `keylocation`, and the
same file later lets Arch's initramfs load the key without asking. The command reads the passphrase twice without
showing it, and writes the file only if both are the same and 8 to 512 characters long. `IFS=` keeps spaces at the start
and end of the passphrase. It runs in `bash`, because `zsh`'s `read` takes other options; the three lines are one
command, so copy them together.

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

If the command prints the error, run it again. The file must stay on the live system until the installation is
finished.

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
| `compatibility=openzfs-2.2-linux` | only the features of OpenZFS 2.2, so that ZFS 2.2 or later can import it |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | everything the pool mounts during the installation appears below `/mnt` |
| `encryption=aes-256-gcm` (B) | likely the fastest cipher on modern x86_64 hardware |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file of step 2; also in Arch's initramfs, inside the pool |

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
| `zroot/ROOT/arch` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, including `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/arch` | the boot environment ZFSBootMenu boots after the countdown |

- `canmount=noauto` is required on every file system with `mountpoint=/`, and it is not inherited: each boot
  environment you create later needs it set again.
- Keep `/usr`, `/etc` and `/var` in the boot environment: they change together with every package update (pacman's
  database is in `/var/lib/pacman`). Add datasets only for user data (`/srv`, for example), if you want them.

> **Important**
> `/boot` is a directory of the root dataset, not a file system of its own and not the ESP. ZFSBootMenu reads
> Arch's kernel and initramfs from there, and in Options B and C the initramfs with the key file is then inside the
> encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu. Many Arch installations mount the
> ESP at `/boot`; this one must not.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/arch`.

### Setting the ZFSBootMenu properties

Why: ZFSBootMenu reads the kernel command line for Arch from a ZFS property. It is set on `zroot/ROOT`, so every
future boot environment inherits it.

The `zfs` hook of Arch's initramfs imports the pool read-only unless the command line contains `rw`, so every option
has it.

**Options A and B:**

```
root # zfs set org.zfsbootmenu:commandline="rw quiet" "${POOL}/ROOT"
```

**Option C** adds the parameters of the `encrypt` hook. The line is built in two steps to keep it short:

```
root # KCL="rw quiet cryptdevice=UUID=${LUKS_UUID}:luks-${LUKS_UUID}"
root # KCL="${KCL} cryptkey=rootfs:/etc/cryptsetup-keys.d/zroot.key"
root # zfs set org.zfsbootmenu:commandline="${KCL}" "${POOL}/ROOT"
```

| Parameter | Read by | Meaning |
|---|---|---|
| `rw` | `zfs` | import the pool read-write |
| `cryptdevice=UUID=<UUID>:luks-<UUID>` | `encrypt` | the container, by its LUKS UUID, and the mapping name to create |
| `cryptkey=rootfs:/etc/cryptsetup-keys.d/zroot.key` | `encrypt` | the key file, at this path **inside the initramfs** |

The `encrypt` hook asks for the passphrase only if the key file does not work. No `:allow-discards` is needed
after the mapping name: the TRIM flag is stored in the LUKS header.

**All options.** Do not put `root=` or `zfs=` into this property: ZFSBootMenu always adds the root argument itself.
For a boot environment whose `/etc/os-release` says `ID=arch` it adds `zfs=zroot/ROOT/arch`, which the archzfs
`zfs` hook reads; there is no need to set `org.zfsbootmenu:rootprefix`. Do not add `spl.spl_hostid=` either;
ZFSBootMenu sets it.

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

The first prints `rw quiet` (Options A and B), or for Option C `rw quiet cryptdevice=UUID=...` with the LUKS UUID
twice and the `cryptkey=` part. The second prints `zroot/ROOT/arch` for Option B and `-` for Options A and C.

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
`zpool destroy "${POOL}"`, then repeat everything from step 2 of [Creating the ZFS pool](#creating-the-zfs-pool)
(the passphrase file and the pool) through [Creating the datasets](#creating-the-datasets) and
[Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties), and this section.

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

`-N` imports without mounting; the root dataset is mounted by hand because it is `canmount=noauto`. The cache file
is copied into the new system later: the archzfs `zfs` hook copies it into the initramfs, and the
`zfs-import-cache` service of the running system reads it.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot. The
> ZFS tools inside the new system may not match the ZFS kernel module that the live system has loaded.

**Check:**

```
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`/mnt` is `zroot/ROOT/arch` and `/mnt/home` is `zroot/home`. Option B: the key status is `available` (Options A
and C print `-`).

## Installing the base system

Why: `pacstrap` installs a minimal Arch Linux into the mounted boot environment.

```
root # pacstrap -K /mnt base linux-firmware nano
```

- `-K` creates a new, empty pacman keyring in the new system, which the `archlinux-keyring` package fills. The
  archzfs key is added to it in [Kernel and ZFS](#kernel-and-zfs).
- `pacstrap` copies the live system's mirror list into the new system; that is why
  [Installation media](#installation-media) restored the current one.
- The kernel is not installed yet: it comes in [Kernel and ZFS](#kernel-and-zfs), together with its ZFS module.
- `pacstrap` uses the live system's `/etc/pacman.conf`, which now names `[archzfs]`; nothing from it is installed
  here, and the new system gets Arch's default `/etc/pacman.conf`.

**Check:**

```
root # grep '^ID=' /mnt/etc/os-release
root # ls /mnt
root # grep -c '^Server' /mnt/etc/pacman.d/mirrorlist
root # grep -c archive.archlinux.org /mnt/etc/pacman.d/mirrorlist
```

`ID=arch`, `/mnt` holds a root file system (`bin`, `boot`, `etc`, `usr`, `var` and so on), and the mirror list is
the current one, not the archive line: the first `grep` prints a number above `0`, the second prints `0`. If not,
copy the live system's list again (`cp /etc/pacman.d/mirrorlist /mnt/etc/pacman.d/mirrorlist`) after checking it
with the same two commands.

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

Why: the rest of the installation runs inside the new system. `arch-chroot` mounts `/proc`, `/sys` with the EFI
variables, `/dev`, `/run` and `/tmp` for it, and makes the live system's `/etc/resolv.conf` visible inside, so that
the network works.

```
root # arch-chroot /mnt /bin/bash
(chroot) root # source /root/install.env
```

If you leave the chroot and come back, run both lines again. `arch-chroot` unmounts what it mounted when you leave.

**Check:**

```
(chroot) root # echo "${POOL} ${BE} ${DISK}"
(chroot) root # ls /sys/firmware/efi/efivars | head -n 3
```

The first prints `zroot arch /dev/nvme0n1`; the second lists EFI variables.

## Kernel and ZFS

Why: Arch's kernel, and the ZFS module built for it, make up the system ZFSBootMenu will boot.

**Which kernel and which ZFS package.** archzfs offers two kinds of module packages, and either kernel can be used:

- **`linux-lts` with `zfs-linux-lts`** (this guide): the module is built by archzfs for exactly one `linux-lts`
  version and depends on it. pacman never installs a kernel without its ZFS module: when Arch publishes a new
  `linux-lts` before archzfs has built the matching module, `pacman -Syu` stops with a dependency error and changes
  nothing, until archzfs catches up.
- `linux-lts` with `linux-lts-headers` and `zfs-dkms`: DKMS builds the module on this computer after every kernel
  update; updates are never held back. If the build fails, the new kernel has no ZFS module, and neither Arch nor a
  new ZFSBootMenu image can import the pool with it.
- `linux` with `zfs-linux`: as the first choice, with Arch's current kernel; new major versions arrive more often,
  so updates are held back more often.
- `linux` with `linux-headers` and `zfs-dkms`: the riskiest. A new `linux` can be newer than the newest kernel the
  OpenZFS release supports, and then the module does not build.

This guide uses `linux-lts` with `zfs-linux-lts`: a failed update leaves the system as it was instead of leaving it
without ZFS, and the LTS kernel stays inside OpenZFS's supported range for longer. To use another row, replace the
package names in the next commands and in later commands that name `linux-lts` (`/boot/vmlinuz-linux-lts`,
`/boot/initramfs-linux-lts.img`).

Add the archzfs repository and its key to the new system, exactly as on the live system:

```
(chroot) root # cat >> /etc/pacman.conf <<'EOF'

[archzfs]
SigLevel = Required
Server = https://github.com/archzfs/archzfs/releases/download/experimental
EOF
(chroot) root # pacman-key --recv-keys 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
(chroot) root # pacman-key --finger 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
(chroot) root # pacman-key --lsign-key 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
```

Install the kernel, mkinitcpio and ZFS, and enable the ZFS services:

```
(chroot) root # pacman -Syu
(chroot) root # pacman -S --needed mkinitcpio linux-lts zfs-linux-lts zfs-utils
(chroot) root # systemctl enable zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target
```

If the `pacman -S` line fails with an error such as `unable to satisfy dependency 'linux-lts=…' required by
zfs-linux-lts`, archzfs has not yet built the module for Arch's newest `linux-lts`. Install mkinitcpio, then the
kernel version that `zfs-linux-lts` was built for from the Arch Linux Archive, then the rest, and run the
`systemctl enable` line:

```
(chroot) root # pacman -S --needed mkinitcpio
(chroot) root # KVER="$(pacman -Si zfs-linux-lts | sed -n 's/.*linux-lts=\([^ ]*\).*/\1/p')"
(chroot) root # echo "${KVER}"
(chroot) root # pacman -U "https://archive.archlinux.org/packages/l/linux-lts/linux-lts-${KVER}-x86_64.pkg.tar.zst"
(chroot) root # pacman -S --needed zfs-linux-lts zfs-utils
```

`echo` prints a version such as `6.18.63-3`. mkinitcpio is installed first because the kernel package needs an
initramfs generator, and pacman would otherwise ask which one to install. Until archzfs catches up, `pacman -Syu`
stops with a dependency error and changes nothing; see
[pacman -Syu stops on zfs-linux-lts](#pacman--syu-stops-on-zfs-linux-lts). This fallback has not been run.

- The kernel package installs `/usr/lib/modules/<version>-lts/vmlinuz`; mkinitcpio's pacman hook copies it to
  `/boot/vmlinuz-linux-lts` and builds `/boot/initramfs-linux-lts.img`, both in `/boot` of the boot environment,
  where ZFSBootMenu looks for them. That is what the preset `/etc/mkinitcpio.d/linux-lts.preset` says; it names no
  unified kernel image (UKI), which would be written to the ESP.
- This first initramfs cannot import the pool yet; [Configuring the initramfs](#configuring-the-initramfs) builds it
  again. mkinitcpio may print warnings such as `Possibly missing firmware for module`; they are harmless.
- `zfs-import-cache` imports the pool from `/etc/zfs/zpool.cache` at boot if the initramfs has not, and `zfs-mount`
  mounts `zroot/home`.
- Nothing in this guide installs GRUB or runs `bootctl install` or `kernel-install`; with them, kernels could end up
  on the ESP.

**Check:**

```
(chroot) root # pacman -Q linux-lts zfs-linux-lts zfs-utils
(chroot) root # ls /boot
(chroot) root # ls -A /boot/efi
(chroot) root # grep -E '^[^#]*_(kver|image|uki)=' /etc/mkinitcpio.d/linux-lts.preset
(chroot) root # find /usr/lib/modules -name 'zfs.ko*'
(chroot) root # systemctl is-enabled zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target
(chroot) root # od -A n -t x4 /etc/hostid
```

- `pacman -Q` lists the three packages, and the version of `zfs-linux-lts` contains the version of `linux-lts`
  (for example `2.4.4_6.18.63.3-1` with `linux-lts 6.18.63-3`).
- `/boot` holds `vmlinuz-linux-lts`, `initramfs-linux-lts.img` and the directory `efi`; `/boot/efi` is empty.
- The preset prints `ALL_kver="/boot/vmlinuz-linux-lts"` and `default_image="/boot/initramfs-linux-lts.img"`, and no
  `_uki=` line.
- `find` prints one `zfs.ko.zst`, below `/usr/lib/modules/<version>-lts/`.
- `systemctl is-enabled` prints `enabled` four times, and the host ID is `00bab10c`.

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Why: Arch's initramfs must unlock the container again after `kexec`; a random key in a second key slot lets it do
so without asking.

```
(chroot) root # pacman -S --needed cryptsetup
(chroot) root # install -d -m 0700 /etc/cryptsetup-keys.d
(chroot) root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot.key
(chroot) root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  "${POOL_PART}" /etc/cryptsetup-keys.d/zroot.key
```

`luksAddKey` asks for the passphrase and adds the file's content as key slot 1. A random key needs no slow key
derivation, so `--pbkdf pbkdf2 --pbkdf-force-iterations 1000` (the smallest count cryptsetup accepts) makes the second
unlock almost instant. The passphrase slot keeps the slow, memory-hard default.

**Check:**

```
(chroot) root # ls -l /etc/cryptsetup-keys.d/zroot.key
(chroot) root # cryptsetup open --test-passphrase --verbose --key-file /etc/cryptsetup-keys.d/zroot.key "${POOL_PART}"
(chroot) root # cryptsetup luksDump "${POOL_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, cryptsetup reports `Key slot 1 unlocked.`, and two key slots (`0` and
`1`) are listed.

## Configuring the initramfs

Why: after `kexec`, Arch's initramfs (mkinitcpio) must import the pool and, in Options B and C, unlock it with the
key file. In Options B and C the image then contains a key, so it must be readable by root only.

What Arch's initramfs must contain:

| | Option A | Option B | Option C |
|---|---|---|---|
| ZFS module and tools, `zpool.cache` (hook `zfs`) | yes | yes | yes |
| `/etc/zfs/zroot.key` | | yes | |
| `cryptsetup` and dm-crypt (hook `encrypt`) | | | yes |
| `/etc/cryptsetup-keys.d/zroot.key` | | | yes |
| the NVMe driver (hooks `autodetect` and `block`) | yes | yes | yes |
| readable by root only | not needed | **required** | **required** |

mkinitcpio reads `/etc/mkinitcpio.conf` and then every file in `/etc/mkinitcpio.conf.d/`, in name order; a later
`HOOKS=(...)` replaces an earlier one, and `FILES+=(...)` adds to `FILES`. This guide leaves `/etc/mkinitcpio.conf`
as the package installed it and writes its settings into files of its own there.

**The hooks.** The archzfs `zfs` hook is a busybox hook: it does not run in an initramfs built with mkinitcpio's
`systemd` hook, and archzfs ships no systemd version of it. Current mkinitcpio packages default to the `systemd`
hooks, so set the busybox hooks explicitly. `zfs` comes after `block`; in Option C `encrypt` comes before `zfs`.

**Options A and B:**

**FILE** `/etc/mkinitcpio.conf.d/10-zfs.conf`

```
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block zfs filesystems)
```

**Option C:**

**FILE** `/etc/mkinitcpio.conf.d/10-zfs.conf`

```
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block encrypt zfs filesystems)
```

**All options.**

- `microcode` puts the CPU microcode at the front of the initramfs. ZFSBootMenu loads exactly one initramfs file per
  kernel, `initramfs-linux-lts.img`, so microcode in a separate `intel-ucode.img` or `amd-ucode.img` would never be
  loaded. Install the package for your CPU (only one of the two lines):

  ```
  (chroot) root # pacman -S --needed intel-ucode
  (chroot) root # pacman -S --needed amd-ucode
  ```

- `keymap` and `consolefont` read `/etc/vconsole.conf`. Write it now with your console keyboard layout (`us`,
  `de-latin1`, `fr-latin1` and so on; `localectl list-keymaps` lists them):

  ```
  (chroot) root # echo 'KEYMAP=us' > /etc/vconsole.conf
  ```

  The layout is used only for prompts of Arch's initramfs, which after this guide appear only when something went
  wrong. `consolefont` warns that no font is set; that is harmless.
- `autodetect` keeps only the drivers this computer needs. The chroot runs on the same computer, so the NVMe driver is
  found.
- `fsck` is not needed for ZFS.

**Option A.** Nothing else.

**Option B.** Add the key file to the initramfs:

**FILE** `/etc/mkinitcpio.conf.d/90-zbm-key.conf`

```
FILES+=(/etc/zfs/zroot.key)
```

The `zfs` hook loads the key of an encryption root whose `keylocation` is a file with `zfs load-key`, from the file
**inside the initramfs**. If that fails, it asks for the passphrase instead (`Unable to load key ...; please type the
passphrase`).

**Option C.** Add the key file to the initramfs:

**FILE** `/etc/mkinitcpio.conf.d/90-zbm-key.conf`

```
FILES+=(/etc/cryptsetup-keys.d/zroot.key)
```

The `encrypt` hook finds it through `cryptkey=rootfs:/etc/cryptsetup-keys.d/zroot.key` on the command line, set in
[Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties). If the key does not open the container,
it prints `Invalid keyfile. Reverting to passphrase.` and asks.

**All options.** mkinitcpio sets its umask to `077` before it writes an image, so a new image is readable by root
only. Confirm that the installed version does, then rebuild every preset:

```
(chroot) root # grep -n 'umask 077' /usr/bin/mkinitcpio
(chroot) root # mkinitcpio -P
```

The build lists `Using drop-in configuration file: '10-zfs.conf'` (and `'90-zbm-key.conf'` in Options B and C) and
the hooks it runs, and ends with `Initcpio image generation successful`.

**Check:**

```
(chroot) root # ls -l /boot
(chroot) root # lsinitcpio /boot/initramfs-linux-lts.img | grep -E 'zfs\.ko|zpool\.cache|hooks/(zfs|encrypt)$'
(chroot) root # lsinitcpio /boot/initramfs-linux-lts.img | grep -E 'zroot\.key|bin/cryptsetup$'
(chroot) root # ls -A /boot/efi
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `ls -l` of `initramfs-linux-lts.img` | `-rw-------` | `-rw-------` | `-rw-------` |
| first `grep` | `zfs.ko`, `etc/zfs/zpool.cache.org`, `hooks/zfs` | the same | the same and `hooks/encrypt` |
| second `grep` | nothing | `etc/zfs/zroot.key` | `etc/cryptsetup-keys.d/zroot.key`, `.../bin/cryptsetup` |
| `/boot/efi` | empty | empty | empty |

`zfs.ko` may be compressed (`zfs.ko.zst`). If an image for Option B or C is readable by others, run `chmod 0600` on it
and find out why: mkinitcpio rewrites an existing image in place, keeping its old mode, when the file system has too
little free space for a temporary copy. Check the mode again after the next kernel update. Never copy such an image out
of the pool.

## Configuring the system

Why: the base configuration of any Arch system: time zone, locale, host name, network, the ESP in `/etc/fstab`, and
accounts.

Time zone (replace `REGION/CITY`, for example `Europe/Berlin`; `ls /usr/share/zoneinfo` lists them) and the hardware
clock:

```
(chroot) root # ln -sf /usr/share/zoneinfo/REGION/CITY /etc/localtime
(chroot) root # hwclock --systohc
```

Locale. Always enable `en_US.UTF-8`, and add your own in the same way:

```
(chroot) root # sed -i 's/^#en_US.UTF-8 UTF-8/en_US.UTF-8 UTF-8/' /etc/locale.gen
(chroot) root # locale-gen
(chroot) root # echo 'LANG=en_US.UTF-8' > /etc/locale.conf
```

The console keyboard layout is already in `/etc/vconsole.conf`
([Configuring the initramfs](#configuring-the-initramfs)).

Host name. Replace `HOSTNAME`:

```
(chroot) root # echo HOSTNAME > /etc/hostname
```

Network. NetworkManager handles wired and wireless connections (`nmtui` configures Wi-Fi after the first boot), and
`systemd-timesyncd` sets the clock:

```
(chroot) root # pacman -S --needed networkmanager
(chroot) root # systemctl enable NetworkManager.service systemd-timesyncd.service
```

The ESP in `/etc/fstab`. Only the ESP goes there; ZFS mounts its datasets itself. Do not run `genfstab`: it would add
lines for the ZFS datasets.

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

The root password, and a user for daily work (replace `USERNAME`) who may use `sudo` as a member of `wheel`:

```
(chroot) root # passwd
(chroot) root # pacman -S --needed sudo
(chroot) root # useradd -m -G wheel USERNAME
(chroot) root # passwd USERNAME
(chroot) root # echo '%wheel ALL=(ALL:ALL) ALL' > /etc/sudoers.d/10-wheel
(chroot) root # chmod 0440 /etc/sudoers.d/10-wheel
```

**Check:**

```
(chroot) root # cat /etc/hostname /etc/locale.conf /etc/vconsole.conf
(chroot) root # grep -v '^#' /etc/fstab
(chroot) root # findmnt --verify --tab-file /etc/fstab
(chroot) root # visudo -c
(chroot) root # id USERNAME
```

`/etc/fstab` has the one ESP line, `findmnt --verify` reports no errors for that line, `visudo -c` reports the files
parsed OK, and the user is in the group `wheel`.

## Installing ZFSBootMenu

Everything in this section runs **inside the chroot**, as root. ZFSBootMenu is built from this repository, because
upstream ZFSBootMenu cannot unlock LUKS volumes; for the same steps in all three options, Options A and B use this
build too. Do not install the AUR packages `zfsbootmenu` or `zfsbootmenu-efi-bin`: they are upstream ZFSBootMenu.

### Dependencies

Why: the build needs a Rust compiler and the tools that go into ZFSBootMenu's image.

```
(chroot) root # pacman -S --needed git base-devel rust dracut kexec-tools fzf efibootmgr \
  mbuffer less xkeyboard-config libx11
```

| Package | Why |
|---|---|
| `git` | fetch the source |
| `base-devel` | `make`, and binutils: `strip` for `make install`, `strings` for `generate-zbm`, `objcopy` for a Check |
| `rust` | `cargo` and `rustc`, 1.88 or later, for `zbm`, `brush`, `generate-zbm` and `zbm-kcl` |
| `dracut` | builds ZFSBootMenu's image; also provides `lsinitrd` |
| `kexec-tools` | starts Arch's kernel from ZFSBootMenu |
| `fzf` | the menu of the default (shell) runtime |
| `efibootmgr` | the firmware boot entries |
| `mbuffer`, `less` | optional tools of the image; the build warns without them |
| `xkeyboard-config`, `libx11` | keyboard layouts for the optional Rust runtime |

**Option C.** `cryptsetup` is already installed; it is what puts LUKS support into ZFSBootMenu's image. A dracut build
without it leaves LUKS out without a message.

**All options.**

- **Rust.** Arch's `rust` package installs `/usr/bin/cargo` and `/usr/bin/rustc`, which root has on its `PATH`.
  `make install` builds the programs again as the user who runs it, so this matters. The repository's
  `rust-toolchain.toml` (Rust 1.97.0) is read only by `rustup`, not by Arch's package. The Rust crates are in the
  clone (`vendor/`), so the build needs no network.
- **dracut beside mkinitcpio.** Installing `dracut` does not replace mkinitcpio for the system: Arch's kernel packages
  are handled by mkinitcpio's own pacman hook, and `generate-zbm` calls dracut only with ZFSBootMenu's configuration
  directory, `/etc/zfsbootmenu/dracut.conf.d` (never `/etc/dracut.conf.d`). The Check makes sure that the `dracut`
  package brings no pacman hook of its own.
- **The EFI stub.** `generate-zbm` builds the EFI image around systemd's stub,
  `/usr/lib/systemd/boot/efi/linuxx64.efi.stub`. Arch publishes the stub's manual page with its `systemd` package,
  and the stub is believed to be in that package as well; this guide could not confirm it from Arch's servers. The
  Check shows the owner on your system.

**Check:**

```
(chroot) root # ls -l /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # pacman -Qo /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # pacman -Ql dracut | grep 'libalpm/hooks'
(chroot) root # cargo --version && rustc --version
(chroot) root # command -v dracut lsinitrd kexec fzf efibootmgr strings strip objcopy make git
```

- The stub exists, and `pacman -Qo` names its package. If the file is missing, `pacman -Fy` and then
  `pacman -F /usr/lib/systemd/boot/efi/linuxx64.efi.stub` name the package that has it; install that package.
- The `pacman -Ql dracut` line prints nothing. A pacman hook in `libalpm/hooks` would rebuild the system's
  initramfs with dracut on every kernel update; if one is listed, stop here and find out what it does. The package
  may also contain `kernel-install` plugins (`pacman -Ql dracut | grep kernel/install.d` lists them, for example
  `50-dracut.install`); they are expected and do nothing here, because they run only when something calls
  `kernel-install`, which Arch's kernel packages and this guide do not.
- `rustc` is 1.88 or later, and every program is found. Option C: `command -v cryptsetup dmsetup udevadm` finds all
  three.

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

- `make` runs `cargo build --release --locked`; it takes several minutes.
- `make install` puts `generate-zbm` and `zbm-kcl` in `/usr/bin`; `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
  the shell runtime in `/usr/share/zfsbootmenu`; the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in
  `/usr/lib/dracut/modules.d`, and the mkinitcpio hooks `zfsbootmenu` and `zfsbootmenu-measured` in
  `/usr/lib/initcpio`; `config.yaml`, `mkinitcpio.conf` and `dracut.conf.d/` in `/etc/zfsbootmenu`; manual pages;
  and examples in `/usr/share/examples/zfsbootmenu`.
- The mkinitcpio hooks are not for the system's initramfs: never add `zfsbootmenu` to the `HOOKS` of
  `/etc/mkinitcpio.conf.d/10-zfs.conf`. (`zfsbootmenu-measured` belongs there only with measured boot and the Rust
  runtime, which this guide does not set up.)
- `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files it
  installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files with other names there are kept. This guide therefore writes
  `config.yaml` after `make install`, and keeps its own dracut settings in files of its own.

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
| `Global.InitCPIOConfig` | as shown | used only when the image is built with mkinitcpio, which this guide does not do |
| `Components.Enabled` | `false` | no separate kernel and initramfs; only the EFI file |
| `EFI.Enabled` | `true` | build the EFI file: kernel, initramfs and command line in one |
| `EFI.ImageDir` | `/boot/efi/EFI/zbm` | where the EFI file is written |
| `EFI.Versions` | `false` | one image and one `-backup` copy with fixed names, so the firmware entries never change |
| `Kernel.Prefix` | `vmlinuz` | names the images `vmlinuz.EFI` and `vmlinuz-backup.EFI` (see below) |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's own** kernel, built into the EFI file |

Without `Kernel.Prefix`, Arch's kernel file `/boot/vmlinuz-linux-lts` would give `vmlinuz-linux-lts.EFI`.

| Option | `Kernel.CommandLine` | Why |
|---|---|---|
| A, B | `quiet loglevel=0 zbm.luks=off` | do not unlock any LUKS volume |
| C | `quiet loglevel=0 zbm.luks=UUID=<LUKS UUID>` | unlock only this container, as `luks-<UUID>` |

- Without `zbm.luks=off`, ZFSBootMenu would offer every LUKS volume it finds, for example on a USB disk plugged in
  at boot.
- Option C's UUID is that of the **LUKS container** (`cryptsetup luksUUID`), not the pool's GUID and not a file
  system UUID.

Options you may add later: `zbm.timeout=SECONDS` changes the countdown (default 10; `-1` always shows the menu, `0`
boots at once); `loglevel=7` shows ZFSBootMenu's messages for troubleshooting.

**Which kernel the image uses.** `generate-zbm` takes the newest kernel among `/boot/vmlinuz*`, reading its version
from the kernel file itself (Arch's file names carry none), and needs the ZFS module for that kernel. This guide
installs only `linux-lts`. If you install a second kernel package later, pin the image to the one whose ZFS module
you trust by adding `Path` to the `Kernel` section, beside `Prefix` and `CommandLine`:

```
Kernel:
  Path: /boot/vmlinuz-linux-lts
  Prefix: vmlinuz
  CommandLine: "..."
```

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

> **Warning**
> `generate-zbm` runs dracut with this directory instead of `/etc/dracut.conf.d`, but dracut still reads
> `/etc/dracut.conf` and `/usr/lib/dracut/dracut.conf.d/`. Never name a key file in either: it would end up in the
> ZFSBootMenu image on the unencrypted ESP.

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
runtime after a beta. Only the Rust runtime has the locked menu, measured boot, and keyboard layouts other than US at
the passphrase prompt. To choose it:

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

- add `zbm.console=drm` to `CommandLine`, so that ZFSBootMenu runs on its own console on the display, which types
  with the image's first layout. The image needs the kernel's `evdev` module and a DRM driver for the display
  (`simpledrm` on the firmware's framebuffer is enough), for example with `add_drivers+=" evdev "` in a file of its
  own in `/etc/zfsbootmenu/dracut.conf.d/`. When that console cannot use the display, ZFSBootMenu falls back to the
  kernel console, which types with the US layout;
- or, on the default kernel console, press **Ctrl-K** at the prompt to choose a layout first.

The image gets the layouts of `zfsbootmenu_keyboard_layouts`; when it is unset, the layout of `XKBLAYOUT` in
`/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`. This guide's `/etc/vconsole.conf` has only `KEYMAP=` (the
console layout), so name your layout, for example:

```
(chroot) root # echo 'zfsbootmenu_keyboard_layouts="de us"' >> /etc/zfsbootmenu/dracut.conf.d/runtime.conf
```

**Check:**

```
(chroot) root # cat /etc/zfsbootmenu/dracut.conf.d/*.conf | grep -E 'hostonly|zfsbootmenu_|add_drivers'
```

`hostonly="no"`; `zfsbootmenu_runtime=rust`, `zfsbootmenu_keyboard_layouts=` and `add_drivers+=" evdev "` only if you
added them.

### Building and checking the image

Why: build the image, then build it again so that the backup entry has an image to start from the beginning.

```
(chroot) root # generate-zbm --dry-run --debug
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

- `--dry-run` prints one `Would ...` line for each change and makes none; it names the kernel it would use
  (`/boot/vmlinuz-linux-lts`). `--debug` prints every command.
- The first run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`; the second renames it to `vmlinuz-backup.EFI` and writes a
  new one.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | `make install` was not run |
| `failed to install essential executable '<name>'` | a program is missing: often `kexec`, or `fzf` (shell runtime) |
| `Unable to find UEFI stub loader at default locations` | the EFI stub is missing; see [Dependencies](#dependencies) |
| `ManageImages not enabled, no action taken` | `config.yaml` was overwritten by `make install`; write it again |
| `Unable to locate libgcc_s.so` | the GCC runtime library is missing (Arch's `gcc-libs`) |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (see below) |

The host ID message: the SPL module's `spl_hostid` parameter differs from `/etc/hostid` (the check is skipped while
the parameter is 0, as on the live system; on the installed system it appears when `/etc/hostid` was changed after
boot). The image takes `/etc/hostid`, which must be `00bab10c` (`od -A n -t x4 /etc/hostid`).

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
| first `grep` lists | `etc/hostid`, `zfs.ko` (maybe `.zst`), `nvme` | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

The second `grep` must print nothing: no key file and no crypttab may be in ZFSBootMenu's image. If it prints a line,
do not boot this image; find out why first (a key file named in `/etc/dracut.conf` or in
`/usr/lib/dracut/dracut.conf.d/`). If `cryptsetup` is listed for Options A or B, that is fine: it is in the image
whenever the package is installed, and `zbm.luks=off` keeps it unused. The `nvme` line lists the driver module;
whether Arch's `linux-lts` builds the NVMe driver as a module was not confirmed for this guide, and if it is built in,
nothing is listed for it.

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

Why: a snapshot of the fresh system to return to, and a clean export of the pool, which every later import expects.

```
(chroot) root # rm /root/install.env
(chroot) root # exit
root # zfs snapshot "${POOL}/ROOT/${BE}@install"
root # umount /mnt/boot/efi
root # zfs umount -a
root # zpool export "${POOL}"
```

`arch-chroot` has already unmounted `/proc`, `/sys`, `/dev`, `/run` and `/tmp` below `/mnt` when you left it.

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
because the pool is busy, leave any shell that is still inside `/mnt` (`findmnt -R /mnt` shows what is still
mounted) and repeat the unmount and export commands.

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
| 2 | `Booting zroot/ROOT/arch in 10 seconds`, with `[RETURN] boot now` and `[ESCAPE] boot menu` | the same | the same |
| 3 | | the screen clears; `Enter passphrase for 'zroot':`. Type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Arch's kernel | the same | the same |
| 5 | Arch starts | Arch starts **without** asking again | Arch starts **without** asking again |

- **Enter** boots at once; **Escape** during the countdown opens the menu, which lists `zroot/ROOT/arch`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: `zfs load-key` allows three attempts. After three wrong passphrases ZFSBootMenu opens its emergency shell
  with `unable to load key for zroot/ROOT/arch`. Type `exit` there; the menu then asks again.
- With the Rust runtime the same questions appear as dialogs (`Unlock <volume>` for Option C, a ZFS key dialog for
  Option B), and the countdown reads `Booting <environment> in <N> s. Press any key to choose another.`

If Arch asks for a passphrase a second time, or stops in the busybox shell of its initramfs (a prompt that starts
with `[rootfs`), see [The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock].

Log in as root and check the system:

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status zroot
root # hostid
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
root # systemctl --failed
```

**Option B:**

```
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/arch
```

Both datasets show `aes-256-gcm`, the encryption root `zroot` and the key status `available`. Only the encryption
root has a key location: `zroot` shows `file:///etc/zfs/zroot.key`, and `zroot/ROOT/arch` shows `none`.

**Option C:**

```
root # LUKS_UUID="$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)"
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `/proc/cmdline` | the parameters (1) | the same | the same, with `cryptdevice=` and `cryptkey=` |
| `/` | `zroot/ROOT/arch` | the same | the same |
| `zpool status` | `ONLINE`, on `nvme0n1p2` | the same | `ONLINE`, on `luks-<UUID>` |
| `hostid` | `00bab10c` | the same | the same |
| encryption | | `aes-256-gcm`, `available` | `LUKS2`, `4096`, `discards` |
| asked for the passphrase | never | once, by ZFSBootMenu | once, by ZFSBootMenu |
| `systemctl --failed` | `0 loaded units listed` | the same | the same |

(1) `rw quiet`, `zfs=zroot/ROOT/arch` and `spl.spl_hostid=0x00bab10c`.

`BootCurrent` names the entry the firmware used. If it is the number of the `ZFSBootMenu` entry, the firmware honours
entries and `\EFI\BOOT\BOOTX64.EFI` is only a reserve. If it is another number, the firmware started the copy; the
post-hook `50-bootx64.sh` of [Creating the firmware boot entries](#creating-the-firmware-boot-entries) keeps it
current, so check after each `generate-zbm` that it ran (see
[Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image)).

Once the system has booted, do these three things:

1. Install the pacman hook that rebuilds ZFSBootMenu's image after updates:
   [Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image).
2. **Option C:** make the first LUKS header backup:
   [Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header).
3. Keep the live medium until the system has booted several times, including once after a kernel update.

## Usage

### Updating the system

Take a snapshot of the boot environment before large updates; [Boot environments, snapshots and
rollback](#boot-environments-snapshots-and-rollback) shows how to go back:

```
root # zfs snapshot zroot/ROOT/arch@pre-update-$(date +%Y%m%d)
root # pacman -Syu
```

What happens on its own, and what you must check:

- A new **`linux-lts`** comes together with the matching **`zfs-linux-lts`**; pacman refuses one without the other
  (see [pacman -Syu stops on zfs-linux-lts](#pacman--syu-stops-on-zfs-linux-lts)). mkinitcpio's pacman hook
  (`90-mkinitcpio-install.hook`) copies the kernel to `/boot` and builds its initramfs with the configuration of
  [Configuring the initramfs](#configuring-the-initramfs), key file included. The hook of the next section then
  rebuilds ZFSBootMenu's image.
- A new **`zfs-utils`** or **`cryptsetup`** changes files that mkinitcpio's hook watches, so the initramfs is rebuilt
  too; the hook of the next section rebuilds ZFSBootMenu's image.
- As on any Arch system, a kernel update removes the running kernel's modules from `/usr/lib/modules`; reboot soon
  after it.

After an update that brought a kernel, check before rebooting that the kernel and the initramfs fit together:

```
root # pacman -Q linux-lts zfs-linux-lts
root # lsinitcpio /boot/initramfs-linux-lts.img | grep -E 'zfs\.ko'
root # ls -l /boot /boot/efi/EFI/zbm/
```

The version of `zfs-linux-lts` contains that of `linux-lts`, the initramfs contains `zfs.ko`, for Options B and C
`initramfs-linux-lts.img` is `-rw-------`, and `vmlinuz.EFI` has the time of the update.

### Regenerating the ZFSBootMenu image

ZFSBootMenu's image contains the kernel, the ZFS module and tools, cryptsetup and the other tools of **this system at
the time of the build**. Rebuild it after updates of the ZFS packages, of the kernel, of cryptsetup, and after
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
- `generate-zbm` uses the newest kernel in `/boot` and needs the ZFS module for it. Rebuild **before** running
  `zpool upgrade` or raising `compatibility`.
- The post-hook `50-bootx64.sh` refreshes the firmware's fallback copy `\EFI\BOOT\BOOTX64.EFI` after every build.
  `cmp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI` prints nothing when the two are the same;
  `generate-zbm --debug` also prints `## Processing hook: ...50-bootx64.sh`. The `known-good.EFI` copy above is never
  refreshed; that is its purpose.

**The pacman hook.** A pacman hook rebuilds the image after the transactions that need it:

```
root # mkdir -p /etc/pacman.d/hooks
```

**FILE** `/etc/pacman.d/hooks/95-zfsbootmenu.hook`

```
[Trigger]
Type = Package
Operation = Install
Operation = Upgrade
Target = zfs-utils
Target = zfs-dkms
Target = zfs-linux*
Target = cryptsetup
Target = systemd
Target = dracut
Target = kexec-tools
Target = fzf

[Trigger]
Type = Path
Operation = Install
Operation = Upgrade
Target = usr/lib/modules/*/vmlinuz

[Action]
Description = Rebuilding the ZFSBootMenu image...
When = PostTransaction
Exec = /usr/bin/generate-zbm
```

- The `Path` trigger fires when any kernel package is installed or updated.
- The other targets are packages whose files end up in the image: `systemd` supplies the EFI stub (see
  [Dependencies](#dependencies)) and the cryptsetup token plugins, `dracut` builds the image, `kexec-tools` and (for
  the shell runtime) `fzf` are inside it. `systemd` updates often; each one costs a rebuild of a minute or less.
- `When = PostTransaction` hooks run in the order of their file names. `95-` runs after mkinitcpio's
  `90-mkinitcpio-install.hook` (and, if you use `zfs-dkms`, after DKMS's hooks; check their numbers with
  `ls /usr/share/libalpm/hooks/`), so the ZFS module for a new kernel exists when `generate-zbm` runs.
- A target that is not part of a transaction does nothing; `zfs-dkms` may stay in the list.
- If the build fails, pacman shows `generate-zbm`'s error, and the previous image stays. Look at
  `ls -l /boot/efi/EFI/zbm/` before rebooting, fix the cause, and run `generate-zbm` again.

This hook was reviewed for this repository's guide for existing Arch systems; the project has not run it on a new
installation.

**Check:** reinstall the **installed** version of `zfs-utils` from pacman's package cache, which triggers the hook
without updating anything. Do not use `pacman -S zfs-utils` for this: it installs the version in the sync database,
which may be newer than the installed one and no longer match the installed `zfs-linux-lts`.

```
root # ls /var/cache/pacman/pkg/zfs-utils-"$(pacman -Q zfs-utils | cut -d' ' -f2)"-*.pkg.tar.zst
root # pacman -U /var/cache/pacman/pkg/zfs-utils-"$(pacman -Q zfs-utils | cut -d' ' -f2)"-*.pkg.tar.zst
root # ls -l --time-style=full-iso /boot/efi/EFI/zbm/
```

The transaction ends with `Rebuilding the ZFSBootMenu image...`; `vmlinuz.EFI` has a new time, and
`vmlinuz-backup.EFI` is the image that was there before. If the cache no longer holds the package (`paccache` or
`pacman -Sc` removed it), skip this check and watch for the message at the next real update of a target package.

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Snapshots of
it are taken on the running system (`zfs snapshot zroot/ROOT/arch@NAME`) or in ZFSBootMenu.

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
| **MOD+C** | clone | a new boot environment that depends on the snapshot |
| **MOD+X** | clone and promote | a new boot environment that keeps the snapshots |
| **Enter** | duplicate | an independent copy (`zfs send`, `zfs recv`), as large as the snapshot |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

**Rust runtime.** Press **2** or **s** for the snapshots of the selected environment, then **r** to roll back; a
review page shows what is lost, and the change is made once the target's name is typed. **?** lists the keys of
every view.

Example: going back after a bad update.

1. Before the update, on the running system: `zfs snapshot zroot/ROOT/arch@pre-update`.
2. After a bad update, reboot, press **Escape** in the countdown, select `zroot/ROOT/arch`, press **MOD+S**, select
   `@pre-update`, press **MOD+R** and confirm.
3. Or, to try the old state without losing the new one, press **MOD+C** (clone) instead and boot the clone.

- Rollback and the other changes need the pool imported read/write; ZFSBootMenu imports read-only and re-imports when
  needed. In Option B that re-import needs the key again; the key source set in
  [Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties) saves the second prompt.
- A rollback of the boot environment does not touch `zroot/home`.
- pacman's database lives in `/var/lib/pacman`, inside the boot environment, so a rollback also rolls back what pacman
  believes is installed; the two stay consistent.
- A boot environment you create by hand needs `mountpoint=/` and `canmount=noauto`; it inherits
  `org.zfsbootmenu:commandline` from `zroot/ROOT`.
- Snapshots and clones contain their `/boot`, so each boots with the kernel and initramfs (and, in Options B and C,
  the key file) it had when the snapshot was taken. After the key file or the passphrase was replaced, an older
  snapshot's initramfs holds the old one, and its system asks for the passphrase (see
  [Changing the passphrase](#changing-the-passphrase)).

`zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the `org.zfsbootmenu:commandline` property
that every boot environment inherits; see `man zbm-kcl`. Without a dataset name `zbm-kcl` acts on the root file system,
`zroot/ROOT/arch`, and a change there sets a property of its own on that boot environment only.

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
> Rebuild Arch's initramfs now. It holds a copy of the old file; with it, the archzfs `zfs` hook fails to load the
> key at the next boot and asks for the passphrase (`Unable to load key ...; please type the passphrase`), so
> booting still works, but with a second prompt.

```
root # mkinitcpio -P
root # ls -l /boot/initramfs-*.img
```

Every image is still `-rw-------`. ZFSBootMenu's image holds no key, so `generate-zbm` is not needed.

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
root # mkinitcpio -P
```

`luksAddKey` asks for the passphrase. Rebuilding the initramfs is required: it contains a copy of the key file. Continue
only when `mkinitcpio -P` printed no error, `ls -l /boot/initramfs-*.img` shows every image `-rw-------`, and
`lsinitcpio /boot/initramfs-linux-lts.img | grep zroot.key` lists the key file. Then remove the old key slot and the old
file:

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

A scrub reads and checks every block; a full TRIM complements `autotrim`. archzfs's `zfs-utils` ships the systemd
timers `zfs-scrub-monthly@.timer` and `zfs-trim-monthly@.timer` (also `-weekly@` variants). Enable them for the pool:

```
root # systemctl enable --now zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
root # systemctl list-timers 'zfs-*'
```

Option C: TRIM reaches the SSD because the flag is in the LUKS header; `lsblk -D` shows non-zero discard sizes for
`luks-<UUID>`. The SSD's own health: `pacman -S nvme-cli`, then `nvme smart-log /dev/nvme0`.

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
- A newer checkout may need a newer Rust than Arch's `rust` package offers; `make` then names the version
  (`rust-version` in `Cargo.toml`). Update the system first with `pacman -Syu`.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm it works before relying on it; the
backup entry still boots the previous image.

### Relaxing the pool compatibility

A step most readers can skip. The pool was created with `compatibility=openzfs-2.2-linux`. The OpenZFS from archzfs
(2.4 at the time of writing) knows newer features; to enable them, first rebuild ZFSBootMenu's image, then raise the
set:

```
root # generate-zbm
root # zpool set compatibility=openzfs-2.3-linux zroot
root # zpool upgrade zroot
```

- Update every system that imports the pool before enabling new features, starting with ZFSBootMenu, and check that
  your rescue medium's ZFS is new enough. Use the newest set that the installed ZFS **and** the rescue medium both
  know (`ls /usr/share/zfs/compatibility.d/`).
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
`pacman -S --needed cryptsetup` in the chroot, run `generate-zbm`, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

After ZFSBootMenu started Arch's kernel, Arch's initramfs could not use the key file. With mkinitcpio's busybox hooks:

- **Option B**: the `zfs` hook prints `Unable to load key file:///etc/zfs/zroot.key; please type the passphrase` and
  asks for the ZFS passphrase. Type it; the boot goes on.
- **Option C**: the `encrypt` hook prints `Keyfile could not be opened. Reverting to passphrase.` or
  `Invalid keyfile. Reverting to passphrase.` and asks for the LUKS passphrase. Type it; the boot goes on.
- **All options**: if the root cannot be mounted, the boot stops in the busybox shell (prompt `[rootfs`). See
  [The pool is refused](#the-pool-is-refused-wrong-host-id-or-last-used-by-another-system) and the causes below.

Then, on the running system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitcpio /boot/initramfs-linux-lts.img | grep -E 'zroot\.key|hooks/(zfs|encrypt)$'
root # cat /etc/mkinitcpio.conf.d/*.conf
```

Causes, in order of likelihood:

- the key file is not in the initramfs: `/etc/mkinitcpio.conf.d/90-zbm-key.conf` is missing or misspelled, or a
  preset in `/etc/mkinitcpio.d/` has an active `*_config=` line, which makes mkinitcpio ignore the drop-ins. Fix it
  and run `mkinitcpio -P`;
- Option C: the command line lacks `cryptkey=rootfs:/etc/cryptsetup-keys.d/zroot.key`, or `cryptdevice=` carries
  another UUID than `cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks`
  ([Setting the ZFSBootMenu properties](#setting-the-zfsbootmenu-properties));
- `HOOKS` is not the busybox list of [Configuring the initramfs](#configuring-the-initramfs): with the `systemd` hook,
  neither `encrypt` nor archzfs's `zfs` runs;
- the command line lacks `rw`: the `zfs` hook imports the pool read-only, and the boot stops later;
- Option B: the passphrase was changed without rebuilding the initramfs
  ([Changing the passphrase](#changing-the-passphrase));
- an old snapshot or clone was booted, whose initramfs holds an older key.

If ZFSBootMenu **itself** asks twice in Option C, `zbm.luks=` is missing from its command line, and it offers every
LUKS volume it finds.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment. In the recovery shell (Option B: `zfs load-key -L prompt zroot` first; the
key file named by `keylocation` is not in ZFSBootMenu's image):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/arch
root # ls -l MOUNTPOINT/boot
```

`mount_zfs` prints the mount point to use for `MOUNTPOINT`. Causes:

- `/boot` in the dataset is empty: the kernels were written to the ESP or to another partition mounted at `/boot`
  (an `/etc/fstab` line for `/boot`, a preset with a `_uki=` line, or a boot loader package such as systemd-boot with
  `kernel-install` can do that);
- a kernel has no initramfs beside it: Arch's pair is `vmlinuz-linux-lts` and `initramfs-linux-lts.img`;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

- In ZFSBootMenu the default import policy adopts the host ID of the pool when it cannot import it with its own, so
  this usually resolves itself. Otherwise, in the recovery shell: `zpool import -f -N zroot`, then `exit`.
- In the busybox shell of Arch's initramfs (after `ZFS: Unable to import pool zroot.`): `zpool import -f -N zroot`,
  then reboot. The `zfs` hook adds `-f` itself only with `zfs_force=1` on the command line; ZFSBootMenu's **MOD+E**
  lets you add that for one boot.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; run `mkinitcpio -P` and
  `generate-zbm`. The usual cause is a live system that imported the pool and was shut down without `zpool export`.
  Always export the pool on a rescue medium.

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
| `zluks`, `zluks list`, `zluks status`, `zluks open DEVICE [NAME]`, `zluks close` | Option C: LUKS volumes |
| `logs`, `zlogtail`, `zreport`, `zbmcmdline` | logs, a report for bug reports, ZFSBootMenu's command line |
| `help` | the built-in documentation |
| `reboot`, `poweroff`, `firmware-setup` | leave |

### Repairing the system from the live medium

Boot the Arch ISO with `cow_spacesize=4G` and add ZFS to it as in [Installation media](#installation-media), steps 1
to 5. Then:

```
root # export DISK=/dev/nvme0n1
root # export ESP_PART="${DISK}p1"
root # export POOL_PART="${DISK}p2"
root # zgenhostid -f 0x00bab10c
```

`zgenhostid` gives the live system the same host ID as the installed system, so that the pool, when exported at the
end, records the host ID that ZFSBootMenu and Arch use.

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
root # zfs mount zroot/ROOT/arch
root # zfs mount zroot/home
root # mount "${ESP_PART}" /mnt/boot/efi
root # arch-chroot /mnt /bin/bash
```

`-f` imports the pool even if the installed system did not export it; with the same host ID it is usually not
required, and here it is harmless. The stored TRIM flag is applied by
`cryptsetup open` without options. Repair what is needed, then leave the chroot and finish as in
[Finalizing the installation](#finalizing-the-installation), without the snapshot: unmount, `zpool export zroot`, and
for Option C `cryptsetup close`. Do not run `zpool upgrade` from a live system.

### The live system cannot build the ZFS module

`pacman -S zfs-dkms` on the live system ends with a DKMS error, or `modprobe zfs` reports `Module zfs not found`.

```
root # uname -r
root # grep '^Linux-' /usr/src/zfs-*/META
root # pacman -Q linux-headers
root # dkms status
```

- `uname -r` is newer than `Linux-Maximum`: the ISO's kernel is too new for this OpenZFS release. Use an older ISO
  from <https://archive.archlinux.org/iso/> whose kernel is in range (the ISO's date is in its file name).
- `pacman -Q linux-headers` does not show the version of `uname -r` (with `.arch` for `-arch`): repeat step 4 of
  [Installation media](#installation-media); the headers must match exactly.
- `dkms status` lists the module as `added` or `built` but not `installed` for the running kernel: run
  `dkms autoinstall -k "$(uname -r)"`, then `modprobe zfs`.
- The build ran out of space: the live system was booted without `cow_spacesize=4G` (`df -h /run/archiso/cowspace`
  shows the size). Reboot and start again from step 1.
- `zpool` fails with errors about missing symbols or libraries: archzfs's current `zfs-utils` needs newer libraries
  than the ISO's date offers. Use the newest ISO, or one of the alternatives in
  [Installation media](#installation-media).

### pacman -Syu stops on zfs-linux-lts

`pacman -Syu` ends with an error such as `installing linux-lts (NEW) breaks dependency 'linux-lts=OLD' required by
zfs-linux-lts`, and changes nothing.

Arch published a new `linux-lts` before archzfs built the matching `zfs-linux-lts`. The system is unchanged and keeps
working. Wait until archzfs publishes the new package (`pacman -Sy` and `pacman -Si zfs-linux-lts` show its
`Depends On`), then update again. Do not work around it with `--nodeps`, `--assume-installed` or `--ignore`: a partial
upgrade is not supported on Arch, and a kernel without its ZFS module cannot import the pool.

During the installation the same lag makes `pacman -S ... linux-lts zfs-linux-lts` fail with `unable to satisfy
dependency 'linux-lts=…'`; [Kernel and ZFS](#kernel-and-zfs) shows how to install the kernel version that
`zfs-linux-lts` was built for from the Arch Linux Archive.

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption leaves the pool's structure readable without the passphrase: dataset and snapshot names, the
  dataset hierarchy, properties (including `org.zfsbootmenu:commandline`), file sizes and holes, and deduplication
  tables. File and volume data, file attributes, ACLs, permission bits and directory listings are encrypted.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Arch's initramfs in
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
  ESP, an unencrypted partition, a backup, or a unified kernel image (UKI). Snapshots and `zfs send` streams of the
  root dataset contain it too; treat them like the pool.
- A LUKS header backup contains the key slots as they were: an old passphrase still opens a restored header. Store
  backups offline and replace them after key changes.
- ZFSBootMenu closes the container before `kexec`, which wipes the volume key from kernel memory.
- TRIM pass-through reveals which blocks are unused, not their content.

**All options.**

- **The ESP is not encrypted.** Anyone with access to the disk, or root on the system, can replace ZFSBootMenu's
  image with one that records the passphrase. Encryption protects data at rest, for example a stolen powered-off
  laptop, not against someone who can modify the computer and wait for you to type the passphrase. Secure Boot with
  signed images, or measured boot with a TPM, narrows that; neither is set up by this article. (`contrib/zbm-sign.pl`,
  a `generate-zbm` post-hook described in [Building ZFSBootMenu](../../general/building.rst), signs images.)
- Once the pool is unlocked, the recovery shell and the command-line editor are open to anyone at the keyboard.
- Hooks run as root in ZFSBootMenu; install only hooks you have read.
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed
  before updating, keep the backup entries, and keep the rescue medium.
- **archzfs is a third-party repository** that supplies a kernel module and tools running as root. Its packages are
  signed with a key you trusted by hand in this guide; keep `SigLevel = Required`.

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
- [Adding ZFSBootMenu to an existing LUKS-encrypted ZFS root (Arch Linux)](existing-luks-system.md), for an Arch
  system that is already installed
- [Gentoo Linux on a LUKS-encrypted ZFS root](../gentoo/zfs-luks-v4.md), the model of this guide's form
- [The threat model](../../dev/threat-model.md)
- Arch Linux: the *Installation guide*, *ZFS*, *Install Arch Linux on ZFS*, *mkinitcpio*,
  *dm-crypt/System configuration*, *Archiso* and *Arch Linux Archive* pages of the Arch Wiki; `mkinitcpio(8)`,
  `pacstrap(8)`, `arch-chroot(8)`, `alpm-hooks(5)`; `mkinitcpio -H zfs` and `mkinitcpio -H encrypt` for the hooks'
  parameters
- archzfs: [the project](https://github.com/archzfs/archzfs), [its wiki](https://github.com/archzfs/archzfs/wiki),
  and the [`experimental` release](https://github.com/archzfs/archzfs/releases/tag/experimental) with the repository
  configuration
- OpenZFS: the [OpenZFS documentation](https://openzfs.github.io/openzfs-docs/) (its *Arch Linux Root on ZFS* page
  uses another layout and boot loader)
- `gdisk(8)`, `cryptsetup(8)`
