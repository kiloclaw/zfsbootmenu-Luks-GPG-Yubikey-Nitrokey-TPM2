# Gentoo Linux on a LUKS-encrypted ZFS root on NVMe (systemd, no crypttab)

This article describes how to install Gentoo Linux with the **systemd** profile on a **single NVMe SSD**. The SSD holds
a ZFS root pool inside a **LUKS2** container. The system boots with the ZFSBootMenu build from this repository, which
can unlock LUKS volumes.

You type the LUKS passphrase **once**, in ZFSBootMenu. Gentoo's initramfs then unlocks the container again with a
**key file** stored inside the encrypted pool. **No crypttab is used**: ZFSBootMenu and Gentoo's initramfs are both
told what to unlock through kernel command-line parameters.

> **Important**
> This guide erases the NVMe SSD completely. Back up all data on it before starting.

## Contents

- [Overview](#overview)
  - [Boot process](#boot-process)
  - [Unlocking without crypttab](#unlocking-without-crypttab)
  - [Key slots](#key-slots)
- [Requirements](#requirements)
  - [Hardware](#hardware)
  - [Installation media](#installation-media)
  - [Conventions](#conventions)
- [Preparing the NVMe SSD](#preparing-the-nvme-ssd)
  - [Identifying the SSD](#identifying-the-ssd)
  - [Setting variables](#setting-variables)
  - [Optional: switching to 4 KiB sectors](#optional-switching-to-4-kib-sectors)
  - [Erasing and partitioning](#erasing-and-partitioning)
  - [Creating the LUKS container](#creating-the-luks-container)
  - [Creating the ZFS pool](#creating-the-zfs-pool)
- [Installing the stage file](#installing-the-stage-file)
- [Installing the base system](#installing-the-base-system)
  - [Chrooting](#chrooting)
  - [Configuring Portage](#configuring-portage)
  - [Localization and host name](#localization-and-host-name)
- [Kernel and ZFS](#kernel-and-zfs)
- [Creating the LUKS key file](#creating-the-luks-key-file)
- [Configuring the initramfs](#configuring-the-initramfs)
- [Configuring the system](#configuring-the-system)
- [Installing ZFSBootMenu](#installing-zfsbootmenu)
- [Optional: unlocking with a key file on a USB stick](#optional-unlocking-with-a-key-file-on-a-usb-stick)
- [Finalizing the installation](#finalizing-the-installation)
- [Usage](#usage)
- [Troubleshooting](#troubleshooting)
- [See also](#see-also)
- [External resources](#external-resources)

## Overview

### Boot process

1. The UEFI firmware starts **ZFSBootMenu** from the EFI system partition (ESP) on the SSD.
2. ZFSBootMenu reads `zbm.luks=` from its kernel command line, asks for the **LUKS passphrase**, unlocks the container
   and imports the pool.
3. The boot environment is selected (or the countdown expires). ZFSBootMenu closes the container, which wipes the
   volume key from memory, and starts Gentoo's kernel with `kexec`.
4. Gentoo's initramfs reads `luks.uuid=` and `luks.key=` from its kernel command line and unlocks the container with
   the **key file**, without asking.
5. The initramfs imports the pool, mounts the root file system and starts systemd.

### Unlocking without crypttab

| Stage | What to unlock | How | Configured in |
|---|---|---|---|
| ZFSBootMenu | `zbm.luks=UUID=<LUKS UUID>` | passphrase prompt (or a USB key file, optional) | `Kernel.CommandLine` in `/etc/zfsbootmenu/config.yaml` |
| Gentoo initramfs | `luks.uuid=<LUKS UUID>` | `luks.key=<LUKS UUID>=/etc/cryptsetup-keys.d/zroot.key` | ZFS property `org.zfsbootmenu:commandline` on `zroot/ROOT` |

`luks.crypttab=no` makes systemd ignore `/etc/crypttab` in the initramfs and on the running system, and no
`/etc/zfsbootmenu/crypttab` is created. TRIM and the NVMe performance flags are stored **in the LUKS header**, so no
crypttab options are needed for them either. The parameters are described in
[systemd-cryptsetup-generator(8)](https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html)
and `man 7 zfsbootmenu-luks`.

### Key slots

| Key | Stored where | Used by |
|---|---|---|
| Passphrase | your memory | ZFSBootMenu at every boot; recovery from a live system |
| Key file | `/etc/cryptsetup-keys.d/zroot.key`, inside the encrypted pool, copied into Gentoo's initramfs | Gentoo's initramfs, after `kexec` |
| USB key file (optional) | a USB stick | ZFSBootMenu, instead of typing the passphrase |

The key file in Gentoo's initramfs is safe because both the file and the initramfs live inside the encrypted pool
(`/etc` and `/boot`). They cannot be read before the passphrase has unlocked the container. ZFSBootMenu itself lives
on the unencrypted ESP, so it never contains a plain key file.

## Requirements

### Hardware

- An amd64 computer with an **NVMe SSD** that boots in **UEFI** mode.
- **Secure Boot disabled** in the firmware; the ZFSBootMenu image built here is not signed.
- An internet connection.
- Optional: a small USB stick for [unlocking with a key file on a USB stick](#optional-unlocking-with-a-key-file-on-a-usb-stick).

### Installation media

The live system must provide **ZFS** (2.2 or later) and **cryptsetup**. The Gentoo minimal installation CD does not
include ZFS. Suitable choices:

- the Ubuntu Desktop live ISO, 24.04 or later (*Try Ubuntu*, open a terminal and run `sudo -i`);
- a Gentoo LiveGUI image, if `zfs version` works on it;
- any other UEFI live system with ZFS and cryptsetup.

Install the NVMe tools on the live system as well. On Ubuntu:

```
root # apt install nvme-cli
```

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run as root **inside the new Gentoo system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Preparing the NVMe SSD

Boot the live system in UEFI mode, connect to the network and open a root shell.

**Check:**

```
root # ls /sys/firmware/efi
root # modprobe zfs && zfs version
root # cryptsetup --version
root # nvme version
```

The first command must list files (otherwise the system booted in legacy BIOS mode); the others print versions.

### Identifying the SSD

List the NVMe controllers and namespaces:

```
root # nvme list
```

Each line shows a device such as `/dev/nvme0n1`, its model, serial number and size. `nvme0` is the controller and
`n1` its first namespace; partitions are named `/dev/nvme0n1p1`, `/dev/nvme0n1p2`, and so on.

Kernel names like `nvme0n1` can change when there are several NVMe SSDs. Use the persistent name instead:

```
root # ls -l /dev/disk/by-id/ | grep nvme | grep -v -- -part
```

Each SSD usually appears several times, for example:

```
nvme-Samsung_SSD_980_1TB_S64ANS0T123456X -> ../../nvme0n1
nvme-Samsung_SSD_980_1TB_S64ANS0T123456X_1 -> ../../nvme0n1
nvme-eui.002538b111b2c3d4 -> ../../nvme0n1
```

Use the **`nvme-MODEL_SERIAL`** name without a suffix. It is easier to recognize, and it matches the serial number
printed by `nvme list`. The `_1` variant (the namespace number) and the `eui.` or `nvme.` names point to the same
device.

### Setting variables

```
root # export DISK=/dev/disk/by-id/nvme-YOUR-MODEL_YOUR-SERIAL
root # export NVME_NS="$(readlink -f "${DISK}")"
root # export ESP_PART="${DISK}-part1"
root # export LUKS_PART="${DISK}-part2"
root # export POOL=zroot
root # export BE=gentoo
```

`NVME_NS` holds the kernel name, for example `/dev/nvme0n1`, which `nvme` commands need.

> **Tip**
> If the live system is rebooted or the terminal closed, set these variables again before continuing.

**Check:**

```
root # echo "${NVME_NS}"
root # lsblk -o NAME,SIZE,MODEL,SERIAL "${NVME_NS}"
```

The model and serial number match the SSD you intend to erase.

### Optional: switching to 4 KiB sectors

Many NVMe SSDs ship formatted with 512-byte sectors but also support 4096-byte sectors, which can be slightly faster.
Show the supported LBA formats:

```
root # nvme id-ns -H "${NVME_NS}" | grep -E '^LBA Format'
```

Example output:

```
LBA Format  0 : Metadata Size: 0   bytes - Data Size: 512 bytes - Relative Performance: 0x2 Good (in use)
LBA Format  1 : Metadata Size: 0   bytes - Data Size: 4096 bytes - Relative Performance: 0x1 Better
```

If a `4096 bytes` format with no metadata is listed and not `(in use)`, switch to it. Use its number with `--lbaf`:

> **Warning**
> `nvme format` erases the whole SSD immediately.

```
root # nvme format "${NVME_NS}" --lbaf=1 --force
root # nvme id-ns -H "${NVME_NS}" | grep 'in use'
```

If only 512-byte formats are listed, skip this step. The rest of the guide works either way: LUKS and ZFS are told to
use 4 KiB blocks below.

### Erasing and partitioning

Discard all blocks (fast on NVMe; tells the SSD that everything is free) and remove old signatures:

```
root # blkdiscard -f "${NVME_NS}"
root # wipefs -a "${NVME_NS}"
```

Create the partitions:

| Partition | Device | Size | Type | Use |
|---|---|---|---|---|
| 1 | `${ESP_PART}` (`/dev/nvme0n1p1`) | 1 GiB | EFI System | ESP, mounted at `/boot/efi`, holds ZFSBootMenu |
| 2 | `${LUKS_PART}` (`/dev/nvme0n1p2`) | remainder | Linux LUKS | LUKS2 container holding the ZFS pool |

```
root # sfdisk "${DISK}" <<'EOF'
label: gpt
size=1GiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI"
type=CA7D7CCB-63ED-4C53-861C-1742536059CC, name="zroot-luks"
EOF
root # udevadm settle
root # mkfs.vfat -F 32 -n EFI "${ESP_PART}"
```

`sfdisk` aligns partitions to 1 MiB, which suits every NVMe SSD.

**Check:**

```
root # udevadm settle
root # lsblk -o NAME,SIZE,FSTYPE,PARTLABEL "${NVME_NS}"
```

Two partitions are listed: `EFI` (vfat) and `zroot-luks`.

### Creating the LUKS container

Create the container with 4 KiB encryption sectors. Type `YES` when asked, then choose the passphrase:

```
root # cryptsetup luksFormat --type luks2 --sector-size 4096 "${LUKS_PART}"
```

> **Important**
> Choose a strong passphrase that uses only characters found on a US keyboard layout: ZFSBootMenu asks for it before
> any keyboard layout is loaded. Without the passphrase, the data cannot be recovered.

Open the container as `luks-<UUID>`, and store three activation flags **in the LUKS header** with `--persistent`. They
then apply at every boot, in ZFSBootMenu and in Gentoo, without any crypttab options:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${LUKS_PART}")"
root # cryptsetup open \
  --allow-discards \
  --perf-no_read_workqueue \
  --perf-no_write_workqueue \
  --persistent \
  "${LUKS_PART}" "luks-${LUKS_UUID}"
```

| Flag | Effect |
|---|---|
| `--allow-discards` | passes TRIM through to the SSD |
| `--perf-no_read_workqueue`, `--perf-no_write_workqueue` | process encryption directly instead of in kernel work queues; lowers latency on fast NVMe SSDs |
| `--persistent` | stores the flags in the LUKS2 header |

> **Note**
> Passing TRIM through reveals which blocks are unused, but not their content. This is the usual trade-off for SSDs;
> leave out `--allow-discards` if that matters to you, and set `autotrim=off` on the pool.

**Check:**

```
root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^Flags|sector'
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|sector size|flags'
```

The header shows `Flags: allow-discards no-read-workqueue no-write-workqueue` and `sector: 4096 [bytes]`; the status
shows `type: LUKS2` and the same flags.

### Creating the ZFS pool

Set a fixed host ID. The same `/etc/hostid` is copied into Gentoo and from there into both initramfs images:

```
root # zgenhostid -f 0x00bab10c
```

Create the pool:

```
root # zpool create -f \
  -o ashift=12 \
  -o autotrim=on \
  -o compatibility=openzfs-2.2-linux \
  -O compression=lz4 \
  -O acltype=posixacl \
  -O xattr=sa \
  -O relatime=on \
  -O dnodesize=auto \
  -m none \
  "${POOL}" "/dev/mapper/luks-${LUKS_UUID}"
```

| Option | Meaning |
|---|---|
| `ashift=12` | 4 KiB blocks, matching the LUKS sector size and the SSD |
| `autotrim=on` | sends TRIM for freed space continuously |
| `compatibility=openzfs-2.2-linux` | keeps the pool importable by live systems with ZFS 2.2; can be relaxed later |
| `compression=lz4` | fast transparent compression, which also reduces writes to the SSD |
| `acltype=posixacl`, `xattr=sa`, `dnodesize=auto` | Linux ACLs and efficient extended attributes (systemd-journald uses them) |
| `-m none` | the pool itself is not mounted |

Create the datasets and set the default boot environment:

```
root # zfs create -o mountpoint=none "${POOL}/ROOT"
root # zfs create -o mountpoint=/ -o canmount=noauto "${POOL}/ROOT/${BE}"
root # zfs create -o mountpoint=/home "${POOL}/home"
root # zpool set bootfs="${POOL}/ROOT/${BE}" "${POOL}"
```

Set the kernel command line that ZFSBootMenu passes to Gentoo. These three `luks.*` parameters replace the crypttab in
Gentoo's initramfs. The key file is created later, in [Creating the LUKS key file](#creating-the-luks-key-file):

```
root # zfs set org.zfsbootmenu:commandline="quiet luks.crypttab=no luks.uuid=${LUKS_UUID} luks.key=${LUKS_UUID}=/etc/cryptsetup-keys.d/zroot.key" "${POOL}/ROOT"
```

| Parameter | Effect |
|---|---|
| `luks.crypttab=no` | ignore `/etc/crypttab` completely |
| `luks.uuid=<UUID>` | unlock the container with this LUKS UUID, as `luks-<UUID>` |
| `luks.key=<UUID>=<path>` | use this key file for it |

ZFSBootMenu adds `root=zfs:zroot/ROOT/gentoo` itself. The property is set on `zroot/ROOT`, so every future boot
environment inherits it.

Re-import the pool below `/mnt/gentoo` and mount it:

```
root # zpool export "${POOL}"
root # mkdir -p /mnt/gentoo
root # zpool import -N -R /mnt/gentoo "${POOL}"
root # zfs mount "${POOL}/ROOT/${BE}"
root # zfs mount -a
root # zpool set cachefile=/etc/zfs/zpool.cache "${POOL}"
root # udevadm trigger
```

**Check:**

```
root # zpool status "${POOL}"
root # zpool get ashift,autotrim "${POOL}"
root # findmnt -R /mnt/gentoo
root # zfs get -H -o value org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"
```

The pool is `ONLINE` on `luks-<UUID>`, `ashift` is `12`, `autotrim` is `on`, both datasets are mounted, and the
command line contains your LUKS UUID twice.

> **Important**
> From now on, run commands that change the pool (`zpool`, `zfs`) on the **live system**, not inside the chroot. The
> ZFS tools inside Gentoo may not match the ZFS kernel module that the live system has loaded.

## Installing the stage file

Download the latest **systemd** stage3 and its signature:

```
root # cd /mnt/gentoo
root # BASE=https://distfiles.gentoo.org/releases/amd64/autobuilds
root # LATEST="$(wget -qO- "${BASE}/latest-stage3-amd64-systemd.txt" | awk '/^[0-9]+T[0-9]+Z\/stage3-amd64-systemd-.*\.tar\.xz/ { print $1 }')"
root # wget "${BASE}/${LATEST}" "${BASE}/${LATEST}.asc"
```

If `LATEST` is empty, download the newest `stage3-amd64-systemd-*.tar.xz` and its `.asc` file manually from
<https://distfiles.gentoo.org/releases/amd64/autobuilds/current-stage3-amd64-systemd/>.

Verify the signature:

```
root # wget -O - https://qa-reports.gentoo.org/output/service-keys.gpg | gpg --import
root # gpg --verify stage3-amd64-systemd-*.tar.xz.asc
```

The output must include `Good signature from "Gentoo Linux Release Engineering (Automated Weekly Release Key)"`.

Extract the stage file and mount the ESP:

```
root # tar xpf stage3-amd64-systemd-*.tar.xz --xattrs-include='*.*' --numeric-owner -C /mnt/gentoo
root # rm stage3-amd64-systemd-*.tar.xz*
root # mkdir -p /mnt/gentoo/boot/efi
root # mount "${ESP_PART}" /mnt/gentoo/boot/efi
```

Copy the ZFS host files and save the variables for the chroot:

```
root # cp /etc/hostid /mnt/gentoo/etc/hostid
root # mkdir -p /mnt/gentoo/etc/zfs
root # cp /etc/zfs/zpool.cache /mnt/gentoo/etc/zfs/zpool.cache
root # cat > /mnt/gentoo/root/install.env <<EOF
export DISK="${DISK}"
export ESP_PART="${ESP_PART}"
export LUKS_PART="${LUKS_PART}"
export LUKS_UUID="${LUKS_UUID}"
export POOL="${POOL}"
export BE="${BE}"
EOF
```

**Check:**

```
root # ls /mnt/gentoo
root # findmnt /mnt/gentoo/boot/efi
root # cat /mnt/gentoo/root/install.env
```

## Installing the base system

### Chrooting

```
root # cp --dereference /etc/resolv.conf /mnt/gentoo/etc/
root # mount --types proc /proc /mnt/gentoo/proc
root # mount --rbind /sys /mnt/gentoo/sys && mount --make-rslave /mnt/gentoo/sys
root # mount --rbind /dev /mnt/gentoo/dev && mount --make-rslave /mnt/gentoo/dev
root # mount --bind /run /mnt/gentoo/run && mount --make-slave /mnt/gentoo/run
root # chroot /mnt/gentoo /bin/bash
```

```
(chroot) root # source /etc/profile
(chroot) root # source /root/install.env
(chroot) root # export PS1="(chroot) ${PS1}"
```

**Check:**

```
(chroot) root # ping -c 2 gentoo.org
(chroot) root # echo "${LUKS_UUID}"
```

### Configuring Portage

Fetch the ebuild repository and confirm the profile:

```
(chroot) root # emerge-webrsync
(chroot) root # eselect profile show
```

The profile must end in `/systemd`, for example `default/linux/amd64/23.0/systemd`. If it does not, select one with
`eselect profile list` and `eselect profile set <number>`.

Add the following to make.conf, replacing `8` with the number of CPU threads (`nproc`):

**FILE** `/etc/portage/make.conf` — *additions*

```
MAKEOPTS="-j8 -l8"

# Distribution kernels; ZFS is rebuilt automatically for each new kernel
USE="dist-kernel"

ACCEPT_LICENSE="-* @FREE @BINARY-REDISTRIBUTABLE"
```

> **Tip**
> To save many hours of compiling, use Gentoo's binary packages: add
> `FEATURES="${FEATURES} getbinpkg binpkg-request-signature"` to make.conf and run `getuto`.

Set the USE flags needed for this setup:

**FILE** `/etc/portage/package.use/zfs-luks`

```
# ZFS: dracut module and root-on-ZFS support
sys-fs/zfs dracut rootfs
# Do not install kernels newer than ZFS supports
sys-fs/zfs-kmod dist-kernel-cap
# Build an initramfs with dracut for every installed kernel
sys-kernel/installkernel dracut
# systemd-cryptsetup and its generator, which read the luks.* kernel parameters
sys-apps/systemd cryptsetup
```

| Package | USE flag | Why |
|---|---|---|
| `sys-fs/zfs` | `dracut`, `rootfs` | dracut module for a ZFS root in Gentoo's initramfs |
| `sys-fs/zfs-kmod` | `dist-kernel-cap` | keeps the kernel within the range ZFS supports |
| `sys-kernel/installkernel` | `dracut` | creates the initramfs when a kernel is installed |
| `sys-apps/systemd` | `cryptsetup` | unlocks the container from the `luks.*` parameters |

### Localization and host name

Replace the time zone, locale, keymap and host name with your own:

```
(chroot) root # ln -sf ../usr/share/zoneinfo/Europe/London /etc/localtime
(chroot) root # echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen
(chroot) root # locale-gen
(chroot) root # eselect locale set en_US.utf8
(chroot) root # echo "KEYMAP=us" > /etc/vconsole.conf
(chroot) root # echo "gentoo" > /etc/hostname
(chroot) root # env-update && source /etc/profile && export PS1="(chroot) ${PS1}"
```

Rebuild the system with the new USE flags:

```
(chroot) root # emerge --ask --verbose --update --deep --newuse @world
```

**Check:** the update finishes without errors, and `emerge --info | grep ^USE` includes `dist-kernel`.

## Kernel and ZFS

```
(chroot) root # emerge --ask \
  sys-kernel/linux-firmware \
  sys-kernel/gentoo-kernel-bin \
  sys-fs/zfs \
  sys-fs/cryptsetup \
  sys-fs/dosfstools \
  sys-boot/efibootmgr \
  sys-apps/nvme-cli \
  dev-vcs/git \
  app-shells/fzf \
  sys-apps/kexec-tools \
  sys-block/mbuffer \
  dev-perl/Sort-Versions \
  dev-perl/YAML-PP \
  dev-perl/boolean
```

| Package | Purpose |
|---|---|
| `sys-kernel/gentoo-kernel-bin` | prebuilt distribution kernel, with the NVMe driver (`sys-kernel/gentoo-kernel` compiles it instead) |
| `sys-fs/zfs` | ZFS tools; pulls in the `sys-fs/zfs-kmod` kernel module |
| `sys-fs/cryptsetup` | LUKS tools, for Gentoo and for the ZFSBootMenu image |
| `sys-apps/nvme-cli` | NVMe health and firmware information |
| `sys-fs/dosfstools`, `sys-boot/efibootmgr` | ESP and firmware boot entries |
| `dev-vcs/git`, `app-shells/fzf`, `sys-apps/kexec-tools`, `sys-block/mbuffer`, Perl modules | build and run ZFSBootMenu |

If a package is *masked by ~amd64 keyword*, accept it and repeat the command:

```
(chroot) root # echo "CATEGORY/PACKAGE ~amd64" >> /etc/portage/package.accept_keywords/zfsbootmenu
```

Tell the kernel installer to keep kernels in `/boot`, where ZFSBootMenu looks for them:

**FILE** `/etc/kernel/install.conf`

```
layout=compat
initrd_generator=dracut
```

**Check:**

```
(chroot) root # ls /lib/modules/
(chroot) root # find /lib/modules -name 'zfs.ko*'
(chroot) root # ls /boot
```

One kernel version is listed, `zfs.ko` exists for it, and `/boot` contains `vmlinuz-<version>` and
`initramfs-<version>.img`. This first initramfs cannot unlock the SSD yet; that is fixed in
[Configuring the initramfs](#configuring-the-initramfs).

## Creating the LUKS key file

Create a random 64-byte key file for Gentoo's initramfs and add it to the container. cryptsetup asks for the
passphrase:

```
(chroot) root # install -d -m 0700 /etc/cryptsetup-keys.d
(chroot) root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot.key
(chroot) root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  "${LUKS_PART}" /etc/cryptsetup-keys.d/zroot.key
```

A random key needs no slow key derivation, so `--pbkdf pbkdf2 --pbkdf-force-iterations 1000` makes the second unlock
almost instant. The passphrase slot keeps the slow, memory-hard default (Argon2id).

**Check:**

```
(chroot) root # ls -l /etc/cryptsetup-keys.d/zroot.key
(chroot) root # cryptsetup open --test-passphrase --verbose --key-file /etc/cryptsetup-keys.d/zroot.key "${LUKS_PART}"
(chroot) root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, cryptsetup reports `Key slot 1 unlocked.`, and two key slots (`0` and
`1`) are listed.

## Configuring the initramfs

Gentoo's initramfs unlocks the container using only the `luks.*` kernel parameters set in
[Creating the ZFS pool](#creating-the-zfs-pool). It needs the ZFS and crypt dracut modules and the key file.

**FILE** `/etc/dracut.conf.d/10-zfs-luks.conf`

```
# Only include what this computer needs (including the NVMe driver)
hostonly="yes"
# Do not store detected rd.luks.* options; the luks.* kernel parameters are used instead
hostonly_cmdline="no"
nofsck="yes"
# ZFS root, and systemd-cryptsetup for the luks.* kernel parameters
add_dracutmodules+=" zfs crypt "
# Always include the NVMe driver; host-only detection may not see through ZFS and LUKS to the SSD
add_drivers+=" nvme "
# The key file named by luks.key=
install_items+=" /etc/cryptsetup-keys.d/zroot.key "
```

Make sure no crypttab is present, then rebuild the initramfs:

```
(chroot) root # [ -e /etc/crypttab ] && mv /etc/crypttab /etc/crypttab.unused
(chroot) root # emerge --config sys-kernel/gentoo-kernel-bin
```

Use `sys-kernel/gentoo-kernel` in the last command if that kernel was installed instead.

**Check:**

```
(chroot) root # INITRD="$(ls -t /boot/initramfs-*.img | head -n 1)"
(chroot) root # ls -l "${INITRD}"
(chroot) root # lsinitrd "${INITRD}" | grep -E 'zfs\.ko|zroot\.key|etc/hostid|zpool\.cache|systemd-cryptsetup$|nvme'
(chroot) root # lsinitrd -f etc/crypttab "${INITRD}"
```

- The initramfs is readable only by root (`-rw-------`). If not, run `chmod 600 /boot/initramfs-*`.
- The `grep` lists `zfs.ko`, `zroot.key`, `etc/hostid`, `zpool.cache` and `systemd-cryptsetup`, and `nvme.ko` if
  the kernel builds NVMe support as a module (Gentoo's distribution kernels do).
- `etc/crypttab` prints nothing.

## Configuring the system

Enable systemd's presets and the ZFS services:

```
(chroot) root # systemd-machine-id-setup
(chroot) root # systemctl preset-all --preset-mode=enable-only
(chroot) root # systemctl enable zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target zfs-zed.service
```

Schedule a monthly scrub (checks every block) and a monthly full TRIM, which complements `autotrim`:

```
(chroot) root # systemctl enable zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
```

Configure wired networking with systemd-networkd. For wireless networks, see
[iwd](https://wiki.gentoo.org/wiki/Iwd) or [NetworkManager](https://wiki.gentoo.org/wiki/NetworkManager).

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

Only the ESP goes into fstab; ZFS mounts its datasets itself:

```
(chroot) root # echo "UUID=$(blkid -s UUID -o value "${ESP_PART}")  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
```

Set the root password and create a user:

```
(chroot) root # passwd
(chroot) root # useradd -m -G wheel,users -s /bin/bash USERNAME
(chroot) root # passwd USERNAME
```

**Check:**

```
(chroot) root # systemctl is-enabled zfs-import-cache.service zfs-mount.service zfs.target systemd-networkd.service
(chroot) root # systemctl is-enabled zfs-scrub-monthly@zroot.timer zfs-trim-monthly@zroot.timer
(chroot) root # cat /etc/fstab
```

All services and timers are `enabled`, and fstab contains one `vfat` line. If the timers are reported as missing,
the installed ZFS version does not ship them; see [Usage](#scrubbing-and-trimming) for running scrub and TRIM by hand.

## Installing ZFSBootMenu

This repository's build is required: upstream ZFSBootMenu cannot unlock LUKS volumes.

```
(chroot) root # ZBM_BRANCH=claude/modest-volta-qv7ugi
(chroot) root # git clone --branch "${ZBM_BRANCH}" \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2.git \
  /usr/local/src/zfsbootmenu
(chroot) root # make -C /usr/local/src/zfsbootmenu core dracut
```

> **Note**
> `ZBM_BRANCH` names the branch that contains the LUKS support. Use `master` once that work has been merged. If one of
> the Perl modules could not be installed with emerge, run
> `emerge --ask dev-perl/App-cpanminus && cpanm --installdeps /usr/local/src/zfsbootmenu`.

Write the generate-zbm configuration. `Kernel.CommandLine` carries `zbm.luks=`, which replaces the ZFSBootMenu
crypttab:

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
  CommandLine: "quiet loglevel=0 zbm.luks=UUID=${LUKS_UUID}"
EOF
```

`zbm.luks=UUID=<UUID>` tells ZFSBootMenu to unlock exactly this container, as `luks-<UUID>`, asking for the passphrase.

Build the image twice; the second run keeps the first image as a backup:

```
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

**Check** the image:

```
(chroot) root # ls -l /boot/efi/EFI/zbm/
(chroot) root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
(chroot) root # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/cryptsetup$|nvme'
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'zroot\.key|etc/zfsbootmenu/crypttab'
(chroot) root # rm /run/zbm-initrd.img
```

- `vmlinuz.EFI` and `vmlinuz-backup.EFI` exist.
- The embedded command line contains `zbm.luks=UUID=` and your LUKS UUID.
- The image contains `cryptsetup` and NVMe support.
- The last `grep` prints **nothing**: no key file and no crypttab are in the image.

Create the firmware boot entries. `efibootmgr` takes the SSD and the ESP's partition number:

```
(chroot) root # efibootmgr -c -d "${DISK}" -p 1 -L "ZFSBootMenu (Backup)" -l '\EFI\ZBM\VMLINUZ-BACKUP.EFI'
(chroot) root # efibootmgr -c -d "${DISK}" -p 1 -L "ZFSBootMenu" -l '\EFI\ZBM\VMLINUZ.EFI'
(chroot) root # mkdir -p /boot/efi/EFI/BOOT
(chroot) root # cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI
```

The copy at `\EFI\BOOT\BOOTX64.EFI` is used by firmware that ignores boot entries.

**Check:**

```
(chroot) root # efibootmgr -v | grep -i zfsbootmenu
```

## Optional: unlocking with a key file on a USB stick

Instead of typing the passphrase, ZFSBootMenu can read a key file from a USB stick. The passphrase keeps working when
the stick is absent.

> **Warning**
> Anyone who has the USB stick and the computer can unlock the SSD. Keep the stick separate from the computer.

Format the stick with the label `ZROOTKEY` (this erases it). Replace `/dev/sdX` with the stick's device, as shown by
`lsblk`:

```
(chroot) root # mkfs.vfat -F 32 -n ZROOTKEY /dev/sdX1
(chroot) root # mkdir -p /mnt/usbkey && mount /dev/disk/by-label/ZROOTKEY /mnt/usbkey
(chroot) root # dd if=/dev/urandom of=/mnt/usbkey/zroot.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  "${LUKS_PART}" /mnt/usbkey/zroot.key
(chroot) root # umount /mnt/usbkey
```

Add `zbm.luks_key=` to `Kernel.CommandLine` in `/etc/zfsbootmenu/config.yaml`. The part after `:` names the device
that holds the file; ZFSBootMenu mounts it read-only, reads the key and unmounts it again:

**FILE** `/etc/zfsbootmenu/config.yaml` — *Kernel section*

```
Kernel:
  CommandLine: "quiet loglevel=0 zbm.luks=UUID=YOUR-LUKS-UUID zbm.luks_key=/zroot.key:LABEL=ZROOTKEY"
```

```
(chroot) root # generate-zbm
```

If the stick is missing, ZFSBootMenu asks for the passphrase.

## Finalizing the installation

Switch DNS to systemd-resolved (this breaks name resolution inside the chroot, so it comes last) and leave the chroot:

```
(chroot) root # ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
(chroot) root # exit
```

Snapshot the fresh installation, unmount everything, export the pool and close the container:

```
root # zfs snapshot "${POOL}/ROOT/${BE}@install"
root # umount /mnt/gentoo/boot/efi
root # umount -R -l /mnt/gentoo/dev /mnt/gentoo/sys /mnt/gentoo/proc /mnt/gentoo/run
root # zfs umount -a
root # zpool export "${POOL}"
root # cryptsetup close "luks-${LUKS_UUID}"
```

**Check:**

```
root # zpool list
root # ls /dev/mapper
```

`zpool list` reports *no pools available*, and `/dev/mapper` contains only `control`. If the export fails because the
pool is busy, leave any shell that is still inside `/mnt/gentoo` and repeat the unmount and export commands.

Reboot and remove the live medium:

```
root # reboot
```

## Usage

### First boot

1. ZFSBootMenu shows `Unlock LUKS volume ...` and asks for the passphrase (or reads the USB key file).
2. ZFSBootMenu lists `zroot/ROOT/gentoo`. Press **Enter**, or wait for the countdown.
3. Gentoo starts **without** asking for the passphrase again.

**Check** after logging in as root:

```
root # cat /proc/cmdline
root # zpool status
root # findmnt /
root # cryptsetup status "luks-$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)" | grep -E 'sector size|flags'
root # systemctl --failed
```

The command line contains `luks.crypttab=no`, `luks.uuid=` and `luks.key=`. The pool is `ONLINE`, `/` is
`zroot/ROOT/gentoo`, the container shows `sector size: 4096` and
`flags: discards no_read_workqueue no_write_workqueue`, and no units have failed.

### Checking the SSD's health

```
root # nvme smart-log /dev/nvme0
```

Watch `critical_warning` (should be `0`), `percentage_used` (wear; 100% is the rated endurance), `media_errors` and
`available_spare`. `nvme list` shows the firmware version; firmware updates come from the SSD vendor.

### Scrubbing and trimming

The timers enabled during installation run a scrub and a full TRIM once a month. To run them by hand, and to see the
result:

```
root # zpool scrub zroot
root # zpool trim zroot
root # zpool status -t zroot
```

### Updating the system

```
root # emerge --sync
root # emerge --ask --verbose --update --deep --newuse @world
root # emerge --ask --depclean
```

New kernels get a rebuilt ZFS module and a new initramfs, including the key file, automatically. Run `generate-zbm`
again after `sys-fs/zfs` or `sys-fs/cryptsetup` is updated, or after changing `/etc/zfsbootmenu/config.yaml`.

Before large updates, take a snapshot: `zfs snapshot zroot/ROOT/gentoo@pre-update-$(date +%Y%m%d)`. To roll back,
select the boot environment in ZFSBootMenu and press **Alt+S** to list its snapshots.

### Changing the passphrase

```
root # cryptsetup luksChangeKey /dev/disk/by-partlabel/zroot-luks
```

### Replacing the key file

Create a new key file, add it, test it, and only then remove the old one:

```
root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot.key.new
root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot.key.new bs=64 count=1
root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 \
  /dev/disk/by-partlabel/zroot-luks /etc/cryptsetup-keys.d/zroot.key.new
root # cryptsetup luksRemoveKey /dev/disk/by-partlabel/zroot-luks /etc/cryptsetup-keys.d/zroot.key
root # mv /etc/cryptsetup-keys.d/zroot.key.new /etc/cryptsetup-keys.d/zroot.key
root # emerge --config sys-kernel/gentoo-kernel-bin
```

Rebuilding the initramfs is required: it contains a copy of the key file.

### Backing up the LUKS header

A damaged LUKS header makes the data unrecoverable, even with the right passphrase. Store a header backup **away from
this SSD**:

```
root # cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks --header-backup-file /PATH/ON/USB/zroot-luks-header.img
```

### Relaxing the pool compatibility

Once no older live system needs to import the pool, newer ZFS features can be enabled. Rebuild ZFSBootMenu first so
that it can still read the pool:

```
root # generate-zbm
root # zpool set compatibility=off zroot
root # zpool upgrade zroot
```

## Troubleshooting

### The firmware does not find ZFSBootMenu

- Check that Secure Boot is disabled.
- Choose *ZFSBootMenu*, or the NVMe SSD itself, in the firmware's boot menu.
- Check the entries with `efibootmgr -v` from a live system; the fallback copy at `\EFI\BOOT\BOOTX64.EFI` is used when
  entries are missing.

### ZFSBootMenu does not ask for the passphrase, or shows no pools

Open the recovery shell from the menu with **Alt+R** and run:

```
root # cat /proc/cmdline
root # ls /dev/nvme*
root # zluks list
root # zluks
```

- `/proc/cmdline` must contain `zbm.luks=UUID=` with the right UUID. If not, fix `Kernel.CommandLine` and run
  `generate-zbm`.
- If no `/dev/nvme*` devices exist, the NVMe driver is missing from the image. Add `add_drivers+=" nvme "` to a file
  in `/etc/zfsbootmenu/dracut.conf.d/` and run `generate-zbm`.
- `zluks` unlocks the container by hand; see `man 8 zluks`.

### The USB key file is not used

In the recovery shell, check that the stick is visible with `ls /dev/disk/by-label/`. If it is not, add
`add_drivers+=" usb_storage uas "` to a file in `/etc/zfsbootmenu/dracut.conf.d/` and run `generate-zbm`.

### Gentoo asks for the passphrase again after ZFSBootMenu

Gentoo's initramfs could not use the key file. On the running system:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline zroot/ROOT
root # lsinitrd /boot/initramfs-$(uname -r).img | grep -E 'zroot\.key|systemd-cryptsetup$'
```

- The command line must contain `luks.uuid=` and `luks.key=` with the right UUID.
- If the key file is missing from the initramfs, check `/etc/dracut.conf.d/10-zfs-luks.conf` and run
  `emerge --config sys-kernel/gentoo-kernel-bin`.
- If `systemd-cryptsetup` is missing, `sys-apps/systemd` was built without `USE="cryptsetup"`.

### Gentoo drops to an emergency shell and cannot import the pool

Import it by hand and continue booting:

```
root # zpool import -f -N zroot
root # exit
```

Then check that `hostid` prints `00bab10c` and that `/etc/zfs/zpool.cache` exists, and rebuild the initramfs.

### zfs.ko is missing after a kernel update

The kernel is newer than ZFS supports. Make sure `sys-fs/zfs-kmod dist-kernel-cap` is set in
`/etc/portage/package.use/zfs-luks` and run `emerge --ask --update --deep --newuse @world`. The supported range is in
the ebuild:

```
root # grep -h MODULES_KERNEL_MAX /var/db/repos/gentoo/sys-fs/zfs-kmod/*.ebuild
```

### Repairing the system from the live medium

```
root # export DISK=/dev/disk/by-id/nvme-YOUR-MODEL_YOUR-SERIAL
root # export LUKS_PART="${DISK}-part2"
root # export LUKS_UUID="$(cryptsetup luksUUID "${LUKS_PART}")"
root # cryptsetup open "${LUKS_PART}" "luks-${LUKS_UUID}"
root # zpool import -f -N -R /mnt/gentoo zroot
root # zfs mount zroot/ROOT/gentoo
root # zfs mount -a
root # mount "${DISK}-part1" /mnt/gentoo/boot/efi
```

The stored TRIM and performance flags are applied automatically by `cryptsetup open`. Then chroot as in
[Chrooting](#chrooting), and finish as in [Finalizing the installation](#finalizing-the-installation).

## See also

- [Handbook:AMD64](https://wiki.gentoo.org/wiki/Handbook:AMD64) — the Gentoo installation handbook
- [NVMe](https://wiki.gentoo.org/wiki/NVMe) — NVMe SSDs on Gentoo
- [ZFS](https://wiki.gentoo.org/wiki/ZFS) — ZFS on Gentoo
- [ZFS/rootfs](https://wiki.gentoo.org/wiki/ZFS/rootfs) — booting Gentoo from a ZFS root
- [Dm-crypt](https://wiki.gentoo.org/wiki/Dm-crypt) — disk encryption with dm-crypt and LUKS
- [Full Disk Encryption From Scratch](https://wiki.gentoo.org/wiki/Full_Disk_Encryption_From_Scratch)
- [SSD](https://wiki.gentoo.org/wiki/SSD) — TRIM and other SSD topics
- [Dracut](https://wiki.gentoo.org/wiki/Dracut) — the initramfs generator
- [Systemd](https://wiki.gentoo.org/wiki/Systemd)

## External resources

- This repository's documentation: `docs/general/luks.rst` (how unlocking works), `docs/luks/administration.rst` (key
  management) and the manual pages `zfsbootmenu-luks(7)` and `zluks(8)`
- [ZFSBootMenu documentation](https://docs.zfsbootmenu.org/)
- [systemd-cryptsetup-generator(8)](https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html)
  — the `luks.*` kernel parameters
- [cryptsetup(8)](https://man7.org/linux/man-pages/man8/cryptsetup.8.html) — `--persistent`, `--sector-size` and the
  performance flags
- [OpenZFS: Workload Tuning](https://openzfs.github.io/openzfs-docs/Performance%20and%20Tuning/Workload%20Tuning.html)
- [Cloudflare: Speeding up Linux disk encryption](https://blog.cloudflare.com/speeding-up-linux-disk-encryption/) —
  background on the work-queue flags
