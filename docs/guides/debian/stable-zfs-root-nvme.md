# Debian Stable on a ZFS root with ZFSBootMenu on one NVMe disk

This article describes how to install Debian Stable, Debian 13 "trixie", from scratch on a **single NVMe SSD** with a
**ZFS root pool**. The system boots with the ZFSBootMenu build from this repository (branch `claude/zbm-refactor`).
Debian is installed with `debootstrap` from the Debian live image; its own initramfs is built by **initramfs-tools**,
and dracut is used only to build ZFSBootMenu's image.

Choose one of three options at the start and follow it through the article:

- **Option A**: the pool is not encrypted.
- **Option B**: the pool uses **ZFS native encryption**.
- **Option C**: the pool is inside a **LUKS2** container.

With Options B and C you type the passphrase **once**, in ZFSBootMenu. Debian's initramfs then unlocks the pool again
with a **key file** stored inside the encrypted pool.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

> **Note**
> This guide was written from the documentation of Debian, OpenZFS and this repository; some Debian package details
> were read in Ubuntu's packages, which are derived from Debian's. The project has not run it from start to finish.
> `claude/zbm-refactor` is a development branch, not a release.
> The project's VM tests boot ZFSBootMenu images built with dracut, but only a test system whose initramfs is
> built with dracut: Debian's initramfs-tools initramfs, which this guide uses, is not covered by them.
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
  - [The ZFS module is missing for a kernel](#the-zfs-module-is-missing-for-a-kernel)
  - [apt wants to remove initramfs-tools](#apt-wants-to-remove-initramfs-tools)
- [Security notes](#security-notes)
- [See also](#see-also)

[no-cryptsetup]: #option-c-zfsbootmenu-never-asks-for-the-passphrase-the-image-lacks-cryptsetup
[hostid-refused]: #the-pool-is-refused-wrong-host-id-or-last-used-by-another-system

## Overview

### Choosing an option

| | Option A | Option B | Option C |
|---|---|---|---|
| Pool | not encrypted | ZFS native encryption (`aes-256-gcm`) | inside a LUKS2 container |
| Passphrase typed | never | once, in ZFSBootMenu | once, in ZFSBootMenu |
| Second unlock, by Debian's initramfs | none | key file (1) | key file (2) |
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
3. ZFSBootMenu imports the pool and shows the boot environment `zroot/ROOT/debian` with a countdown of 10 seconds.
   When the countdown ends, the pool's `bootfs` is booted; a key press opens the menu instead.
4. **Option B**: the `keylocation` of the boot environment's encryption root (`zroot`) names a file that is not in
   ZFSBootMenu's image, so ZFSBootMenu asks for the **ZFS passphrase** and loads the key. With the default runtime this
   prompt comes **after** the countdown.
5. ZFSBootMenu loads Debian's kernel and initramfs from `/boot` **inside the boot environment** and starts the kernel
   with `kexec`. It passes the command line stored in the ZFS property `org.zfsbootmenu:commandline`, the root
   argument `root=zfs:zroot/ROOT/debian` and the host ID. **Option C**: before `kexec` it exports the pool and closes
   the container, which wipes the volume key from memory.
6. Debian's initramfs (initramfs-tools with OpenZFS's `zfs` scripts) starts in the new kernel:
   - **Option A**: it imports the pool and mounts the root file system.
   - **Option B**: it imports the pool and loads the ZFS key from `/etc/zfs/zroot.key` **inside the initramfs**,
     without asking.
   - **Option C**: cryptsetup's initramfs script unlocks the container again with the key file
     `/etc/cryptsetup-keys.d/zroot.key` **inside the initramfs**, without asking; then the pool is imported.

Why a second unlock (Options B and C): `kexec` starts a new kernel, which holds no loaded ZFS keys and no open
dm-crypt mappings. The boot environment's own initramfs must unlock the pool again, exactly as it would without
ZFSBootMenu. ZFSBootMenu does not replace Debian's initramfs.

### Where the keys are

**Option A.** There are none.

| Key | Option | Stored where | Used by |
|---|---|---|---|
| ZFS passphrase | B | your memory | ZFSBootMenu at every boot; a live system for repairs |
| The same passphrase in `/etc/zfs/zroot.key` | B | in the pool (see below) | Debian's initramfs |
| LUKS passphrase (key slot 0) | C | your memory | ZFSBootMenu at every boot; a live system for repairs |
| Key file `/etc/cryptsetup-keys.d/zroot.key` (slot 1) | C | in the pool (see below) | Debian's initramfs |

"In the pool" means twice: the file itself in `/etc`, and a copy of it in Debian's initramfs in `/boot`. Both
directories are inside the encrypted pool.

The key file in Debian's initramfs is safe **only because** the file and the initramfs both live inside the encrypted
pool (`/etc` and `/boot`); they cannot be read before the passphrase has been typed. ZFSBootMenu's own image lives on
the unencrypted ESP and never contains a key.

### What the project has tested

From [Testing](../../luks/testing.rst):

- The VM tests boot ZFSBootMenu images built **with dracut** by the project's build container (Ubuntu 26.10,
  Linux 7.3, OpenZFS 2.4.4, cryptsetup 2.8.7, systemd 261, dracut 112), in QEMU.
- The system they boot is a test payload, not an installed distribution: an Ubuntu kernel with a **dracut**
  initramfs and a minimal root file system. "Boot environments whose initramfs is not built by dracut (for example
  mkinitcpio or initramfs-tools)" are listed as not covered.
- For this guide that means: building ZFSBootMenu's image with dracut is the tested path, but on Debian it is built
  with Debian's kernel (6.12), OpenZFS (2.3) and dracut, which the tests do not use. Debian's **initramfs-tools**
  initramfs, which does the second unlock in Options B and C, is **not covered**.
- Debian 13's dracut is 106 (`dracut-core` 106-6); the project builds and tests ZFSBootMenu with dracut 112.
  Building this repository's dracut module with dracut 106 has not been tried. The
  [Dependencies](#dependencies) Check shows the version you get.
- The `oldzfs` tests boot an image built with OpenZFS 2.2.2 and Linux 6.8; no test uses OpenZFS 2.3, Linux 6.12 or
  dracut 106.
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
  Debian's `/etc/crypttab` and cryptsetup-initramfs.
- Real hardware and Secure Boot are not covered.

## Requirements

### Hardware

- An x86_64 computer with an NVMe SSD that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware: the ZFSBootMenu image built here is not signed, and the live system
  cannot load a self-built ZFS module with it. How to check is shown in [Installation media](#installation-media).
- An internet connection, wired if possible.
- Several GiB of RAM: the live system keeps the packages it installs in memory, and it builds the ZFS module.
- Free space in the pool for the build: a few GB for the clone of this repository, the Rust toolchain in
  `/root/.rustup` and Cargo's build directory. The size of the build directory was not measured for this guide.

### Installation media

The facts this guide depends on, and how to check each on your system:

| Fact | Value in Debian 13 | Check |
|---|---|---|
| Release | Debian 13 "trixie" is stable; point releases (13.x) only update packages | `cat /etc/debian_version` |
| Kernel | Linux 6.12, package `linux-image-amd64` | `uname -r` |
| OpenZFS | 2.3, from `contrib` (`zfs-dkms`, `zfsutils-linux`, `zfs-initramfs`) | `zfs version` |
| System initramfs | initramfs-tools | `update-initramfs --version` |
| dracut | 106 (`dracut-core`), only for ZFSBootMenu's image | `dpkg-query -W dracut-core` |
| `rustc` | 1.85, older than the 1.88 this repository needs | `apt-cache policy rustc` |
| EFI stub | package `systemd-boot-efi` | `ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub` |

- OpenZFS 2.4 is in `trixie-backports`; this guide uses the 2.3 of Debian 13 itself.
- This guide installs Rust with `rustup`; see [Dependencies](#dependencies).
- The versions of the kernel, OpenZFS, `rustc` and dracut were taken from search results of Debian's package pages,
  which could not be opened from where this guide was written. The commands in the right-hand column show the
  versions on your system.

Use the current **Debian 13 live image** for amd64, from
<https://cdimage.debian.org/debian-cd/current-live/amd64/iso-hybrid/> (any desktop flavour, or `standard` without a
desktop). Write it to a USB stick and boot it in **UEFI** mode. Log in,
if asked, as `user` with the password `live`, connect to the network and open a terminal.

On a live image with a desktop, stop the desktop from mounting old partitions of the SSD (run this as `user`, before
becoming root; the command is for GNOME):

```
user $ gsettings set org.gnome.desktop.media-handling automount false
```

Become root, and check that the live system booted in UEFI mode and that Secure Boot is off. With Secure Boot on,
the kernel refuses the ZFS module that DKMS builds below.

```
user $ sudo -i
root # ls /sys/firmware/efi
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
```

- `ls` lists files; otherwise the live system booted in legacy BIOS mode. Boot it again in UEFI mode.
- The last number printed by `od` is `0` when Secure Boot is disabled and `1` when it is enabled. If it is `1`,
  disable Secure Boot in the firmware setup first. If the file does not exist, the firmware has no Secure Boot.

Add the `contrib` component, which holds ZFS:

```
root # ls /etc/apt/sources.list.d/
root # cat > /etc/apt/sources.list <<EOF
deb http://deb.debian.org/debian/ trixie main contrib non-free-firmware
EOF
root # apt update
```

This guide could not confirm where the Debian 13 live image keeps its package sources. If `ls` lists a file ending
in `.sources` that names `trixie`, do not write `/etc/apt/sources.list`; add `contrib` to the `Components:` line of
that file instead (for example `Components: main contrib non-free-firmware`), then run `apt update`. `apt update`
must not warn about targets configured multiple times.

Install the tools. The second command builds the ZFS module for the live system's kernel with DKMS, which takes a
few minutes. `zfs-dkms` shows a note that the licenses of OpenZFS and Linux are incompatible; press Enter.

```
root # apt install --yes debootstrap gdisk dkms linux-headers-$(uname -r) dosfstools nvme-cli
root # apt install --yes zfsutils-linux zfs-dkms
root # modprobe zfs
```

If `modprobe zfs` prints `Key was rejected by service`, Secure Boot is on; see above.

**Option C** also needs cryptsetup:

```
root # apt install --yes cryptsetup
```

> **Note**
> `linux-headers-$(uname -r)` must match the live system's kernel exactly. If apt cannot find the package, the live
> image is older than the kernel in the Debian archive: download the current live image.

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new Debian system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |
| `<Enter>` | in a transcript: press Enter without typing anything |
| **Option A.** / **Option B.** / **Option C.** | the next paragraph and commands are for that option only |
| **All options.** | the text is for every option again |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Preparing the NVMe SSD

Everything in this section runs on the live system. First check that ZFS works and that the tools are recent
enough.

**Check:**

```
root # zfs version
root # dpkg-query -W -f='${Version}\n' gdisk
root # cryptsetup --version
```

- `zfs version` prints `zfs-2.3...` (the tools) and `zfs-kmod-2.3...` (the module), or later versions. The pool in
  this guide needs 2.2 or later.
- `dpkg-query` prints gdisk's package version, `1.0.9` or later (for example `1.0.10-...`).
- `cryptsetup --version` (Option C only) prints 2.4 or later.

### Identifying the SSD

Why: every following command acts on one device, and the wrong one would be erased.

```
root # nvme list
root # lsblk -d -o NAME,SIZE,MODEL,SERIAL,TRAN
```

`nvme list` shows each NVMe namespace, such as `/dev/nvme0n1`, with its model, serial number and size. `nvme0` is the
controller, `n1` its first namespace; the partitions created below will be `/dev/nvme0n1p1` and `/dev/nvme0n1p2`. In
the `lsblk` output the `TRAN` column of an NVMe SSD shows `nvme`.

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
root # export BE=debian
```

| Variable | Value | Used by | Meaning |
|---|---|---|---|
| `DISK` | `/dev/nvme0n1` | A B C | the NVMe namespace (the whole SSD) |
| `ESP_PART` | `${DISK}p1` | A B C | partition 1, the EFI system partition (1) |
| `POOL_PART` | `${DISK}p2` | A B C | partition 2: holds the pool (A, B) or the LUKS container (C) |
| `POOL` | `zroot` | A B C | the pool's name |
| `BE` | `debian` | A B C | the boot environment: dataset `${POOL}/ROOT/${BE}` |
| `POOL_DEV` | see (2) | A B C | the device `zpool create` gets |
| `LUKS_UUID` | see (3) | C | the UUID of the LUKS container |

1. NVMe partitions have a `p` before the number.
2. Options A and B: `${POOL_PART}`, set in [Creating the ZFS pool](#creating-the-zfs-pool). Option C:
   `/dev/mapper/luks-${LUKS_UUID}`, set in
   [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container).
3. `$(cryptsetup luksUUID "${POOL_PART}")`; it exists only after `luksFormat`.

The name in `BE` is free; ZFSBootMenu does not read it. It recognizes Debian from `/etc/os-release` inside the boot
environment.

Some paths are written literally, because files cannot expand variables. If you change `POOL`, change `zroot` in
these paths too: `/etc/zfs/zroot.key` (Option B), `/etc/cryptsetup-keys.d/zroot.key` (Option C), and every later
command that names `zroot` after the installation.

> **Tip: if the terminal is closed or the live system restarts during the installation**
>
> - **Terminal closed only:** set these variables again; Option C also sets `LUKS_UUID` and `POOL_DEV` again (the
>   first two commands at the end of
>   [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container)). Inside the chroot,
>   run the `chroot` and `source` lines of [Chrooting](#chrooting) again.
> - **Live system restarted:** it has lost everything it kept in memory: the installed packages, the variables,
>   `/etc/hostid`, Option B's `/etc/zfs/zroot.key`, the open LUKS container and the imported pool. Before the pool
>   exists, start again from [Installation media](#installation-media). Once it exists:
>   1. repeat the commands of [Installation media](#installation-media) and set the variables again;
>   2. `zgenhostid -f 0x00bab10c`, before any `zpool` command;
>   3. **Option B:** write `/etc/zfs/zroot.key` again with the **same** passphrase, with the three commands of step 2
>      of [Creating the ZFS pool](#creating-the-zfs-pool);
>   4. **Option C:** open the container with the three commands at the end of
>      [Option C only: creating the LUKS container](#option-c-only-creating-the-luks-container);
>   5. `zpool import -N -R /mnt zroot`, then continue in [Mounting the pool below /mnt](#mounting-the-pool-below-mnt)
>      from the `zfs load-key` line (Option B) or the `zfs mount` lines (Options A and C);
>   6. once the base system is installed: `mount -t tmpfs tmpfs /mnt/run`, `mkdir -p /mnt/run/lock` and
>      `mount "${ESP_PART}" /mnt/boot/efi`, then go on with [Chrooting](#chrooting).

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
| 1 | `${ESP_PART}` (`/dev/nvme0n1p1`) | 1 GiB | `EF00` EFI system partition | `EFI` | the ESP |
| 2, Options A and B | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `BF00` Solaris root | `zroot` | the ZFS pool |
| 2, Option C | `${POOL_PART}` (`/dev/nvme0n1p2`) | the rest | `8309` Linux LUKS | `zroot-luks` | the LUKS2 container |

The ESP is formatted FAT32, mounted at `/boot/efi`, and holds ZFSBootMenu. In Option C the LUKS2 container holds
the pool.

The ESP holds only ZFSBootMenu, but several copies of it: the image, its backup, a copy at `\EFI\BOOT\BOOTX64.EFI`,
and any copy you keep yourself. 1 GiB leaves room for all of them. No Debian kernel or initramfs is ever stored on
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
  `apt install --yes parted && partprobe "${DISK}"`, or reboot the live system and set the variables again.
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
  partition 2 (`d`, `2`) and create it again (`n`, `<Enter>`, `<Enter>`), answering the `Last sector` prompt with
  the number that `echo $(( (LAST + 1) / 2048 * 2048 - 1 ))` prints, where `LAST` is the larger number in the
  prompt's range. Type `8309` as the type, name it with `c`, `2`, `zroot-luks`, and write with `w`, `y`. Then run
  `udevadm settle` and repeat this Check. Debian 13 is expected to ship gdisk 1.0.10, so this is rarely needed.
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
- The container is opened as `luks-<UUID>`, the name ZFSBootMenu gives volumes it is told to open by UUID. Using the
  same name everywhere keeps the device path recorded in the pool the same in ZFSBootMenu and in Debian.
- `--allow-discards --persistent` stores the TRIM flag **in the LUKS2 header**, so the container passes TRIM requests
  through wherever it is opened (needed for `autotrim=on`): in ZFSBootMenu, in Debian's initramfs and on a live
  system.

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
normal import by another host while the pool was not exported. ZFSBootMenu, Debian's initramfs and the running system
all import this pool, so they must agree. Write a fixed host ID now; it is copied into the new system later.

```
root # zgenhostid -f 0x00bab10c
```

**2. Option B: the passphrase file.** `zpool create` reads the passphrase from the file named by `keylocation`, and
the same file later lets Debian's initramfs load the key without asking. Avoid spaces at the start and end of the
passphrase: they are easy to forget at the prompt.

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
| `compatibility=openzfs-2.2-linux` | enables only the features OpenZFS 2.2 knows (see below) |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `relatime=on` | fewer access-time writes |
| `-m none` | the pool's own root dataset is not mounted |
| `-R /mnt` | everything the pool mounts during the installation appears below `/mnt` |
| `encryption=aes-256-gcm` (B) | likely the fastest cipher on modern x86_64 hardware |
| `keyformat=passphrase` (B) | lets ZFSBootMenu ask for the key |
| `keylocation=file:///etc/zfs/zroot.key` (B) | the file of step 2; also copied into Debian's initramfs |

With `compatibility=openzfs-2.2-linux`, live systems with ZFS 2.2 or later can import the pool. It can be relaxed
later: [Relaxing the pool compatibility](#relaxing-the-pool-compatibility).

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
| `zroot/ROOT/debian` | `mountpoint=/`, `canmount=noauto` | the boot environment: the whole system, including `/boot` |
| `zroot/home` | `mountpoint=/home` | user data, outside the boot environment |
| pool property `bootfs` | `zroot/ROOT/debian` | the boot environment ZFSBootMenu boots after the countdown |

- `canmount=noauto` is required on every file system with `mountpoint=/`, and it is not inherited: each boot
  environment you create later needs it set again.
- Keep `/usr`, `/etc` and `/var` in the boot environment: they change together with every package update. Add
  datasets only for user data (`/srv`, for example), if you want them.

> **Important**
> `/boot` is a directory of the root dataset, not a file system of its own and not the ESP. ZFSBootMenu reads
> Debian's kernel and initramfs from there, and in Options B and C the initramfs with the key file is then inside the
> encrypted pool. The ESP is mounted at `/boot/efi` and holds only ZFSBootMenu.

**Check:**

```
root # zfs list -o name,mountpoint,canmount -r "${POOL}"
root # zpool get -H -o value bootfs "${POOL}"
```

Three datasets below `zroot` with the mount points of the table, and `bootfs` is `zroot/ROOT/debian`.

### Setting the ZFSBootMenu properties

Why: ZFSBootMenu reads the kernel command line for Debian from a ZFS property. It is set on `zroot/ROOT`, so every
future boot environment inherits it.

**All options.** Debian's initramfs needs nothing on the command line for any option: Option B finds the key file
through the dataset's `keylocation`, and Option C is configured in `/etc/crypttab` later.

```
root # zfs set org.zfsbootmenu:commandline="quiet" "${POOL}/ROOT"
```

Do not put `root=` into this property: ZFSBootMenu always adds the root argument itself. For Debian it adds
`root=zfs:zroot/ROOT/debian`, which OpenZFS's initramfs-tools scripts read; there is no need to set
`org.zfsbootmenu:rootprefix`. Do not add `spl.spl_hostid=` either; ZFSBootMenu sets it.

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

The first prints `quiet`; the second prints `zroot/ROOT/debian` for Option B and `-` for Options A and C.

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

If it refuses the passphrase, the file does not hold what you typed. Nothing is installed yet: run
`zpool destroy "${POOL}"`, write the file again and repeat [Creating the ZFS pool](#creating-the-zfs-pool) from
step 2.

**All options.**

```
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount "${POOL}/home"
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

`-N` imports without mounting; the root dataset is mounted by hand because it is `canmount=noauto`. The cache file
is copied into the new system later, where Debian's `zfs-import-cache` service and initramfs use it.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot. The
> ZFS tools inside the new system may not match the ZFS kernel module that the live system has loaded.

**Check:**

```
root # findmnt -R /mnt
root # zfs get -H -o value keystatus "${POOL}"
```

`/mnt` is `zroot/ROOT/debian` and `/mnt/home` is `zroot/home`. Option B: the key status is `available` (Options A
and C print `-`).

## Installing the base system

Why: `debootstrap` installs a minimal Debian 13 into the mounted boot environment.

Give the new system a `/run` in memory, then install:

```
root # mkdir -p /mnt/run
root # mount -t tmpfs tmpfs /mnt/run
root # mkdir -p /mnt/run/lock
root # debootstrap trixie /mnt
```

`debootstrap` downloads and unpacks the base packages; it ends with `I: Base system installed successfully.`

Write the package sources of the new system. `contrib` holds ZFS, `non-free-firmware` the CPU microcode and device
firmware:

```
root # cat > /mnt/etc/apt/sources.list <<EOF
deb http://deb.debian.org/debian trixie main contrib non-free-firmware
deb http://deb.debian.org/debian-security trixie-security main contrib non-free-firmware
deb http://deb.debian.org/debian trixie-updates main contrib non-free-firmware
EOF
```

**Check:**

```
root # cat /mnt/etc/debian_version
root # ls /mnt
```

The version starts with `13`, and `/mnt` holds a root file system (`bin`, `boot`, `etc`, `usr`, `var` and so on).

### Copying the host ID and the key file

Why: the new system must use the same host ID as the live system that created the pool, the pool's cache file, and
(Option B) the passphrase file.

```
root # mkdir -p /mnt/boot/efi
root # mount "${ESP_PART}" /mnt/boot/efi
root # cp /etc/hostid /mnt/etc/hostid
root # mkdir -p /mnt/etc/zfs
root # cp /etc/zfs/zpool.cache /mnt/etc/zfs/zpool.cache
root # cp /etc/resolv.conf /mnt/etc/resolv.conf
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

Why: the rest of the installation runs inside the new system, with the live system's devices, kernel interfaces and
EFI variables made visible to it.

```
root # mount --make-private --rbind /dev /mnt/dev
root # mount --make-private --rbind /proc /mnt/proc
root # mount --make-private --rbind /sys /mnt/sys
root # chroot /mnt /bin/bash --login
(chroot) root # source /root/install.env
(chroot) root # apt update
```

`--rbind` (not `--bind`) also brings the mounts below `/sys`, among them the EFI variables that `efibootmgr` needs.
Until the locales are configured in [Configuring the system](#configuring-the-system), `apt` and `perl` may print
warnings such as `perl: warning: Setting locale failed`; they are harmless.

If you leave the chroot and come back, run the `chroot` and `source` lines again.

**Check:**

```
(chroot) root # echo "${POOL} ${BE} ${DISK}"
(chroot) root # ls /sys/firmware/efi/efivars | head -n 3
```

The first prints `zroot debian /dev/nvme0n1`; the second lists EFI variables. `apt update` ended without errors.

## Kernel and ZFS

Why: Debian's kernel, and the ZFS module built for it with DKMS, make up the system ZFSBootMenu will boot.

Install the kernel and its headers first, so that DKMS can build the ZFS module for it, then ZFS with its
initramfs-tools scripts. `zfs-dkms` builds the module, which takes a few minutes, and shows the license note again;
press Enter.

```
(chroot) root # apt install --yes dpkg-dev linux-headers-amd64 linux-image-amd64
(chroot) root # apt install --yes zfs-dkms zfsutils-linux zfs-initramfs
(chroot) root # systemctl enable zfs.target zfs-import-cache zfs-mount zfs-import.target
(chroot) root # export KVER="$(ls /lib/modules)"
```

- Debian installs the kernel as `/boot/vmlinuz-<version>` and its initramfs as `/boot/initrd.img-<version>`, in
  `/boot` of the boot environment, where ZFSBootMenu looks for them. Nothing in this guide installs `grub` or the
  `systemd-boot` package; with them, kernels could end up on the ESP.
- The first initramfs, built with the kernel, cannot unlock anything yet;
  [Configuring the initramfs](#configuring-the-initramfs) builds it again.
- `zfsutils-linux` keeps the host ID you copied: it writes a new `/etc/hostid` only when the file is missing.
- `KVER` names the installed kernel version, used by the Checks below. If `ls /lib/modules` prints more than one
  version, set `KVER` to the newest by hand. It is not in `/root/install.env`: after re-entering the chroot, run the
  `export KVER=...` line again.

**Check:**

```
(chroot) root # echo "${KVER}"
(chroot) root # ls /boot
(chroot) root # ls -A /boot/efi
(chroot) root # modinfo -k "${KVER}" -F version zfs
(chroot) root # zfs version
(chroot) root # systemctl is-enabled zfs.target zfs-import-cache zfs-mount zfs-import.target
(chroot) root # od -A n -t x4 /etc/hostid
```

- `KVER` is one version, such as `6.12.NN+deb13-amd64`.
- `/boot` holds `vmlinuz-${KVER}`, `initrd.img-${KVER}`, `config-${KVER}`, `System.map-${KVER}` and the directory
  `efi`; `/boot/efi` is empty.
- `modinfo` prints the version of the ZFS module built for the new kernel (2.3 or later). If it prints an error, see
  [The ZFS module is missing for a kernel](#the-zfs-module-is-missing-for-a-kernel).
- `systemctl is-enabled` prints `enabled` four times, and the host ID is `00bab10c`.

## Option C only: creating the LUKS key file

Skip this section for Options A and B.

Why: Debian's initramfs must unlock the container again after `kexec`; a random key in a second key slot lets it do
so without asking.

Install cryptsetup and its initramfs-tools integration in the new system. The package installation may already print
cryptsetup warnings about the root device; they are explained in the next section.

```
(chroot) root # apt install --yes cryptsetup cryptsetup-initramfs
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

Why: after `kexec`, Debian's initramfs (initramfs-tools) must import the pool and, in Options B and C, unlock it with
the key file. In Options B and C the image then contains a key, so it must be readable by root only.

What Debian's initramfs must contain:

| | Option A | Option B | Option C |
|---|---|---|---|
| ZFS module and tools, `/etc/hostid`, `zpool.cache` | yes | yes | yes |
| `/etc/zfs/zroot.key` | | yes | |
| cryptsetup and the container's entry with its key file | | | yes |
| readable by root only | not needed | **required** | **required** |

`zfs-initramfs` already adds the ZFS module and tools, `/etc/hostid` and `/etc/zfs/zpool.cache` for every option.

**Option A.** Nothing to configure.

**Option B.** OpenZFS's initramfs-tools script loads the key of an encryption root whose `keylocation` is a file with
`zfs load-key`, from the file **inside the initramfs**; but its hook copies no key file into the image. This was
read in OpenZFS's sources and in Ubuntu's `zfs-linux` 2.3.1 package, which is derived from Debian's, not in Debian's
own package; the Check below shows the result on your system. (The older ZFSBootMenu guides for Debian say that the
key is copied automatically; OpenZFS's hook does not do that.) Add a hook of your own that copies it, and make the
image readable by root only:

**FILE** `/etc/initramfs-tools/hooks/zfs-key`

```
#!/bin/sh
# Copy the ZFS passphrase file into the initramfs. The initramfs lives in /boot
# inside the encrypted pool, so the file is never stored unencrypted.
PREREQ="zfs"
prereqs() { echo "$PREREQ"; }
case "$1" in
prereqs) prereqs; exit 0 ;;
esac
. /usr/share/initramfs-tools/hook-functions
copy_file key /etc/zfs/zroot.key
```

```
(chroot) root # chmod 0755 /etc/initramfs-tools/hooks/zfs-key
(chroot) root # echo "UMASK=0077" > /etc/initramfs-tools/conf.d/umask.conf
```

**Option C.** Debian's cryptsetup-initramfs unlocks, in the initramfs, the root device, resume devices and **every
device whose `/etc/crypttab` entry has the `initramfs` option**. It cannot see that the ZFS root is on the container,
hence the option. `KEYFILE_PATTERN` tells it which key files it may copy into the image, and `UMASK=0077` makes the
image readable by root only (the hook warns about a permissive umask otherwise).

```
(chroot) root # echo "luks-${LUKS_UUID} UUID=${LUKS_UUID} /etc/cryptsetup-keys.d/zroot.key luks,discard,initramfs" \
  >> /etc/crypttab
(chroot) root # echo 'KEYFILE_PATTERN="/etc/cryptsetup-keys.d/*.key"' >> /etc/cryptsetup-initramfs/conf-hook
(chroot) root # echo "UMASK=0077" > /etc/initramfs-tools/conf.d/umask.conf
```

| Field of the crypttab line | Value | Meaning |
|---|---|---|
| name | `luks-<UUID>` | the same mapping name ZFSBootMenu and the live system use |
| device | `UUID=<UUID>` | the LUKS container, by its LUKS UUID |
| key file | `/etc/cryptsetup-keys.d/zroot.key` | copied into the image (see below) |
| options | `luks,discard,initramfs` | LUKS; pass TRIM (already stored in the header); unlock in the initramfs |

The details of cryptsetup-initramfs in this section (the `initramfs` option, `KEYFILE_PATTERN`, the umask warning,
and the key's path inside the image, `/cryptroot/keyfiles/luks-<UUID>.key`) were read in Ubuntu's
cryptsetup-initramfs 2.7.5, which is derived from Debian's; the Check below shows them on your system, and
`/usr/share/doc/cryptsetup-initramfs/README.initramfs.gz` describes Debian's own.

`UMASK` may also go into `/etc/initramfs-tools/initramfs.conf`, as cryptsetup's documentation shows; `mkinitramfs`
reads the files in `/etc/initramfs-tools/conf.d/` as well.

**Optional, all options: CPU microcode.** ZFSBootMenu loads exactly one initramfs file per kernel. Debian's microcode
packages add the early microcode to that same `initrd.img` file, so it is loaded. Install the one for your CPU:

```
(chroot) root # apt install --yes intel-microcode
(chroot) root # apt install --yes amd64-microcode
```

(only one of the two lines).

**All options.** Rebuild the initramfs for every installed kernel:

```
(chroot) root # update-initramfs -c -k all
```

Option C: the cryptsetup hook prints `cryptsetup: ERROR: Couldn't resolve device zroot/ROOT/debian` and
`cryptsetup: WARNING: Couldn't determine root device`. They are expected: it does not understand a ZFS root, which is
why the crypttab entry carries the `initramfs` option. (These messages were read in Ubuntu's cryptsetup-initramfs
and appear in OpenZFS's guide for Debian 13; the exact wording may differ.) Any other cryptsetup warning,
especially about the key file or a permissive umask, is not expected.

**Check:**

```
(chroot) root # ls -l /boot/initrd.img-*
(chroot) root # lsinitramfs "/boot/initrd.img-${KVER}" | grep -E 'zfs\.ko|etc/hostid|zpool\.cache'
(chroot) root # lsinitramfs "/boot/initrd.img-${KVER}" | grep -E 'zroot\.key|cryptroot/|sbin/cryptsetup$'
(chroot) root # ls -A /boot/efi
```

| | Option A | Option B | Option C |
|---|---|---|---|
| `ls -l` | `-rw-r--r--` is fine | `-rw-------` | `-rw-------` |
| first `grep` | `zfs.ko` (or `zfs.ko.xz`), `etc/hostid`, `etc/zfs/zpool.cache` | the same | the same |
| second `grep` | nothing | `etc/zfs/zroot.key` | three lines (see below) |
| `/boot/efi` | empty | empty | empty |

Option C's second `grep` lists `cryptroot/crypttab`, `cryptroot/keyfiles/luks-<UUID>.key` and a path ending in
`sbin/cryptsetup`.

If an image for Option B or C is readable by others (`-rw-r--r--`), the `UMASK` line is missing; fix it and rebuild.
Never copy such an image out of the pool.

## Configuring the system

Why: the base configuration of any Debian system: locales, time zone, keyboard, host name, network, the ESP in
`/etc/fstab`, and accounts.

Locales, time zone and keyboard. Always enable `en_US.UTF-8` as well, because some programs expect it:

```
(chroot) root # apt install --yes locales console-setup keyboard-configuration systemd-timesyncd
(chroot) root # dpkg-reconfigure locales tzdata keyboard-configuration console-setup
```

Host name. Replace `HOSTNAME` in both places where it appears. The second command writes the whole of
`/etc/hosts`, whatever `debootstrap` left there:

```
(chroot) root # echo HOSTNAME > /etc/hostname
(chroot) root # cat > /etc/hosts <<EOF
127.0.0.1   localhost
127.0.1.1   HOSTNAME
::1         localhost ip6-localhost ip6-loopback
EOF
```

Network. The OpenZFS guide for Debian 13 configures a wired interface with ifupdown and DHCP; in Debian 13 ifupdown
uses `dhcpcd-base` as its DHCP client. Find the interface's name with `ip -br link` (on the live system or in the
chroot; for example `enp1s0`) and replace `NAME`:

```
(chroot) root # apt install --yes ifupdown dhcpcd-base
```

**FILE** `/etc/network/interfaces.d/NAME`

```
auto NAME
iface NAME inet dhcp
```

For Wi-Fi, a desktop or another network
setup, install it later from the running system (for example with `tasksel`, which installs NetworkManager with a
desktop). The firmware of many network and graphics devices is in `firmware-linux` (component `non-free-firmware`).

The ESP in `/etc/fstab`. Only the ESP goes there; ZFS mounts its datasets itself:

```
(chroot) root # ESP_UUID="$(blkid -s UUID -o value "${ESP_PART}")"
(chroot) root # echo "UUID=${ESP_UUID}  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

The root password, and a user for daily work (replace `USERNAME`) who may use `sudo`:

```
(chroot) root # passwd
(chroot) root # apt install --yes sudo
(chroot) root # adduser USERNAME
(chroot) root # usermod -aG sudo USERNAME
```

**Check:**

```
(chroot) root # cat /etc/hostname /etc/hosts /etc/fstab
(chroot) root # locale -a | grep -i en_us
(chroot) root # findmnt --verify --tab-file /etc/fstab
(chroot) root # id USERNAME
```

`/etc/hostname` and the `127.0.1.1` line of `/etc/hosts` show your host name, `/etc/fstab` has the one ESP line,
`en_US.utf8` is listed, `findmnt --verify` reports no errors for that line, and the user is in the group `sudo`.

## Installing ZFSBootMenu

Everything in this section runs **inside the chroot**, as root. ZFSBootMenu is built from this repository, because
upstream ZFSBootMenu cannot unlock LUKS volumes; for the same steps in all three options, Options A and B use this
build too.

### Dependencies

Why: the build needs a Rust compiler and the tools that go into ZFSBootMenu's image.

`kexec-tools` asks whether it should handle reboots. Answer *No*; this line answers it in advance:

```
(chroot) root # echo "kexec-tools kexec-tools/load_kexec boolean false" | debconf-set-selections
```

Then install the packages:

```
(chroot) root # apt install --yes git curl ca-certificates build-essential dracut-core systemd-boot-efi \
  kexec-tools fzf mbuffer bsdextrautils less efibootmgr binutils procps kmod udev util-linux \
  xkb-data libx11-data
```

| Package | Why |
|---|---|
| `git`, `curl`, `ca-certificates` | fetch the source and the Rust installer |
| `build-essential` | `make`, and the C compiler and linker Rust needs |
| `dracut-core` | builds ZFSBootMenu's image |
| `systemd-boot-efi` | the EFI stub `/usr/lib/systemd/boot/efi/linuxx64.efi.stub` |
| `kexec-tools` | starts Debian's kernel from ZFSBootMenu |
| `fzf` | the menu of the default (shell) runtime |
| `mbuffer`, `bsdextrautils` (`column`), `less` | optional tools of the image; without them the build warns |
| `efibootmgr` | the firmware boot entries |
| `binutils` | `strip` for `make install`, `strings` for `generate-zbm`, `objcopy` for a Check |
| `procps`, `kmod`, `udev`, `util-linux` | tools of the image; a Debian system has them already |
| `xkb-data`, `libx11-data` | keyboard layouts for the optional Rust runtime |

> **Warning**
> Install `dracut-core`, never the package `dracut`. `dracut` replaces initramfs-tools as Debian's initramfs
> generator (it conflicts with `initramfs-tools`), and apt would then remove `initramfs-tools`, `zfs-initramfs` and
> (Option C) `cryptsetup-initramfs`. `dracut-core` installs the `dracut` program and its modules, but no script in
> `/etc/kernel/`, so kernel updates keep using initramfs-tools. If apt proposes to remove `initramfs-tools`, answer
> `n`. This was read in Ubuntu's dracut 106 packaging, which is derived from Debian's, and Debian's own
> `README.Debian` of `dracut-core` describes using it beside initramfs-tools; the Check below confirms it on your
> system.

**Option C.** `cryptsetup` is already installed; it is what puts LUKS support into ZFSBootMenu's image. A dracut build
without it leaves LUKS out without a message.

**All options.** Rust. This repository needs `rustc` 1.88 or later; Debian 13's `rustc` is 1.85. Install `rustup`,
the Rust project's installer, for root. `--default-toolchain none` installs no compiler yet: the repository's
`rust-toolchain.toml` names the version to use (Rust 1.97.0 with `rustfmt` and `clippy`), and `rustup` downloads it,
several hundred MB, on first use in the clone.

```
(chroot) root # curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /root/rustup-init.sh
(chroot) root # sh /root/rustup-init.sh -y --profile minimal --default-toolchain none
(chroot) root # . /root/.cargo/env
```

`rustup` installs into `/root/.cargo` and `/root/.rustup`, and adds `/root/.cargo/bin` to root's `PATH` in later
login shells. `make install` builds again as the user who runs it, so root needs Rust on its `PATH`; in a new shell,
run `. /root/.cargo/env` if `cargo` is not found.

> **Note**
> `trixie-backports` has carried `rustc` 1.88 and later (1.88 and 1.89 were seen in search results; this guide
> could not open Debian's package index to see today's version). If `apt-cache policy rustc`, after adding
> `trixie-backports` to the sources, shows 1.88 or later, `apt install -t trixie-backports cargo rustc` works instead
> of `rustup`. Without `rustup`, `rust-toolchain.toml` is ignored and the build uses the backports compiler, which
> the project does not test.

**Check:**

```
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # dpkg -l initramfs-tools zfs-initramfs | grep '^ii'
(chroot) root # dpkg-query -W dracut-core
(chroot) root # dpkg -L dracut-core | grep '^/etc/kernel/'
(chroot) root # ls /etc/kernel/postinst.d/
(chroot) root # grep LOAD_KEXEC /etc/default/kexec
(chroot) root # rustup --version
(chroot) root # command -v dracut lsinitrd kexec fzf efibootmgr strings
```

- The stub exists, and `initramfs-tools` and `zfs-initramfs` are still installed (`ii`).
- `dpkg-query` prints the version of `dracut-core`, `106-...` on Debian 13. The project tests with dracut 112; see
  [What the project has tested](#what-the-project-has-tested).
- `dpkg -L ... | grep` prints nothing. If it prints a path, do not continue: that script would rebuild Debian's
  initramfs with dracut after a kernel update, without the key file. Remove `dracut-core` (`apt remove dracut-core`)
  and report it.
- `/etc/kernel/postinst.d/` lists `dkms` and `initramfs-tools`, and perhaps scripts of other packages, but none
  named after `dracut` or `systemd-boot`.
- `LOAD_KEXEC=false`, `rustup` prints its version, and every program is found. Option C:
  `command -v cryptsetup dmsetup udevadm` finds all three.

### Building and installing

Why: `make` builds the programs `zbm`, `brush`, `generate-zbm` and `zbm-kcl`; `make install` installs them with the
dracut module and the configuration.

```
(chroot) root # git clone --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2 /usr/src/zbm
(chroot) root # cd /usr/src/zbm
(chroot) root # cargo --version && rustc --version
(chroot) root # make -C /usr/src/zbm
(chroot) root # make -C /usr/src/zbm install
```

> **Warning**
> The repository's default branch, `master`, holds the upstream layout without the LUKS code. Always use
> `claude/zbm-refactor`. Do not run `make core dracut`, which older guides show: it installs neither `zbm` nor
> `generate-zbm`.

- `cargo --version` inside the clone makes `rustup` download the toolchain named in `rust-toolchain.toml` (this needs
  the network once). The Rust crates themselves are in the clone (`vendor/`), so the build needs no network.
- `make` runs `cargo build --release --locked`; it takes several minutes.
- `make install` puts `generate-zbm` and `zbm-kcl` in `/usr/bin`; `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
  the shell runtime in `/usr/share/zfsbootmenu`; the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in
  `/usr/lib/dracut/modules.d`; the mkinitcpio hooks in `/usr/lib/initcpio`; `config.yaml`, `mkinitcpio.conf` and
  `dracut.conf.d/` in `/etc/zfsbootmenu`; manual pages; and examples in `/usr/share/examples/zfsbootmenu`.
- `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files it
  installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files with other names there are kept. This guide therefore writes
  `config.yaml` after `make install`, and keeps its own dracut settings in files of its own.

**Check:**

```
(chroot) root # git -C /usr/src/zbm branch --show-current
(chroot) root # rustc --version
(chroot) root # generate-zbm --showver
(chroot) root # ls -l /usr/bin/generate-zbm /usr/bin/zbm-kcl /usr/libexec/zfsbootmenu/
(chroot) root # ls /usr/lib/dracut/modules.d/ | grep zfsbootmenu
```

The branch is `claude/zbm-refactor`, `rustc` is 1.88 or later, `generate-zbm` prints a version, `zbm` and `brush`
are in `/usr/libexec/zfsbootmenu`, and both dracut modules are listed.

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

`/etc/zfsbootmenu/dracut.conf.d` is never the system's `/etc/dracut.conf.d`. Debian's kernel name would give the
same image names as `Kernel.Prefix`; the key keeps them so.

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

Keyboard: with the shell runtime the passphrase prompt uses the US layout. With the Rust runtime, press **Ctrl-K** at
the prompt to choose a layout. Another way is to add `zbm.console=drm` to `CommandLine`: the console on the display
then types with the image's first layout. That console needs the kernel's `evdev` module and a DRM driver in the
image (`simpledrm` on the firmware's framebuffer is enough), for example with
`echo 'add_drivers+=" evdev "' > /etc/zfsbootmenu/dracut.conf.d/console.conf`. The installed `omit-drivers.conf`
leaves out `drm` and the common GPU drivers, and this guide did not check whether Debian's kernel provides
`simpledrm` to the image; when the console cannot use the display, ZFSBootMenu goes on with the kernel console.
The image gets the layouts of `zfsbootmenu_keyboard_layouts`; when it is
unset, the layout of `XKBLAYOUT` in `/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`. Debian keeps its layout in
`/etc/default/keyboard`, and this guide could not confirm that a Debian system has `/etc/vconsole.conf`
(`ls /etc/vconsole.conf` tells). To be sure, name your layout, for example:

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
  (`/boot/vmlinuz-${KVER}`). `--debug` prints every command.
- The first run writes `/boot/efi/EFI/zbm/vmlinuz.EFI`; the second renames it to `vmlinuz-backup.EFI` and writes a
  new one.

| Message | Cause |
|---|---|
| `zbm, which plans every ZFSBootMenu image, is not at /usr/libexec/zfsbootmenu/zbm` | `make install` was not run |
| `failed to install essential executable '<name>'` | a required program is missing (1) |
| `Unable to find UEFI stub loader at default locations` | `systemd-boot-efi` is not installed |
| `ManageImages not enabled, no action taken` | `config.yaml` was overwritten by `make install`; write it again |
| `Unable to locate libgcc_s.so` | the GCC runtime library is missing (`libgcc-s1`) |
| `SPL (...) and system (...) hostids do not match!` | a warning only; the build goes on (2) |
| any other dracut error | possibly dracut 106 (3) |

1. Often `kexec`, or `fzf` for the shell runtime.
2. The SPL module's `spl_hostid` parameter differs from `/etc/hostid` (the check is skipped while the parameter is
   0). The image takes `/etc/hostid`, which must be `00bab10c` (`od -A n -t x4 /etc/hostid`).
3. The project tests with dracut 112, not with Debian 13's 106. Do not boot an image from a build that printed
   errors. On an installed system, keep the old image on the ESP: do not run `generate-zbm` again until the error
   is solved, because each run replaces the backup. Report the error with the output of `generate-zbm --debug`.

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
| `zbm-kcl` prints | `quiet loglevel=0 zbm.luks=off` | the same | `quiet loglevel=0 zbm.luks=UUID=<UUID>` |
| first `grep` lists | `etc/hostid`, `zfs.ko` (or `zfs.ko.xz`), `nvme` | the same | the same and `bin/cryptsetup` |
| second `grep` prints | nothing | **nothing** | **nothing** |

With the Rust runtime, the `zbm-kcl` line ends in `init_on_free=1`.

The second `grep` must print nothing: no key file and no crypttab may be in ZFSBootMenu's image. If it prints a line,
do not boot this image; find out why first (a key file named in `/etc/dracut.conf` or in
`/usr/lib/dracut/dracut.conf.d/`). If `cryptsetup` is listed for Options A or B, that is fine: it is in the image
whenever the package is installed, and `zbm.luks=off` keeps it unused. The `nvme` line lists the driver module;
whether Debian's kernel builds the NVMe driver as a module was not confirmed for this guide, and if it is built in,
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
| 2 | `Booting zroot/ROOT/debian in 10 seconds` (1) | the same | the same |
| 3 | | the screen clears; `Enter passphrase for 'zroot':`. Type the ZFS passphrase | |
| 4 | the screen goes dark for a moment while `kexec` starts Debian's kernel | the same | the same |
| 5 | Debian starts | Debian starts **without** asking again | Debian starts **without** asking again |

1. With `[RETURN] boot now` and `[ESCAPE] boot menu` below it.

- **Enter** boots at once; **Escape** during the countdown opens the menu, which lists `zroot/ROOT/debian`. In
  Option B the menu asks for the passphrase before it lists the boot environment.
- Option C: an empty passphrase skips the volume; after three failed attempts ZFSBootMenu offers to try again or
  skip.
- Option B: `zfs load-key` allows three attempts. After three wrong passphrases ZFSBootMenu opens its emergency shell
  with `unable to load key for zroot/ROOT/debian`. Type `exit` there; the menu then asks again.
- With the Rust runtime the same questions appear as dialogs (`Unlock <volume>` for Option C, a ZFS key dialog for
  Option B), and the countdown reads `Booting <environment> in <N> s. Press any key to choose another.`

If Debian asks for a passphrase a second time, or stops in the initramfs shell (`(initramfs)` prompt), see
[The passphrase is asked for a second time, or the system stops in its initramfs][second-unlock].

[second-unlock]: #the-passphrase-is-asked-for-a-second-time-or-the-system-stops-in-its-initramfs

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
root # zfs get -o name,property,value encryption,encryptionroot,keylocation,keystatus zroot zroot/ROOT/debian
```

Both datasets show `aes-256-gcm`, the encryption root `zroot` and the key status `available`. Only the encryption
root has a key location: `zroot` shows `file:///etc/zfs/zroot.key`, and `zroot/ROOT/debian` shows `none`.

**Option C:**

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
| encryption | | `aes-256-gcm`, `file:///etc/zfs/zroot.key`, `available` | `LUKS2`, `4096`, `discards` |
| asked for the passphrase | never | once, by ZFSBootMenu | once, by ZFSBootMenu |
| `systemctl --failed` | `0 loaded units listed` | the same | the same |

`BootCurrent` names the entry the firmware used. If it is the number of the `ZFSBootMenu` entry, the firmware honours
entries and `\EFI\BOOT\BOOTX64.EFI` is only a reserve. If it is another number, the firmware started the copy; the
post-hook `50-bootx64.sh` of [Creating the firmware boot entries](#creating-the-firmware-boot-entries) keeps it
current, so check after each `generate-zbm` that it ran (see
[Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image)).

Once the system has booted, do these three things:

1. Install the kernel hook that rebuilds ZFSBootMenu's image after kernel updates:
   [Regenerating the ZFSBootMenu image](#regenerating-the-zfsbootmenu-image).
2. **Option C:** make the first LUKS header backup:
   [Option C only: backing up the LUKS header](#option-c-only-backing-up-the-luks-header).
3. Keep the live medium until the system has booted several times, including once after a kernel update.

## Usage

### Updating the system

Take a snapshot of the boot environment before large updates; [Boot environments, snapshots and
rollback](#boot-environments-snapshots-and-rollback) shows how to go back:

```
root # zfs snapshot zroot/ROOT/debian@pre-update-$(date +%Y%m%d)
root # apt update
root # apt full-upgrade
```

What happens on its own, and what you must check:

- A new **kernel** package runs the scripts in `/etc/kernel/postinst.d/`: `dkms` builds the ZFS module for the new
  kernel when its headers are installed (`linux-headers-amd64` keeps them coming), `initramfs-tools` builds its
  initramfs with the configuration of [Configuring the initramfs](#configuring-the-initramfs), key file included.
  When the headers are configured after the kernel, `dkms` builds the module from
  `/etc/kernel/header_postinst.d/` instead. `zz-generate-zbm` (once installed, in both directories) rebuilds
  ZFSBootMenu's image from whichever directory runs once the module exists.
- A new **zfs-dkms** rebuilds the module for the installed kernels and rebuilds the initramfs images. Run
  `generate-zbm` afterwards yourself (next section). This is not urgent: the old ZFSBootMenu image keeps importing
  the pool, because the `compatibility` property keeps new pool features off until you change it.
- A new **cryptsetup** rebuilds the initramfs through its hooks. Run `generate-zbm` afterwards in Option C.

This guide could not confirm that apt always configures a kernel's headers before the kernel itself, so after every
update that brought a kernel, check before rebooting that each kernel in `/boot` has a ZFS module and an initramfs
with it:

```
root # for k in /boot/vmlinuz-*; do v="${k#/boot/vmlinuz-}"; echo "${v}: $(modinfo -k "${v}" -F version zfs)"; done
root # lsinitramfs "/boot/initrd.img-$(ls /lib/modules | sort -V | tail -n 1)" | grep -E 'zfs\.ko'
root # ls -l /boot/initrd.img-*
```

Every kernel shows a ZFS version, the newest initramfs contains `zfs.ko`, and for Options B and C every initramfs is
`-rw-------`. If a kernel has no module, see
[The ZFS module is missing for a kernel](#the-zfs-module-is-missing-for-a-kernel) before you reboot.

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

**The kernel hook.** Debian runs the programs in `/etc/kernel/postinst.d/` in name order after a kernel package is
installed, and those in `/etc/kernel/header_postinst.d/` after a headers package is installed, each with the kernel
version as the first argument. In both, `dkms` runs before a name starting with `zz-`. The same hook goes into both
directories, because this guide could not confirm in which order apt configures a kernel and its headers. This hook
is this guide's own; the repository has none for Debian, and it has not been run by the project.

**FILE** `/etc/kernel/postinst.d/zz-generate-zbm`

```
#!/bin/sh
# Rebuild the ZFSBootMenu image with the kernel that was just installed.
# Installed in /etc/kernel/postinst.d and /etc/kernel/header_postinst.d;
# Debian passes the kernel version as the first argument to both.
version="$1"
stamp="/run/zz-generate-zbm-${version}"
[ -x /usr/bin/generate-zbm ] || exit 0
# The headers may come before the kernel: wait for the kernel's own run.
[ -e "/boot/vmlinuz-${version}" ] || exit 0
# Build once per kernel and boot, although the hook is installed twice.
[ -e "${stamp}" ] && exit 0
if ! modinfo -k "${version}" zfs >/dev/null 2>&1; then
	echo "zz-generate-zbm: no ZFS module for ${version} yet; run generate-zbm once it exists" >&2
	exit 0
fi
if generate-zbm --kver "${version}"; then
	touch "${stamp}"
else
	echo "zz-generate-zbm: generate-zbm failed; the old image is unchanged" >&2
fi
exit 0
```

```
root # chmod 0755 /etc/kernel/postinst.d/zz-generate-zbm
root # mkdir -p /etc/kernel/header_postinst.d
root # cp -p /etc/kernel/postinst.d/zz-generate-zbm /etc/kernel/header_postinst.d/zz-generate-zbm
```

The hook never fails the kernel installation; it prints a message instead. It builds with the kernel just installed,
also when you reinstall an older kernel, and only once per kernel until the next reboot (the mark in `/run`), so the
second copy does not overwrite the backup image with a second build. After a `zfs-dkms` or `cryptsetup` update, run
`generate-zbm` by hand. To see which of these packages an update changed:

```
root # grep -E ' (install|upgrade) (linux-image|zfs|cryptsetup)' /var/log/dpkg.log | tail
```

**Check** the hook by running it as Debian would:

```
root # ls /etc/kernel/postinst.d/ /etc/kernel/header_postinst.d/
root # rm -f "/run/zz-generate-zbm-$(uname -r)"
root # /etc/kernel/postinst.d/zz-generate-zbm "$(uname -r)"
root # /etc/kernel/header_postinst.d/zz-generate-zbm "$(uname -r)"
root # ls -l --time-style=full-iso /boot/efi/EFI/zbm/
```

- `postinst.d` lists `dkms`, `initramfs-tools` and `zz-generate-zbm` (others may be there too, but none from the
  `dracut` or `systemd-boot` packages, which would build or copy kernels differently).
- `header_postinst.d` lists `dkms` and `zz-generate-zbm`. If it has no `dkms`, the hook there does no harm, but
  this guide's assumption about Debian's `dkms` package is wrong; the after-update Check above still catches a
  missing module.
- The first run of the hook builds an image; the second prints nothing and builds nothing. `vmlinuz.EFI` has the
  time of the first run.

### Boot environments, snapshots and rollback

A boot environment is a file system with `mountpoint=/` that holds a kernel and an initramfs in `/boot`. Snapshots of
it are taken on the running system (`zfs snapshot zroot/ROOT/debian@NAME`) or in ZFSBootMenu.

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
| **MOD+X** | clone and promote | a new boot environment that keeps the snapshots (1) |
| **Enter** | duplicate | an independent copy (`zfs send` and `zfs recv`) (2) |
| **MOD+N** | new snapshot | of the selected boot environment |
| **MOD+D** | diff | between a snapshot and the file system, or two snapshots chosen with **Tab** |
| **MOD+J** | chroot | into the snapshot, read-only |

1. The old file system can be destroyed afterwards.
2. It uses as much space as the snapshot refers to.

**Rust runtime.** Press **2** or **s** for the snapshots of the selected environment, then **r** to roll back; a
review page shows what is lost, and the change is made once the target's name is typed. **?** lists the keys of
every view.

Example: going back after a bad update.

1. Before the update, on the running system: `zfs snapshot zroot/ROOT/debian@pre-update`.
2. After a bad update, reboot, press **Escape** in the countdown, select `zroot/ROOT/debian`, press **MOD+S**, select
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
  snapshot's initramfs holds the old one, and its system stops in the initramfs shell.

`zbm-kcl zroot/ROOT` shows, and `zbm-kcl -a ARGUMENT zroot/ROOT` changes, the `org.zfsbootmenu:commandline` property
that every boot environment inherits; see `man zbm-kcl`. Without a dataset name `zbm-kcl` acts on the root file system,
`zroot/ROOT/debian`, and a change there sets a property of its own on that boot environment only.

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
> Rebuild Debian's initramfs now. It holds a copy of the old file, and OpenZFS's initramfs script does not fall back
> to a prompt for a `file://` key location: with the old file the next boot stops in the initramfs shell.

```
root # update-initramfs -u -k all
root # ls -l /boot/initrd.img-*
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

A scrub reads and checks every block; a full TRIM complements `autotrim`. Debian's `zfsutils-linux` ships
`/etc/cron.d/zfsutils-linux`, which trims on the first and scrubs on the second Sunday of each month, but only runs
where a cron daemon is installed (`apt install cron`). The same package ships the systemd timers
`zfs-scrub-monthly@.timer` and `zfs-trim-monthly@.timer`; use one of the two ways, not both:

```
root # cat /etc/cron.d/zfsutils-linux
root # systemctl list-unit-files 'zfs-*monthly@.timer'
```

These file names are from the Debian packaging that Ubuntu's `zfs-linux` package is derived from; check them on your
system with the two commands above. The cron scripts honour the properties `org.debian:periodic-scrub` and
`org.debian:periodic-trim` on the pool's root dataset (`auto`, `enable` or `disable`; by default every healthy pool
is scrubbed, and a pool whose devices are all on NVMe SSDs, also through LUKS, is trimmed).

Option C: TRIM reaches the SSD because the flag is in the LUKS header; `lsblk -D` shows non-zero discard sizes for
`luks-<UUID>`. The SSD's own health: `apt install nvme-cli`, then `nvme smart-log /dev/nvme0`.

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
- If `cargo` is not found, run `. /root/.cargo/env`. A new `rust-toolchain.toml` makes `rustup` download another
  toolchain on first use.

**Check:** `generate-zbm --showver` prints the new version. Boot once and confirm it works before relying on it; the
backup entry still boots the previous image.

### Relaxing the pool compatibility

A step most readers can skip. The pool was created with `compatibility=openzfs-2.2-linux`. Debian 13's OpenZFS 2.3
knows newer features; to enable them, first rebuild ZFSBootMenu's image, then raise the set:

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
- OpenZFS 2.4 from `trixie-backports` (2.4.4 at the time of writing, from search results;
  `apt-cache policy zfsutils-linux` after adding `trixie-backports` shows it) brings the set `openzfs-2.4-linux`.
  A pool with those features can no longer be imported by the Debian 13 live image's OpenZFS 2.3: keep a rescue
  medium with OpenZFS 2.4 first.

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
`apt install cryptsetup` in the chroot, run `generate-zbm`, and repeat the Check of
[Building and checking the image](#building-and-checking-the-image).

### The passphrase is asked for a second time, or the system stops in its initramfs

After ZFSBootMenu started Debian's kernel, Debian's initramfs could not use the key file. With initramfs-tools:

- **Option B**: `zfs load-key` fails and the boot stops in the initramfs shell (`(initramfs)` prompt); OpenZFS's
  script does not ask for the passphrase when `keylocation` is a file.
- **Option C**: cryptsetup's initramfs script tries a key file only once (`maximum number of tries exceeded`), then
  the pool cannot be imported and the boot stops in the initramfs shell. It does not fall back to a passphrase
  prompt for an entry with a key file. (This was read in Ubuntu's cryptsetup-initramfs, which is derived from
  Debian's, and matches a user report on a Debian-based system; it was not seen on Debian 13.)

In the initramfs shell, Option B can try `zfs load-key -L prompt zroot` and Option C
`cryptsetup open /dev/disk/by-partlabel/zroot-luks luks-<UUID>`, then `exit`. Whether Debian's initramfs then goes on
booting was not checked for this guide; the live medium (below) always works. Then, on the running system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitramfs "/boot/initrd.img-$(uname -r)" | grep -E 'zroot\.key|cryptroot/'
```

Causes, in order of likelihood:

- the key file is not in the initramfs: Option B, the hook `/etc/initramfs-tools/hooks/zfs-key` is missing or not
  executable; Option C, the crypttab line lacks `initramfs`, or `KEYFILE_PATTERN` does not match the key file's path.
  Fix it and run `update-initramfs -u -k all`;
- Option C: the crypttab line carries another UUID than `cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks`;
- Option B: the passphrase was changed without rebuilding the initramfs
  ([Changing the passphrase](#changing-the-passphrase));
- an old snapshot or clone was booted, whose initramfs holds an older key.

If ZFSBootMenu **itself** also asks for another LUKS volume in Option C (for example one on a USB disk),
`zbm.luks=` is missing from its command line, and it offers every LUKS volume it finds. Check with
`zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI`.

### ZFSBootMenu lists no kernels, or no boot environment

The shell runtime shows `No boot environments with kernels found` and opens its emergency shell. ZFSBootMenu looks
only in `/boot` of each boot environment. In the recovery shell (Option B: `zfs load-key -L prompt zroot` first;
the key file is not in ZFSBootMenu's image, so a plain `zfs load-key` fails; `Key already loaded` means it was
loaded):

```
root # zfs list -o name,mountpoint,canmount
root # mount_zfs zroot/ROOT/debian
root # ls -l MOUNTPOINT/boot
```

`mount_zfs` prints the mount point to use for `MOUNTPOINT`. Causes:

- `/boot` in the dataset is empty: the kernels were installed onto the ESP or another partition mounted at `/boot`
  (a `grub` or `systemd-boot` package installed later can do that);
- a kernel has no initramfs beside it: Debian's pairs are `vmlinuz-<version>` and `initrd.img-<version>`;
- the dataset does not have `mountpoint=/`, or has `org.zfsbootmenu:active=off`.

### The pool is refused: wrong host ID, or last used by another system

- In ZFSBootMenu the default import policy adopts the host ID of the pool when it cannot import it with its own, so
  this usually resolves itself. Otherwise, in the recovery shell: `zpool import -f -N zroot`, then `exit`.
- In Debian's initramfs shell: `zpool import -f -N zroot`, then `exit`.
- Then, on the running system: `hostid` must print `00bab10c` and `/etc/hostid` must exist; run
  `update-initramfs -u -k all` and `generate-zbm`. The usual cause is a live system that imported the pool and was
  shut down without `zpool export`. Always export the pool on a rescue medium.

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

`zfs-chroot` is the way to repair a boot environment or rebuild its initramfs from ZFSBootMenu. `zluks` unlocks,
lists and closes LUKS volumes; `cryptsetup` and `dmsetup` are in the image as well.

### Repairing the system from the live medium

Boot the Debian live image, become root and install ZFS as in [Installation media](#installation-media) (Option C:
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

**All options.** Give the live system the host ID of the installed system first, so that the pool is not
foreign to it:

```
root # zgenhostid -f 0x00bab10c
root # zpool import -N -R /mnt zroot
```

**Option B:**

```
root # zfs load-key -L prompt zroot
```

**All options.**

```
root # zfs mount zroot/ROOT/debian
root # zfs mount zroot/home
root # mount "${ESP_PART}" /mnt/boot/efi
root # mount -t tmpfs tmpfs /mnt/run
root # mount --make-private --rbind /dev /mnt/dev
root # mount --make-private --rbind /proc /mnt/proc
root # mount --make-private --rbind /sys /mnt/sys
root # chroot /mnt /bin/bash --login
```

If the import refuses because the pool was last used by another system, add `-f`:
`zpool import -f -N -R /mnt zroot`. The stored TRIM flag is applied by `cryptsetup open` without options.

Repair what is needed, then leave the chroot and finish as in
[Finalizing the installation](#finalizing-the-installation), without the snapshot: unmount, `zpool export zroot`, and
for Option C `cryptsetup close`. Always export the pool before shutting the live system down.

### The ZFS module is missing for a kernel

`modinfo -k VERSION zfs` prints `modinfo: ERROR: Module zfs not found.` for a kernel in `/boot`. The kernel's headers
were not installed, or not yet configured, when DKMS ran. Replace `VERSION` with the kernel version:

```
root # apt install --yes linux-headers-VERSION
root # dkms autoinstall -k VERSION
root # update-initramfs -u -k VERSION
root # generate-zbm
```

`dkms status` lists the ZFS module and the kernels it is installed for. If the build fails, the kernel may be newer
than the OpenZFS version supports: keep booting the older kernel (choose it with **MOD+K** in ZFSBootMenu) and
consider OpenZFS from `trixie-backports`.

### apt wants to remove initramfs-tools

The package `dracut` was requested instead of `dracut-core`. Answer `n`, and install `dracut-core`; see
[Dependencies](#dependencies).

## Security notes

**Option A.**

- Nothing is protected at rest. Anyone who has the SSD, or who boots the computer from another medium, can read and
  change everything.
- Anyone at the keyboard can open ZFSBootMenu's recovery shell or edit the kernel command line and become root.

**Option B.**

- ZFS native encryption leaves the pool's structure readable without the passphrase: dataset and snapshot names, the
  dataset hierarchy, properties (including `org.zfsbootmenu:commandline`), file sizes and holes, and deduplication
  tables. File and volume data, file attributes, ACLs, permission bits and directory listings are encrypted.
- `/etc/zfs/zroot.key` holds the **passphrase itself**, inside the encrypted pool and inside Debian's initramfs in
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
  ESP, an unencrypted partition, a backup. Snapshots and `zfs send` streams of the root dataset contain it too; treat
  them like the pool.
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
- Rust is installed with `rustup` from the Rust project, outside Debian's package management and its security
  updates; `rustup update` updates it.

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
- Debian: [Debian 13 release information](https://www.debian.org/releases/trixie/),
  [ZFS in the Debian wiki](https://wiki.debian.org/ZFS), `crypttab(5)`, `initramfs-tools(7)`, `initramfs.conf(5)`,
  and `README.initramfs` in `/usr/share/doc/cryptsetup-initramfs/`
- OpenZFS: [Debian Trixie Root on ZFS][openzfs-trixie] (with GRUB instead of ZFSBootMenu), and the OpenZFS
  documentation (<https://openzfs.github.io/openzfs-docs/>)
- `gdisk(8)`, `cryptsetup(8)`, `debootstrap(8)`, and [rustup](https://rustup.rs/)

[openzfs-trixie]: https://openzfs.github.io/openzfs-docs/Getting%20Started/Debian/Debian%20Trixie%20Root%20on%20ZFS.html
