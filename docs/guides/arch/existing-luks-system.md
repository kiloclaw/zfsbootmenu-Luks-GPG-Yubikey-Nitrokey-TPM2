# Adding ZFSBootMenu to an existing LUKS-encrypted ZFS root (Arch Linux)

This article is for an **Arch Linux** system that already runs from a ZFS pool inside one or more **LUKS2**
containers. Today another boot loader (GRUB, systemd-boot, rEFInd or the kernel's EFI stub) starts the kernel, and
the initramfs built by mkinitcpio asks for the passphrase. After following this article, the system boots with
**ZFSBootMenu** from this repository, without reinstalling, and the old boot loader stays available as a fallback until
ZFSBootMenu has proven itself.

You type the passphrase **once**, in ZFSBootMenu. Arch's initramfs then unlocks the containers a second time with a
**key file** stored inside the encrypted pool.

> **Important**
> Upstream ZFSBootMenu cannot unlock LUKS volumes. Its LUKS support exists only in this repository, on the
> development branch `claude/zbm-refactor`. The AUR packages `zfsbootmenu` and `zfsbootmenu-efi-bin` build or ship
> **upstream** ZFSBootMenu: do not use them for this setup.

> **Note**
> This fork's VM tests boot only images built with dracut, and only boot environments whose initramfs is built by
> dracut ([Not Covered](../../luks/testing.rst#not-covered)). Arch's mkinitcpio initramfs, which this article keeps,
> is not covered by them. Keep the fallback described here until several boots have worked.

## Contents

- [Overview](#overview)
  - [What changes in the boot chain](#what-changes-in-the-boot-chain)
  - [Why the system's initramfs unlocks the containers again](#why-the-systems-initramfs-unlocks-the-containers-again)
  - [What stays unencrypted](#what-stays-unencrypted)
  - [Which initramfs you have](#which-initramfs-you-have)
- [Requirements](#requirements)
  - [Conventions](#conventions)
- [Before you start](#before-you-start)
  - [Setting variables](#setting-variables)
  - [Snapshotting the boot environments](#snapshotting-the-boot-environments)
  - [Backing up the LUKS headers and the ESP](#backing-up-the-luks-headers-and-the-esp)
  - [Preparing a rescue medium](#preparing-a-rescue-medium)
  - [Recording the current boot setup](#recording-the-current-boot-setup)
- [Checking the ZFS packages](#checking-the-zfs-packages)
- [Checking and fixing the layout](#checking-and-fixing-the-layout)
  - [The root dataset and bootfs](#the-root-dataset-and-bootfs)
  - [The host ID](#the-host-id)
  - [Moving /boot into the root dataset](#moving-boot-into-the-root-dataset)
  - [Several LUKS containers](#several-luks-containers)
  - [Swap](#swap)
- [Unlocking a second time with a key file](#unlocking-a-second-time-with-a-key-file)
  - [Creating the key file](#creating-the-key-file)
  - [Keeping kernels and the key file off the ESP](#keeping-kernels-and-the-key-file-off-the-esp)
  - [Way A: busybox initramfs (encrypt and zfs hooks)](#way-a-busybox-initramfs-encrypt-and-zfs-hooks)
  - [Way B: systemd initramfs (sd-encrypt)](#way-b-systemd-initramfs-sd-encrypt)
  - [Way C: dracut](#way-c-dracut)
  - [The command line for the system](#the-command-line-for-the-system)
  - [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs)
- [Installing ZFSBootMenu](#installing-zfsbootmenu)
  - [Dependencies](#dependencies)
  - [Building and installing](#building-and-installing)
- [Configuring generate-zbm](#configuring-generate-zbm)
  - [config.yaml key by key](#configyaml-key-by-key)
  - [Choosing the runtime](#choosing-the-runtime)
  - [Building and checking the image](#building-and-checking-the-image)
  - [Alternative: building the image with mkinitcpio](#alternative-building-the-image-with-mkinitcpio)
- [Firmware entries and the first boot](#firmware-entries-and-the-first-boot)
  - [Secure Boot](#secure-boot)
  - [Creating the entries](#creating-the-entries)
  - [Keeping the old boot loader able to import the pool](#keeping-the-old-boot-loader-able-to-import-the-pool)
  - [Booting ZFSBootMenu once](#booting-zfsbootmenu-once)
  - [Making ZFSBootMenu the default](#making-zfsbootmenu-the-default)
  - [Optional: the removable-media fallback path](#optional-the-removable-media-fallback-path)
- [Keeping it working](#keeping-it-working)
  - [Rebuilding the image automatically](#rebuilding-the-image-automatically)
  - [Updating ZFSBootMenu](#updating-zfsbootmenu)
  - [Changing the passphrase](#changing-the-passphrase)
  - [Replacing the key file](#replacing-the-key-file)
  - [The old boot loader after kernel updates](#the-old-boot-loader-after-kernel-updates)
- [Optional next steps](#optional-next-steps)
- [Troubleshooting](#troubleshooting)
- [Removing ZFSBootMenu](#removing-zfsbootmenu)
- [Security notes](#security-notes)
- [See also](#see-also)

## Overview

### What changes in the boot chain

| | Today | With ZFSBootMenu |
|---|---|---|
| Firmware starts | GRUB, systemd-boot, rEFInd, or the kernel | `\EFI\zbm\vmlinuz.EFI`, one EFI file on the ESP |
| Passphrase asked by | Arch's initramfs | ZFSBootMenu |
| Kernel and initramfs read from | the ESP or a `/boot` partition | `/boot` in the root dataset, inside the pool |
| Kernel started by | the boot loader | ZFSBootMenu, with `kexec` |
| Containers unlocked by Arch's initramfs with | the passphrase | a key file inside that initramfs |

1. The firmware starts ZFSBootMenu from the EFI system partition (ESP).
2. ZFSBootMenu reads `zbm.luks=` from its own command line, asks for the passphrase, opens the containers and
   imports the pool.
3. It finds the boot environments: every filesystem with `mountpoint=/` whose `/boot` holds a kernel with an
   initramfs. The pool's `bootfs` is booted after a countdown, or you choose another one.
4. It exports the pool, closes the containers (which wipes their volume keys from memory) and starts Arch's kernel
   with `kexec`, passing the command line from the ZFS property `org.zfsbootmenu:commandline`.
5. Arch's initramfs unlocks the containers again with the key file, imports the pool and mounts the root.

### Why the system's initramfs unlocks the containers again

`kexec` starts a new kernel. The new kernel has no open dm-crypt mappings, so Arch's initramfs must unlock the
containers again, exactly as it does today
([Booting the system afterwards](../../general/luks.rst#booting-the-system-afterwards)). To avoid a second prompt, a
random key file is added to a LUKS key slot and copied into Arch's initramfs.

This is safe only because the initramfs lives in `/boot` **inside the encrypted pool**. Nobody can read it, or the key
file inside it, without first unlocking the pool. That is why `/boot` must become a directory of the root dataset: an
initramfs with a key file on the unencrypted ESP, or on an unencrypted `/boot` partition, would hand the key to anyone
who can read the disk.

### What stays unencrypted

Only the ESP. After the switch it holds:

- ZFSBootMenu's image and its backup (`EFI/zbm/`). They contain no key: plain key files are never put into the image
  ([Security notes](../../general/luks.rst#security-notes));
- the old boot loader and the copies of the kernel and initramfs it uses today, which contain no key either (they were
  built before the key file existed).

### Which initramfs you have

Arch's initramfs is built by mkinitcpio. Its `HOOKS` decide how the containers are unlocked and the pool imported.
mkinitcpio reads `/etc/mkinitcpio.conf` and then every `/etc/mkinitcpio.conf.d/*.conf` drop-in, so a drop-in can
override `HOOKS` or `FILES`. Look at all of them:

```
root # cat /etc/mkinitcpio.conf /etc/mkinitcpio.conf.d/*.conf 2>/dev/null | grep -E '^[[:space:]]*(HOOKS|FILES)'
root # grep -E '^[^#]*_config=' /etc/mkinitcpio.d/*.preset
root # cat /proc/cmdline
```

The last `HOOKS` line printed is the one that counts (later lines override earlier ones; `FILES+=(...)` adds to
`FILES`). A preset with an active `*_config=` line uses that file instead, and mkinitcpio then ignores the drop-ins;
edit that file wherever this article says `/etc/mkinitcpio.conf`.

| Way | `HOOKS` contain | Kernel parameters today | Containers |
|---|---|---|---|
| A | `udev ... encrypt zfs` | `cryptdevice=`, `zfs=` or `root=ZFS=` | **one** |
| B | `systemd ... sd-encrypt`, a systemd ZFS hook | `rd.luks.*`, or a crypttab | any number |
| C | none: dracut builds the initramfs | `rd.luks.*` or `luks.*`, `root=zfs:` | any number |

The `encrypt` hook handles exactly one container (it reads one `cryptdevice=`). A pool spread over several containers
is therefore already booting with way B or C. The sections
[Way A](#way-a-busybox-initramfs-encrypt-and-zfs-hooks), [Way B](#way-b-systemd-initramfs-sd-encrypt) and
[Way C](#way-c-dracut) describe each. Whatever you use today keeps working: this article only adds the key
file and moves the kernel command line into a ZFS property.

## Requirements

- An x86_64 computer booting in **UEFI** mode. ZFSBootMenu's EFI image does not boot in legacy BIOS mode.
- **Secure Boot disabled**: the image built here is not signed (see [Secure Boot](#secure-boot)).
- The pool already contains the running root file system, inside LUKS2 containers, and the system boots today.
- A root shell, a network connection, and an hour without pressure.
- A USB stick for the LUKS header backup, and a rescue medium (see
  [Preparing a rescue medium](#preparing-a-rescue-medium)).

**Check:**

```
root # ls /sys/firmware/efi
root # findmnt -n -o SOURCE,FSTYPE /
root # cryptsetup --version
root # zfs version
```

The first command lists files (otherwise the system booted in legacy BIOS mode). `/` is a ZFS dataset such as
`zroot/ROOT/arch` of type `zfs`.

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the running Arch system; do not type the prompt |
| `(live) root #` | run as root on the rescue medium |
| **FILE** `/path` | create or edit this file with the content shown |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Before you start

### Setting variables

Find the root dataset, the pool, its LUKS containers and the ESP. The function `pool_luks_devices` comes from
[administration.rst](../../luks/administration.rst) and prints the LUKS partitions that hold the pool's vdevs:

```
root # pool_luks_devices() {
  local vdev
  for vdev in $(zpool status -LP "$1" | awk '$1 ~ "^/dev/" { print $1 }'); do
    [ -e "/sys/class/block/${vdev##*/}/dm/uuid" ] || continue
    grep -q '^CRYPT-' "/sys/class/block/${vdev##*/}/dm/uuid" || continue
    echo "/dev/$(ls "/sys/class/block/${vdev##*/}/slaves")"
  done
}
root # export ROOT_DS="$(findmnt -n -o SOURCE /)"
root # export POOL="${ROOT_DS%%/*}"
root # export BE_PARENT="${ROOT_DS%/*}"
root # export LUKS_PARTS="$(pool_luks_devices "${POOL}" | tr '\n' ' ')"
root # export LUKS_PART="$(pool_luks_devices "${POOL}" | head -n 1)"
root # export LUKS_UUID="$(cryptsetup luksUUID "${LUKS_PART}")"
root # export LUKS_NAME="$(lsblk -n -r -o NAME,TYPE "${LUKS_PART}" | awk '$2 == "crypt" { print $1 }')"
root # export KEY_FILE="/etc/cryptsetup-keys.d/${POOL}.key"
```

Find the ESP by its partition type, and choose where it will be mounted afterwards. This article uses `/boot/efi`,
the default of ZFSBootMenu's configuration. `/efi` works as well; then set `ESP_MNT=/efi`. If the ESP is mounted at
`/efi` today, keep it there with `ESP_MNT=/efi`, and it need not move at all:

```
root # lsblk -p -o NAME,SIZE,FSTYPE,PARTTYPE,MOUNTPOINTS
root # ESP_TYPE=c12a7328-f81f-11d2-ba4b-00a0c93ec93b
root # export ESP_PART="$(lsblk -p -n -r -o NAME,PARTTYPE | awk -v t="${ESP_TYPE}" '$2 == t { print $1 }')"
root # export ESP_DISK="/dev/$(lsblk -n -o PKNAME "${ESP_PART}")"
root # export ESP_PARTNUM="$(cat "/sys/class/block/${ESP_PART##*/}/partition")"
root # export ESP_MNT=/boot/efi
```

If the computer has several ESPs (several disks, or another operating system), `ESP_PART` holds several lines: set it
by hand to the partition the firmware boots today, which the `HD(...)` part of `efibootmgr -v` identifies by its
partition GUID (`lsblk -o NAME,PARTUUID`).

Save the variables, so that a new shell can load them with `source /root/zbm-switch.env`:

```
root # cat > /root/zbm-switch.env <<EOF
export ROOT_DS="${ROOT_DS}"
export POOL="${POOL}"
export BE_PARENT="${BE_PARENT}"
export LUKS_PARTS="${LUKS_PARTS}"
export LUKS_PART="${LUKS_PART}"
export LUKS_UUID="${LUKS_UUID}"
export LUKS_NAME="${LUKS_NAME}"
export KEY_FILE="${KEY_FILE}"
export ESP_PART="${ESP_PART}"
export ESP_DISK="${ESP_DISK}"
export ESP_PARTNUM="${ESP_PARTNUM}"
export ESP_MNT="${ESP_MNT}"
EOF
```

**Check:**

```
root # cat /root/zbm-switch.env
root # for p in ${LUKS_PARTS}; do echo "${p} UUID=$(cryptsetup luksUUID "${p}")"; done
```

Every variable has a value, `LUKS_PARTS` lists every container of the pool (one line each in `zpool status -LP`),
`LUKS_NAME` is the mapping name you see below `/dev/mapper` today (such as `cryptroot` or `luks-<UUID>`), and
`ESP_PART` is one partition, for example `/dev/nvme0n1p1` with `ESP_DISK=/dev/nvme0n1` and `ESP_PARTNUM=1`.

### Snapshotting the boot environments

A recursive snapshot of everything below `${BE_PARENT}` lets you return to the state before any change:

```
root # zfs snapshot -r "${BE_PARENT}@before-zfsbootmenu"
```

If the root dataset sits directly below the pool (`POOL/arch`), `${BE_PARENT}` is the pool itself, and `-r` would
snapshot every dataset in it, including large data sets whose snapshots keep deleted data on the disk. Then snapshot
only the root dataset, plus any other datasets the system needs to start:
`zfs snapshot "${ROOT_DS}@before-zfsbootmenu"`.

Later, ZFSBootMenu itself can roll back to it (select the boot environment and list its snapshots). Destroy the
snapshot once everything works, so that it does not keep old data alive forever:
`zfs destroy -r "${BE_PARENT}@before-zfsbootmenu"`.

**Check:**

```
root # zfs list -t snapshot -o name,creation | grep before-zfsbootmenu
```

### Backing up the LUKS headers and the ESP

A damaged LUKS header makes a container unreadable for good, whatever the passphrase. This article adds a key slot to
every header, so back them up first, **away from this disk**. Mount a USB stick at `/mnt/usb` and run:

```
root # for p in ${LUKS_PARTS}; do
  cryptsetup luksHeaderBackup "${p}" --header-backup-file "/mnt/usb/$(cryptsetup luksUUID "${p}").header"
done
```

A header backup contains the key slots as they are now. Anyone who has it and a passphrase that is valid now can open
the container even after you change that passphrase: store it like the passphrase itself, and make a new backup
after removing a key slot.

The ESP's content is not in the ZFS snapshot. Keep a copy of it in the pool, where it is encrypted. The ESP must be
mounted (with an automount, the `ls` mounts it); the archive is made from its current mount point, wherever that is:

```
root # ls /boot /efi /boot/efi >/dev/null 2>&1; findmnt -n -o TARGET "${ESP_PART}"
root # tar -C "$(findmnt -n -o TARGET "${ESP_PART}" | head -n 1)" -cpf /root/esp-before-zfsbootmenu.tar .
```

**Check:**

```
root # ls -l /mnt/usb/*.header
root # tar -tf /root/esp-before-zfsbootmenu.tar | head
```

One header file exists for each container, and the archive lists the ESP's files, such as `./EFI/`.

### Preparing a rescue medium

If neither ZFSBootMenu nor the old boot loader starts the system, a live system that can open LUKS and import the pool
is the way back in. The official Arch ISO does not include ZFS. Use one of:

- a live system with OpenZFS at least as new as the pool's features need, for example Ubuntu's desktop live image
  (choose *Try Ubuntu*, open a terminal, run `sudo -i`);
- an Arch ISO you built yourself with archiso and the archzfs packages;
- the ZFSBootMenu recovery image of this repository, built in the container
  ([Recovery image](../../general/building.rst#recovery-image)), whose recovery shell has `zluks`, `zpool` and
  `zfs-chroot`.

Which ZFS version is new enough depends on the pool:

```
root # zfs version
root # zpool get compatibility "${POOL}"
root # zpool status "${POOL}" | grep -A3 -i 'status:'
```

A live system with an older OpenZFS can refuse the pool, or import it only read-only, when the pool uses newer
features. Try the rescue medium **now**, before changing anything, with these commands (replace the device and the
names with yours; `-R /mnt` keeps the pool's mount points below `/mnt`):

```
(live) root # cryptsetup open /dev/YOUR-LUKS-PARTITION LUKS-NAME
(live) root # zpool import -N -R /mnt POOL
(live) root # zfs mount POOL/ROOT/YOUR-BE
(live) root # zfs mount -a
(live) root # mount /dev/YOUR-ESP /mnt/boot/efi
```

The last command applies once the ESP has moved to `/boot/efi`; before that, mount it at `/mnt/boot`. On an Arch
medium, `arch-chroot /mnt` enters the system. On others:

```
(live) root # for d in proc sys dev run; do mount --rbind "/${d}" "/mnt/${d}"; mount --make-rslave "/mnt/${d}"; done
(live) root # chroot /mnt /bin/bash
```

When done, leave the chroot and always export the pool, so that the next import by ZFSBootMenu or Arch is clean:

```
(live) root # umount -R /mnt/proc /mnt/sys /mnt/dev /mnt/run /mnt/boot/efi
(live) root # zfs umount -a
(live) root # zpool export POOL
(live) root # cryptsetup close LUKS-NAME
```

If the import is refused because the pool was last used by another system, add `-f` to `zpool import`, and export
the pool again before rebooting. Do not run `zpool upgrade` from a live system.

**Check:** on the rescue medium, `zpool import` lists the pool as `ONLINE` after `cryptsetup open`.

### Recording the current boot setup

Keep a record of how the system boots today. You will need the firmware entries to return to the old boot loader, and
the kernel command line to build ZFSBootMenu's:

```
root # efibootmgr -v | tee /root/efibootmgr-before.txt
root # cat /proc/cmdline | tee /root/cmdline-before.txt
root # cp -a /etc/fstab /root/fstab-before
root # cp -a /etc/mkinitcpio.conf /root/mkinitcpio.conf-before
root # findmnt /boot; findmnt /boot/efi; findmnt /efi
```

**Check:** `/root/efibootmgr-before.txt` shows `BootOrder` and the entry of the current boot loader, and
`/root/cmdline-before.txt` holds the current kernel command line.

## Checking the ZFS packages

ZFSBootMenu's image is built with the ZFS kernel module and tools of this system, so they must be installed and
current. On Arch they usually come from the **archzfs** repository:

| Package | Contains |
|---|---|
| `zfs-utils` | tools, udev rules, the mkinitcpio hook `zfs`, the dracut module, systemd units |
| `zfs-dkms` | module source, rebuilt by DKMS for every installed kernel |
| `zfs-linux`, `zfs-linux-lts`, ... | module built for exactly one version of `linux`, `linux-lts`, ... |

```
root # pacman -Q | grep -E '^(zfs|linux)'
root # grep -A3 '^\[archzfs\]' /etc/pacman.conf
```

The archzfs project now publishes its repository as GitHub releases. The former `archzfs.com` repository is stale.
If `/etc/pacman.conf` still points there, replace the section with the one the project documents in the notes of its
[`experimental` release](https://github.com/archzfs/archzfs/releases/tag/experimental):

**FILE** `/etc/pacman.conf` — *archzfs section*

```
[archzfs]
SigLevel = Required
Server = https://github.com/archzfs/archzfs/releases/download/experimental
```

`SigLevel = Required` needs the project's release key in pacman's keyring. Import it, then compare the fingerprint
with the one published in the
[archzfs-keyring repository](https://github.com/archzfs/archzfs-keyring/tree/master/keyring/packager/archzfs)
before signing it locally:

```
root # pacman-key --recv-keys 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
root # pacman-key --finger 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
root # pacman-key --lsign-key 3A9917BF0DED5C13F69AC68FABEC0A1208037BE9
root # pacman -Syu
```

An installable `archzfs-keyring` package is announced but, at the time of writing, not finished; follow the
project's instructions once it is. The same release notes advise caution with the new repository and suggest starting
with non-critical systems, and they describe `SigLevel = Never` only as a temporary compatibility option. Never use
`SigLevel = Never` for the repository that supplies your kernel module: an unsigned package runs as root during
every update.

If `pacman -Syu` installed a new kernel or new ZFS packages, **reboot now with the old boot loader** and confirm that
the system still starts, that `zpool status` shows the pool `ONLINE`, and (with `zfs-dkms`) that `dkms status` shows
the module `installed` for the running kernel. Do not go on until it does: every later step relies on a system that
boots as it is.

> **Warning**
> `zfs-linux` depends on **exactly one** version of `linux`. When Arch publishes a new kernel before archzfs has
> built a matching module, `pacman -Syu` stops on the unsatisfied dependency, and a partial upgrade is not supported on
> Arch. `zfs-dkms` builds for any kernel, but only for kernels that the OpenZFS release supports: a kernel too new
> for it gets no `zfs.ko`, and neither Arch nor ZFSBootMenu can then import the pool with that kernel. Many ZFS users
> on Arch keep `linux-lts` installed as a second kernel for this reason.

**Check:**

```
root # pacman -Qi zfs-utils | grep -E '^(Version|Repository)'
root # find /usr/lib/modules -name 'zfs.ko*'
root # ls /usr/lib/initcpio/install/zfs
root # ls -d /usr/lib/dracut/modules.d/90zfs
```

Every kernel in `/usr/lib/modules` has a `zfs.ko` (or `zfs.ko.zst`), and the `zfs` mkinitcpio hook exists. If you use
`zfs-dkms`, `dkms status` shows the module `installed` for every kernel. The OpenZFS dracut module `90zfs` matters
only if dracut builds the **system's** initramfs ([Way C](#way-c-dracut)); ZFSBootMenu's own dracut module does not
depend on it. This article could not confirm from Arch's servers that archzfs's `zfs-utils` installs it; the
`ls -d` answers that for your system.

## Checking and fixing the layout

### The root dataset and bootfs

ZFSBootMenu lists every ZFS filesystem with `mountpoint=/` as a boot environment
([Boot Environments and You](../../general/bootenvs-and-you.rst)). The usual layout, `POOL/ROOT/NAME` with
`mountpoint=/` and `canmount=noauto`, lets you add more boot environments next to it later, which inherit the
properties set on `POOL/ROOT`. The pool's `bootfs` tells ZFSBootMenu which one to boot after the countdown.

```
root # zfs list -o name,mountpoint,canmount,mounted -r "${POOL}"
root # zpool get bootfs "${POOL}"
```

If `bootfs` is not the running root, set it:

```
root # zpool set bootfs="${ROOT_DS}" "${POOL}"
```

If `canmount` of the root dataset is `on`, set it to `noauto`, so that other boot environments with `mountpoint=/`
added later are not mounted over each other by `zfs mount -a`. The running root stays mounted:

```
root # zfs set canmount=noauto "${ROOT_DS}"
```

A root dataset elsewhere, such as `POOL/arch`, works too: ZFSBootMenu does not require the `ROOT` level.
`${BE_PARENT}` is then the pool itself, and properties set there are inherited by every dataset in the pool. Moving a
mounted root to another name is not possible from the running system; leave it where it is.

**Check:**

```
root # zfs get -H -o property,value mountpoint,canmount "${ROOT_DS}"
root # zpool get -H -o value bootfs "${POOL}"
```

`mountpoint` is `/`, `canmount` is `noauto`, and `bootfs` is `${ROOT_DS}`.

### The host ID

ZFS records in the pool's labels the host ID of the system that imported it, and `zpool import` refuses a pool that
was not exported and carries another host's non-zero ID. ZFSBootMenu's image takes `/etc/hostid` (or, without that
file, the value `hostid` prints, unless it is zero) when it is built, imports the pool with that host ID, and passes
it to Arch's kernel as `spl.spl_hostid=` (the option `zbm.set_hostid`, on by default, see
[zfsbootmenu(7)](../../man/zfsbootmenu.7.rst)). The ZFS module in Arch's initramfs then imports with the same host
ID. This matters on Arch, because the archzfs `zfs` hook, in its current version, does not copy `/etc/hostid` into
the initramfs: without `spl.spl_hostid=`, Arch's initramfs imports with host ID 0.

```
root # ls -l /etc/hostid
root # hostid
```

If `/etc/hostid` exists, keep it. If it does not, create it with the value `hostid` prints now, which is also the
value OpenZFS's dracut module puts into a dracut initramfs on a system without the file:

```
root # zgenhostid "$(hostid)"
```

Creating the file is safe for ZFSBootMenu and for Arch started by it: a pool that was imported with host ID 0, as
it was on a system without `/etc/hostid`, is not refused by any host. `zgenhostid` refuses to overwrite an existing
file unless given `-f`; do not do that on a working system without a reason.

The other direction needs care: once a boot through ZFSBootMenu has imported the pool with this host ID, the old
boot loader's initramfs, which imports with host ID 0, refuses the pool, because Arch does not export the root pool
at shutdown. [Keeping the old boot loader able to import the pool](#keeping-the-old-boot-loader-able-to-import-the-pool)
fixes that before the first boot through ZFSBootMenu.

**Check:**

```
root # od -A n -t x4 /etc/hostid
root # hostid
```

Both print the same eight hexadecimal digits.

### Moving /boot into the root dataset

ZFSBootMenu boots the kernels it finds in `/boot` **inside the boot environment**. On many Arch systems, `/boot` is the
ESP itself (systemd-boot, EFISTUB, rEFInd) or a separate unencrypted partition (GRUB). In both cases the kernel,
the initramfs and, after the next step, the key file would sit on an unencrypted partition. This step makes `/boot` a
plain directory of the root dataset and mounts the ESP at `${ESP_MNT}` instead.

The old boot loader keeps working afterwards: its files stay on the ESP (or on the old `/boot` partition), untouched.
See [The old boot loader after kernel updates](#the-old-boot-loader-after-kernel-updates) for how long.

> **Warning**
> Do not run `pacman -Syu`, and do not install or update a kernel package, from the start of this step until
> ZFSBootMenu has booted the system successfully. A kernel update removes the running kernel's modules from
> `/usr/lib/modules`, so the old boot loader's kernel on the ESP would no longer boot, before ZFSBootMenu has proven
> that it does.

Find out where `/boot` lives:

```
root # findmnt -n -o SOURCE,FSTYPE /boot
root # findmnt -n -o SOURCE,FSTYPE "${ESP_MNT}"
```

| `findmnt /boot` shows | Situation | What to do |
|---|---|---|
| nothing | `/boot` is already a directory of the root dataset | skip to the **Check** |
| the ESP (`vfat`) | systemd-boot, EFISTUB or rEFInd setups | steps 1 to 7 below |
| another partition (`ext4`, `xfs`, ...) | GRUB; ESP at `/boot/efi` or `/efi` | steps 1 to 7 |
| a ZFS dataset (`bpool/BOOT/arch`, ...) | a separate boot pool or dataset | steps 1 to 7, see the note |

> **Note**
> ZFSBootMenu does not mount a separate boot dataset: it reads `/boot` of the boot environment only. If `/boot` is a
> dataset of its own, unmount it in step 2 with `zfs set canmount=noauto DATASET` and `zfs umount DATASET` instead of
> `umount /boot`. Leave the dataset itself alone: the old boot loader reads it.

1. Stop anything that writes to `/boot`, and do not run `pacman` until step 7.

2. Unmount the ESP if it is mounted inside `/boot`, then `/boot` itself. If the fstab line has
   `x-systemd.automount`, systemd would mount it again on the next access: stop the automount units first
   (`boot.automount`, and `boot-efi.automount` for `/boot/efi`):

   ```
   root # systemctl stop boot-efi.automount boot.automount 2>/dev/null
   root # umount /boot/efi 2>/dev/null; umount /boot
   root # findmnt /boot
   ```

   `findmnt` prints nothing now. If `umount` reports that the target is busy, find the user with `fuser -vm /boot`.

3. Look at what the root dataset has below the old mount point. Files there were hidden by the mount:

   ```
   root # ls -A /boot
   ```

   If it is not empty, move its content aside rather than mixing it with new files:

   ```
   root # mkdir /root/boot-hidden
   root # find /boot -mindepth 1 -maxdepth 1 -exec mv -t /root/boot-hidden/ {} +
   ```

4. Create the new mount point, and change the ESP's line in `/etc/fstab` so that its second field is `${ESP_MNT}`.
   Keep the other fields. If `/boot` was another partition, put a `#` in front of its line, so that it is no longer
   mounted (it stays intact for the old boot loader):

   ```
   root # mkdir -p "${ESP_MNT}"
   root # nano /etc/fstab
   ```

   **FILE** `/etc/fstab` — *example ESP line after the change*

   ```
   UUID=ABCD-1234  /boot/efi  vfat  umask=0077  0 2
   ```

   `umask=0077` makes the ESP readable by root only, which is a good choice for any setup.

   On many systemd-boot installations the ESP has **no line** in `/etc/fstab`: systemd-gpt-auto-generator mounts it
   at `/boot` by itself. Look for the ESP's line:

   ```
   root # blkid -s UUID -o value "${ESP_PART}"
   root # grep -v '^#' /etc/fstab | grep -i "$(blkid -s UUID -o value "${ESP_PART}")"
   ```

   If the `grep` prints nothing, add a line like the example above with that UUID. Without it the ESP would not be
   mounted at all: the generator creates no mount for a target directory that contains files, and `/boot` now does.
   `generate-zbm` also needs the line to mount `${ESP_MNT}` itself.

5. Reload the mount units, check `/etc/fstab`, and mount the ESP at its new place:

   ```
   root # systemctl daemon-reload
   root # findmnt --verify
   root # mount "${ESP_MNT}"
   ```

   `findmnt --verify` reports errors in `/etc/fstab`; fix every error it prints before any reboot. Warnings about
   the commented `/boot` line do not matter.

6. Check where the kernel presets put kernels and images, **before** step 7 writes them. On systemd-boot and EFISTUB
   setups a preset often points at the ESP:

   ```
   root # grep -nE '^[^#]*(_uki|_image|_kver)=' /etc/mkinitcpio.d/*.preset
   root # ls /etc/pacman.d/hooks/ /usr/share/libalpm/hooks/ | grep -iE 'kernel-install|uki|sbctl|efistub'
   ```

   - Every `*_image=` must point into `/boot`, such as `/boot/initramfs-linux.img`, and every `*_kver=` that names a
     file (`ALL_kver="/boot/vmlinuz-linux"`) must be in `/boot` as well. Change any path on the ESP (`/efi/...`,
     `/boot/efi/...`) to the same name in `/boot`.
   - No `*_uki=` line may be active. A UKI contains the initramfs, and therefore, after the next section, the key
     file, and is written to the ESP. Put a `#` in front of such lines. The UKIs already on the ESP were built without
     the key file and can stay for the old boot loader.
   - If a pacman hook runs `kernel-install`, or copies kernels to the ESP for EFISTUB or systemd-boot, disable it, or
     change its target to `/boot`. Such hooks are not part of a default Arch installation; you would have added them.

7. Copy every installed kernel to the new `/boot` and build its initramfs there, with the same script that
   mkinitcpio's pacman hook runs after a kernel update. It works on the installed kernels and does not install or
   update any package:

   ```
   root # (cd / && printf '%s\n' usr/lib/modules/*/vmlinuz | /usr/share/libalpm/scripts/mkinitcpio install)
   ```

   The script skips module directories without a `pkgbase` file (kernels not installed by a package), copies each
   kernel to the path its preset names (`/boot/vmlinuz-linux` by default) and runs `mkinitcpio -p` for it. Do not use
   `pacman -S linux` for this: it installs the version in the sync database, which may be newer than the installed
   one, and that is a kernel update (see the warning above). If dracut builds your initramfs ([Way C](#way-c-dracut)),
   this script does not exist; copy the kernels and build the images the way your own hook does.

   This initramfs has no key file yet; it is rebuilt in
   [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs).

Old copies of the kernel, the initramfs and microcode images stay on the ESP (or on the old partition). Leave them:
they are what the old boot loader starts.

**Check:**

```
root # findmnt -n -o SOURCE -T /boot
root # findmnt -n -o SOURCE "${ESP_MNT}"
root # findmnt --verify
root # ls -l /boot
root # ls "${ESP_MNT}"
```

- `/boot` belongs to `${ROOT_DS}` (a ZFS dataset), the ESP is mounted at `${ESP_MNT}`, and `findmnt --verify`
  reports no error.
- `/boot` holds `vmlinuz-linux` and `initramfs-linux.img`, and the same for every other kernel package
  (`vmlinuz-linux-lts` and `initramfs-linux-lts.img`, ...). Installations with an older preset also have
  `initramfs-linux-fallback.img`; ZFSBootMenu does not offer it, it pairs each kernel with `initramfs-<name>.img`.
- The ESP still has `EFI/` and the old boot loader's files.

### Several LUKS containers

A pool on several containers (a mirror, RAIDZ, or separate special or log devices) works the same way, with these
differences:

- the key file is added to **every** container (one key slot in each);
- ZFSBootMenu gets every container's UUID in `zbm.luks=`, separated by commas;
- Arch's initramfs must unlock all of them, which the `encrypt` hook cannot do: you are using way B or C.

When all containers share the passphrase, ZFSBootMenu asks once and tries that passphrase on the others before asking
again ([How it works](../../general/luks.rst#how-it-works)). Hot spares and cache devices on LUKS are found by the path
recorded in the pool: they show as `UNAVAIL` in ZFSBootMenu unless the mapping has the same name as on the running
system ([crypttab](../../general/luks.rst#crypttab)).

### Swap

Swap must not cause a second prompt either. Look at it:

```
root # swapon --show
root # grep -v '^#' /etc/crypttab
```

- **Swap in its own LUKS container with a passphrase**, opened through `/etc/crypttab` by the running system: add the
  key file from the next section to that container too, and put the key file's path in the third field of its line in
  `/etc/crypttab`. The key file is readable on the running system, so systemd uses it without asking.
- **Swap with a random key** (`/dev/urandom` and the `swap` option in `/etc/crypttab`) asks for nothing; keep it.
- **Swap on a zvol** is known to deadlock under memory pressure in OpenZFS; consider removing it.

Resuming from hibernation needs the swap container opened in the initramfs before the root, and is not covered by this
article.

**Check:** after the next boot (with either boot loader), `systemctl --failed` lists no `systemd-cryptsetup@` unit and
no passphrase is asked for swap.

## Unlocking a second time with a key file

### Creating the key file

Create a random 64-byte key file and add it to every container. cryptsetup asks for the current passphrase each time:

```
root # install -d -m 0700 /etc/cryptsetup-keys.d
root # install -m 0400 /dev/null "${KEY_FILE}"
root # dd if=/dev/urandom of="${KEY_FILE}" bs=64 count=1
root # for p in ${LUKS_PARTS}; do
  cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 "${p}" "${KEY_FILE}"
done
```

A passphrase needs a slow, memory-hard key derivation (Argon2id, the LUKS2 default) because people choose guessable
passphrases. A key of 512 random bits cannot be guessed, however fast each attempt is, so `--pbkdf pbkdf2
--pbkdf-force-iterations 1000` (the smallest count cryptsetup accepts) makes the second unlock nearly instant without
weakening anything. The passphrase slot keeps its own settings.

**Check:**

```
root # ls -l "${KEY_FILE}"
root # for p in ${LUKS_PARTS}; do cryptsetup open --test-passphrase --verbose --key-file "${KEY_FILE}" "${p}"; done
root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, cryptsetup reports `Key slot N unlocked.` for every container, and the
dump lists one more key slot than before.

### Keeping kernels and the key file off the ESP

Before the key file goes into any initramfs, make sure again that nothing writes an initramfs, or a unified kernel
image (UKI) containing one, to the ESP. Step 6 of
[Moving /boot into the root dataset](#moving-boot-into-the-root-dataset) explains what to change; run its checks once
more now, in case a package update or another tool added a preset line or a hook since:

```
root # grep -nE '^[^#]*(_uki|_image|_kver)=' /etc/mkinitcpio.d/*.preset
root # ls /etc/pacman.d/hooks/ /usr/share/libalpm/hooks/ | grep -iE 'kernel-install|uki|sbctl|efistub'
```

**Check:** the `grep` of the presets shows no active `_uki=` line, and no `_image=` or file-path `_kver=` outside
`/boot`; no hook copies kernels or images to the ESP.

### Way A: busybox initramfs (encrypt and zfs hooks)

This is the classic Arch setup for a pool in **one** container. The `encrypt` hook reads `cryptdevice=` and opens the
container; with `cryptkey=rootfs:<path>` it first tries a key file found at that path inside the initramfs, and asks
for the passphrase only if the key does not work. The archzfs `zfs` hook then imports the pool and mounts the dataset
named by `zfs=`.

Add the key file to the initramfs in a drop-in of its own, which is easy to find and to remove later:

```
root # install -d /etc/mkinitcpio.conf.d
root # echo "FILES+=(${KEY_FILE})" > /etc/mkinitcpio.conf.d/90-zbm-key.conf
```

**FILE** `/etc/mkinitcpio.conf.d/90-zbm-key.conf`

```
FILES+=(/etc/cryptsetup-keys.d/zroot.key)
```

`+=` adds the file to the `FILES` of `/etc/mkinitcpio.conf` and of earlier drop-ins. If a preset has an active
`*_config=` line (see [Which initramfs you have](#which-initramfs-you-have)), the drop-in is ignored: put the line
into that file instead.

Then check the order of `HOOKS` (in `/etc/mkinitcpio.conf`, or in the drop-in that sets it): `encrypt` must come
before `zfs`, and both after `block`:

**FILE** `/etc/mkinitcpio.conf` — *HOOKS*

```
HOOKS=(base udev autodetect microcode modconf kms keyboard keymap consolefont block encrypt zfs filesystems)
```

Keep any other hooks you have; the `HOOKS` line shown is a common working example, not a requirement.

- `microcode` puts the CPU microcode at the front of the initramfs. ZFSBootMenu loads only `initramfs-linux.img`, so
  microcode that your old boot loader loads as a separate initrd (`/boot/intel-ucode.img` or `/boot/amd-ucode.img`)
  is not loaded by ZFSBootMenu: add the `microcode` hook if it is missing, and keep `intel-ucode` or `amd-ucode`
  installed.
- `keyboard` (and `keymap` for a non-US layout) is only needed now for the fallback prompt.
- `fsck` is not needed for ZFS.

The kernel parameters for these hooks are set in [The command line for the system](#the-command-line-for-the-system):

| Parameter | Read by | Meaning |
|---|---|---|
| `cryptdevice=UUID=<UUID>:<name>[:allow-discards]` | `encrypt` | the container, and the mapping name to create |
| `cryptkey=rootfs:/etc/cryptsetup-keys.d/zroot.key` | `encrypt` | the key file, inside the initramfs |
| `zfs=zroot/ROOT/arch` | `zfs` | added by ZFSBootMenu (see the root prefix below) |
| `rw` | `zfs` | import the pool read-write; the hook imports it read-only otherwise |

Keep the mapping name you use today (`${LUKS_NAME}`): the pool's cache file, which the `zfs` hook copies into the
initramfs, records the vdev path below `/dev/mapper`.

### Way B: systemd initramfs (sd-encrypt)

With the `systemd` hook, `sd-encrypt` opens the containers through systemd-cryptsetup, from a crypttab in the
initramfs or from `rd.luks.*` kernel parameters. Add the key file to the initramfs with the same drop-in as in way A:

```
root # install -d /etc/mkinitcpio.conf.d
root # echo "FILES+=(${KEY_FILE})" > /etc/mkinitcpio.conf.d/90-zbm-key.conf
```

Then tell systemd-cryptsetup to use it, the same way you name the containers today.

**With a crypttab in the initramfs.** Current mkinitcpio copies the lines of `/etc/crypttab` that have
`x-initrd.attach` in their fourth field into the initramfs. Older versions use `/etc/crypttab.initramfs` instead; the
current version still reads that file and warns that it is deprecated. `mkinitcpio -H sd-encrypt` says which one
yours reads. Put the key file in the third field of each container's line, with the mapping names you use today:

**FILE** `/etc/crypttab` (or `/etc/crypttab.initramfs`)

```
# <name>   <device>                                   <key file>                        <options>
cryptzfs0  UUID=11111111-2222-3333-4444-555555555555  /etc/cryptsetup-keys.d/zroot.key  luks,discard,x-initrd.attach
cryptzfs1  UUID=66666666-7777-8888-9999-000000000000  /etc/cryptsetup-keys.d/zroot.key  luks,discard,x-initrd.attach
```

Leave out `x-initrd.attach` in `/etc/crypttab.initramfs`, and `discard` if you do not pass TRIM through today.

**Without a crypttab.** systemd's cryptsetup generator reads the containers from the kernel command line
([Unlocking without a crypttab](../../general/luks.rst#unlocking-without-a-crypttab)). For each container, the
command line in the next section gets:

```
rd.luks.name=<UUID>=<name> rd.luks.key=<UUID>=/etc/cryptsetup-keys.d/zroot.key rd.luks.options=<UUID>=discard
```

The `rd.luks.*` forms are used only in the initramfs; the `luks.*` forms that [luks.rst](../../general/luks.rst) uses
apply to the initramfs and to the running system. `rd.luks.crypttab=no` makes the initramfs ignore an embedded
crypttab.

**The ZFS hook.** The archzfs `zfs` hook is a busybox hook: it does not run in an initramfs built with the `systemd`
hook. A systemd initramfs needs a systemd-compatible ZFS hook, which archzfs does not ship. The AUR has
`mkinitcpio-sd-zfs` (hook `sd-zfs`); this article could not verify its current state, its kernel parameters or its
compatibility with the current mkinitcpio. You use whatever hook boots your system today: read its parameters with
`mkinitcpio -H <hook>`, and keep its root parameter in the form your `/proc/cmdline` shows today, through the root
prefix described in the next section.

### Way C: dracut

If the system's initramfs is built by dracut instead of mkinitcpio (see the Arch Wiki's *Dracut* page), the setup is
the one of the Gentoo guide in this repository
([Configuring the initramfs](../gentoo/zfs-luks-v4.md#configuring-the-initramfs)):
a dracut configuration with `add_dracutmodules+=" zfs crypt "`, `install_items+=" /etc/cryptsetup-keys.d/zroot.key "`
and `hostonly_cmdline="no"`, and `luks.uuid=` and `luks.key=` on the command line. archzfs's `zfs-utils` installs the
OpenZFS dracut module. dracut's ZFS module expects the root as `root=zfs:POOL/ROOT/NAME`, so set the root prefix to
`root=zfs:` (next section). Arch has no pacman hook that runs dracut for the system by default; you set up your own
when you switched. This article has not tested way C on Arch.

ZFSBootMenu pairs the kernel `/boot/vmlinuz-linux` with `/boot/initramfs-linux.img` (or `/boot/initramfs-linux`), so
the dracut image must be written under that name, next to the kernel. Your hook does that already if the old boot
loader found the image today; otherwise, a build for every installed kernel package, with the kernel copied first:

```
root # for f in /usr/lib/modules/*/pkgbase; do
  kver="$(basename "$(dirname "${f}")")"; base="$(cat "${f}")"
  install -m 0644 "/usr/lib/modules/${kver}/vmlinuz" "/boot/vmlinuz-${base}"
  dracut --force "/boot/initramfs-${base}.img" "${kver}"
done
```

### The command line for the system

ZFSBootMenu starts Arch's kernel with the property `org.zfsbootmenu:commandline` of the boot environment (inherited
from `${BE_PARENT}`), and adds the root argument and the host ID itself. Build the property from the current command
line:

```
root # cat /root/cmdline-before.txt
```

| In today's command line | In the property |
|---|---|
| `BOOT_IMAGE=...` (GRUB), `initrd=...` (systemd-boot, EFISTUB) | **leave out**: they belong to the old boot loader |
| `root=...`, `zfs=...` | **leave out**: ZFSBootMenu adds the root argument for the boot environment it boots |
| `spl_hostid=...`, `spl.spl_hostid=...` | **leave out**: ZFSBootMenu sets them (and removes them from the property) |
| `cryptdevice=...` (way A) | **keep**, and add `cryptkey=rootfs:/etc/cryptsetup-keys.d/zroot.key` |
| `rd.luks.*`, `luks.*` (ways B and C) | keep, and add the key as shown above if you do not use a crypttab |
| `rw` | **keep**; the `zfs` hook needs it |
| `quiet`, `loglevel=`, `zfs.zfs_arc_max=`, `resume=`, `console=`, GPU and other driver options | keep as needed |
| `zfs_force=1` | leave out: ZFSBootMenu exports the pool before it starts the kernel |

For way A, with one container:

```
root # zfs set org.zfsbootmenu:commandline="rw quiet cryptdevice=UUID=${LUKS_UUID}:${LUKS_NAME} \
cryptkey=rootfs:${KEY_FILE}" "${BE_PARENT}"
```

Add `:allow-discards` after `${LUKS_NAME}` if today's `cryptdevice=` has it, and any other options you keep. With no
property at all, ZFSBootMenu would use `quiet loglevel=4`, which lacks everything above.

**The root prefix.** ZFSBootMenu writes the root argument as a prefix followed by the boot environment's name. For
Arch (`ID=arch` in `/etc/os-release`, also Artix and CachyOS) the default prefix is `zfs=`, which the archzfs `zfs`
hook reads ([org.zfsbootmenu:rootprefix](../../man/zfsbootmenu.7.rst)). The archzfs hook also accepts `root=ZFS=...`
and `root=zfs` (the pool's `bootfs`). Set the property only if your initramfs needs another form, for example for
dracut (way C):

```
root # zfs set org.zfsbootmenu:rootprefix="root=zfs:" "${BE_PARENT}"
```

The prefix is everything in today's root argument before the pool's name: `zfs=` for `zfs=zroot/ROOT/arch`,
`root=zfs:` for `root=zfs:zroot/ROOT/arch`. In images with the Rust runtime it must be one argument: a name, `=`, and
the start of a value, with no spaces or quotes.

**Check:**

```
root # zfs get -o name,value,source org.zfsbootmenu:commandline,org.zfsbootmenu:rootprefix "${ROOT_DS}"
```

The command line is inherited from `${BE_PARENT}` and contains your unlock parameters with the key file's path, and
`rw`. It contains no `root=`, `zfs=`, `BOOT_IMAGE=` or `initrd=`.

### Rebuilding and checking the initramfs

mkinitcpio sets its umask to `077` before it writes an image, so a new image is readable by root only. Confirm that
the installed version does, then rebuild every preset:

```
root # grep -n 'umask 077' /usr/bin/mkinitcpio
root # mkinitcpio -P
```

With dracut (way C), run the build of [Way C](#way-c-dracut) or your own hook instead of `mkinitcpio -P`.

**Check** (mkinitcpio, ways A and B):

```
root # ls -l /boot/initramfs-*.img
root # lsinitcpio /boot/initramfs-linux.img | grep -E 'cryptsetup-keys\.d|crypttab|bin/cryptsetup$|zpool\.cache|zfs\.ko'
root # ls "${ESP_MNT}"
```

**Check** (dracut, way C):

```
root # ls -l /boot/initramfs-*.img
root # lsinitrd /boot/initramfs-linux.img | grep -E 'cryptsetup-keys\.d|crypttab|bin/cryptsetup$|zfs\.ko|etc/hostid'
root # ls "${ESP_MNT}"
```

- Every image is `-rw-------` and owned by root. If one is not, run `chmod 0600` on it, and find out why: mkinitcpio
  rewrites an existing image in place, keeping its old mode, when the file system has too little free space for a
  temporary copy (less than 1.25 times the image's size), and other tools may write it with other modes. Check the
  mode again after the next kernel update.
- The listing contains the key file (`etc/cryptsetup-keys.d/zroot.key`), `cryptsetup` and `zfs.ko`; for way A, also
  `etc/zfs/zpool.cache.org` if the pool uses `/etc/zfs/zpool.cache`; for way B with a crypttab, `etc/crypttab`.
- The ESP gained no new kernel, initramfs or UKI.

The old boot loader still starts the old kernel and initramfs from the ESP, which ask for the passphrase as before.

## Installing ZFSBootMenu

### Dependencies

```
root # pacman -S --needed git base-devel rust efibootmgr kexec-tools fzf cryptsetup dracut
```

| Package | Why |
|---|---|
| `git` | fetch the source |
| `base-devel` | `make`; binutils: `strip` (`make install`), `strings` (kernel versions), `objcopy` (checks) |
| `rust` | `cargo` and `rustc` for the Rust programs: `zbm`, `brush`, `generate-zbm`, `zbm-kcl` |
| `efibootmgr` | firmware boot entries |
| `kexec-tools` | `kexec`, required in every image |
| `fzf` | the menu of the shell runtime (not needed for the Rust runtime, which has no `fzf`) |
| `cryptsetup` | LUKS support; a dracut build without it leaves LUKS out **silently** |
| `dracut` | builds ZFSBootMenu's image; also provides `lsinitrd` |

**Rust.** The build is offline: Cargo uses the crates vendored in the repository. `rust-toolchain.toml` selects Rust
1.97.0, which `rustup` downloads on first use; Arch's `rust` package works as well if it is 1.88 or later
(`rustc --version`). `make install` builds the programs again as the user who runs it, so root must have `cargo` on
its `PATH` ([Build and install](../../general/building.rst#build-and-install)). Arch's `rust` package installs
`/usr/bin/cargo`, which root has. With `rustup`, the toolchain belongs to the user who installed it; run
`rustup default stable` as root as well, or install `rust` instead (the two packages conflict).

**dracut beside mkinitcpio.** Installing `dracut` does not replace mkinitcpio for the system: Arch's kernel packages
call mkinitcpio through mkinitcpio's own pacman hook, and `generate-zbm` calls dracut only with ZFSBootMenu's
configuration directory, `/etc/zfsbootmenu/dracut.conf.d` (never `/etc/dracut.conf.d`). To make sure that the
`dracut` package brings no hook of its own that would rebuild the system's initramfs, look at its files:

```
root # pacman -Ql dracut | grep -E 'libalpm|kernel/install.d'
```

A `libalpm` hook would run on every kernel update; a `kernel/install.d` plugin runs only if something calls
`kernel-install`, which a default Arch installation does not.

**The EFI stub.** `generate-zbm` builds the EFI image around systemd's stub,
`/usr/lib/systemd/boot/efi/linuxx64.efi.stub` (`EFI.Stub` in the configuration names another one). Find the package
that owns it:

```
root # ls -l /usr/lib/systemd/boot/efi/linuxx64.efi.stub
root # pacman -Qo /usr/lib/systemd/boot/efi/linuxx64.efi.stub
```

If the file is missing, `pacman -Fy` and then `pacman -F /usr/lib/systemd/boot/efi/linuxx64.efi.stub` name the
package that has it. This article could not confirm the package's name on current Arch.

**Optional, for hardware keys and the Rust runtime.** The build adds each unlock method only when its tools are
installed on this system ([LUKS and hardware keys](../../general/building.rst#luks-and-hardware-keys)):

| Method | Arch packages (check the names with `pacman -Si`) |
|---|---|
| TPM2 | `tpm2-tools` (systemd's `systemd-tpm2` token plugin comes with `systemd`) |
| FIDO2 (YubiKey 5, Nitrokey 3) | `libfido2` |
| OpenPGP card (YubiKey, Nitrokey) | `gnupg`; `pcsclite`, `ccid` for readers GnuPG's driver does not handle |
| YubiKey challenge-response | `yubikey-personalization` (`ykchalresp`); with the Rust runtime only a fallback (1) |
| clevis | `clevis`, `jose`, `luksmeta` |
| keyboard layouts of the Rust runtime | `xkeyboard-config`, `libx11` |

(1) Images with the Rust runtime talk to the YubiKey over `hidraw` themselves, and use `ykchalresp` only where the
kernel has no `hidraw` or the key cannot be opened that way
([YubiKey challenge-response](../../general/luks.rst#yubikey-challenge-response)).

**Check:**

```
root # cargo --version && rustc --version
root # command -v kexec fzf cryptsetup dracut lsinitrd strip strings objcopy efibootmgr
```

Every program is found, and `rustc` is 1.88 or later.

### Building and installing

Remove upstream ZFSBootMenu first if it is installed (from the AUR, for example); its files would be overwritten or
would conflict:

```
root # pacman -Qs zfsbootmenu
```

Clone the branch with the LUKS support, build, and install:

```
root # git clone --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2.git \
  /usr/local/src/zfsbootmenu
root # cd /usr/local/src/zfsbootmenu
root # make
root # make install
```

> **Warning**
> The repository's default branch, `master`, holds the upstream layout without the LUKS code. Always use
> `claude/zbm-refactor`. Do not run `make core dracut`: it installs neither `zbm` nor `generate-zbm`.

`make` runs `cargo build --release --locked` for `zbm`, `brush`, `generate-zbm` and `zbm-kcl`. `make install` puts:

- `generate-zbm` and `zbm-kcl` in `/usr/bin`;
- `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
- the shell runtime in `/usr/share/zfsbootmenu`;
- the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in `/usr/lib/dracut/modules.d`, and the mkinitcpio
  hooks `zfsbootmenu` and `zfsbootmenu-measured` (with `zfsbootmenu-measured.service`) in `/usr/lib/initcpio`;
- `config.yaml`, `mkinitcpio.conf` and `dracut.conf.d/` in `/etc/zfsbootmenu`;
- manual pages, and the `contrib/` hooks, `examples/` and `splash.bmp` in `/usr/share/examples/zfsbootmenu`.

> **Warning**
> `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the files it
> installs into `/etc/zfsbootmenu/dracut.conf.d/` (`zfsbootmenu.conf`, `omit-drivers.conf`). Your own files with other
> names in `dracut.conf.d/` are kept. Before every later `make install`, save your `config.yaml`, as shown in
> [Updating ZFSBootMenu](#updating-zfsbootmenu).

The two mkinitcpio hooks have different jobs:

- `zfsbootmenu` builds **ZFSBootMenu's own image**, and only when you build that with mkinitcpio. Never add it to
  the system's `/etc/mkinitcpio.conf`.
- `zfsbootmenu-measured` belongs in the **system's** initramfs, and only if you use measured boot with the Rust
  runtime: it keeps ZFSBootMenu's TPM event log in `/run/log/zfsbootmenu/` of the booted system (see
  [Optional next steps](#optional-next-steps)). Without measured boot, leave it out.

**Check:**

```
root # git -C /usr/local/src/zfsbootmenu branch --show-current
root # generate-zbm --showver
root # ls -l /usr/libexec/zfsbootmenu/
root # ls /usr/lib/dracut/modules.d/ | grep zfsbootmenu
```

The branch is `claude/zbm-refactor`, `generate-zbm` prints a version, and `zbm`, `brush` and both dracut modules are
installed.

## Configuring generate-zbm

### config.yaml key by key

Write the configuration. The heredoc fills in the ESP's mount point and the containers' UUIDs:

```
root # ZBM_LUKS="$(for p in ${LUKS_PARTS}; do printf 'UUID=%s,' "$(cryptsetup luksUUID "${p}")"; done)"
root # ZBM_LUKS="${ZBM_LUKS%,}"
root # cp /usr/share/examples/zfsbootmenu/splash.bmp /etc/zfsbootmenu/splash.bmp
root # cat > /etc/zfsbootmenu/config.yaml <<EOF
Global:
  ManageImages: true
  BootMountPoint: ${ESP_MNT}
  DracutConfDir: /etc/zfsbootmenu/dracut.conf.d
  PreHooksDir: /etc/zfsbootmenu/generate-zbm.pre.d
  PostHooksDir: /etc/zfsbootmenu/generate-zbm.post.d
  InitCPIOConfig: /etc/zfsbootmenu/mkinitcpio.conf
Components:
  Enabled: false
EFI:
  ImageDir: ${ESP_MNT}/EFI/zbm
  Versions: false
  Enabled: true
  SplashImage: /etc/zfsbootmenu/splash.bmp
Kernel:
  Prefix: vmlinuz
  CommandLine: "ro quiet loglevel=0 zbm.luks=${ZBM_LUKS}"
EOF
root # cp -a /etc/zfsbootmenu/config.yaml /root/zbm-config.yaml.keep
```

Every key is described in [generate-zbm(5)](../../man/generate-zbm.5.rst):

| Key | Value | Meaning |
|---|---|---|
| `Global.ManageImages` | `true` | `generate-zbm` does nothing while this is `false`, the installed default |
| `Global.BootMountPoint` | `${ESP_MNT}` | mounted by `generate-zbm` if needed (from fstab), unmounted again |
| `Global.DracutConfDir` | `/etc/zfsbootmenu/dracut.conf.d` | the image's dracut settings; never `/etc/dracut.conf.d` |
| `Global.PreHooksDir`, `PostHooksDir` | `.../generate-zbm.pre.d`, `.../post.d` | run before and after a build |
| `Global.InitCPIOConfig` | `/etc/zfsbootmenu/mkinitcpio.conf` | used only with `Global.InitCPIO: true` |
| `Components.Enabled` | `false` | no separate kernel and initramfs, only the EFI image |
| `EFI.ImageDir` | `${ESP_MNT}/EFI/zbm` | where the EFI image is written |
| `EFI.Versions` | `false` | one image and one backup, with fixed names for fixed firmware entries |
| `EFI.Enabled` | `true` | build the EFI image (kernel, initramfs and command line in one file) |
| `EFI.SplashImage` | `/etc/zfsbootmenu/splash.bmp` | logo shown by systemd's stub; a missing file is skipped silently |
| `Kernel.Prefix` | `vmlinuz` | names the images `vmlinuz.EFI`, `vmlinuz-backup.EFI` (see below) |
| `Kernel.CommandLine` | see below | the command line of **ZFSBootMenu's** kernel, not of Arch's |

`Kernel.CommandLine` holds ZFSBootMenu's options ([zfsbootmenu(7)](../../man/zfsbootmenu.7.rst)):

- `zbm.luks=UUID=<UUID>[,UUID=<UUID>...]` names the containers to unlock, by the UUID of the **LUKS container**
  (`cryptsetup luksUUID`), not the pool's GUID or a file system UUID. Left out, ZFSBootMenu discovers every LUKS volume
  itself, unlocks those you give a passphrase for, and closes again any that holds no ZFS vdev
  ([Choosing which volumes to unlock](../../general/luks.rst#choosing-which-volumes-to-unlock)). Naming the containers
  keeps it from asking about unrelated ones, such as a data disk or a swap container.
- `zbm.timeout=<seconds>` changes the countdown before `bootfs` boots (`-1` always shows the menu).
- `loglevel=7` shows every `cryptsetup` call during start-up, for troubleshooting.

**Which kernel the image uses.** By default `generate-zbm` takes the kernel with the highest version among
`/boot/vmlinuz*`, reading the version from the kernel file itself (Arch's file names carry none). The ZFS module for
that kernel must be installed. If you keep `linux-lts` for its ZFS compatibility, pin the image to it by adding
`Path` to the `Kernel` section, beside `Prefix` and `CommandLine`:

```
Kernel:
  Path: /boot/vmlinuz-linux-lts
  Prefix: vmlinuz
  CommandLine: "..."
```

**Check:**

```
root # cat /etc/zfsbootmenu/config.yaml
root # generate-zbm --dry-run --debug
```

The `CommandLine` line contains one `UUID=` per container. The dry run names the kernel it would use, prints a
`Would ...` line for each change, and changes nothing.

### Choosing the runtime

The image runs one of two runtimes ([What you get](../../general/building.rst#what-you-get)):

- the **shell runtime** (`zfsbootmenu_runtime=legacy`), the default, with the `fzf` menu;
- the **Rust runtime** (`zfsbootmenu_runtime=rust`), opt-in, with the new menu. It is required to unlock TPM2 tokens
  that systemd 262 or later enrolled with `--tpm2-with-pin=yes`, which harden the PIN with Argon2id: the shell runtime
  cannot read them. It also has the locked menu, measured boot, YubiKey challenge-response over `hidraw`, and
  keyboard layouts other than US at the passphrase prompt.

For a passphrase alone, either works. To choose the Rust runtime:

```
root # echo 'zfsbootmenu_runtime=rust' > /etc/zfsbootmenu/dracut.conf.d/runtime.conf
```

and add `init_on_free=1` to `Kernel.CommandLine`, so that ZFSBootMenu's kernel zeroes the memory it frees. Your file
`runtime.conf` is not touched by `make install`.

With the shell runtime, the passphrase prompt uses the US keyboard layout: choose characters that are in the same
place on your layout, or use the Rust runtime. With the Rust runtime, either

- add `zbm.console=drm` to `Kernel.CommandLine`, so that ZFSBootMenu runs on its own console on the display, which
  types with your layout; the image needs the `evdev` module and a DRM driver (`simpledrm` on the firmware's
  framebuffer is enough), for example with `add_drivers+=" evdev "` in a file in `/etc/zfsbootmenu/dracut.conf.d/`;
- or, on the default kernel console, press **Ctrl-K** at the prompt to choose a layout first.

The layouts in the image are those of `zfsbootmenu_keyboard_layouts`; when it is not set, the layout of
`XKBLAYOUT` in this system's `/etc/vconsole.conf`, then `us`, `de`, `fr` and `gb`
([zfsbootmenu(7)](../../man/zfsbootmenu.7.rst)).

**Check:** `cat /etc/zfsbootmenu/dracut.conf.d/*.conf` shows `zfsbootmenu_runtime=rust` only if you chose it.

### Building and checking the image

Build twice. The second run renames the first image to `vmlinuz-backup.EFI`, so both firmware entries have a file:

```
root # generate-zbm
root # generate-zbm
```

**Check** the image, without a reboot:

```
root # ls -l "${ESP_MNT}/EFI/zbm/"
root # zbm-kcl "${ESP_MNT}/EFI/zbm/vmlinuz.EFI"
root # objcopy -O binary --only-section=.initrd "${ESP_MNT}/EFI/zbm/vmlinuz.EFI" /run/zbm-initrd.img
root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/cryptsetup$|zfs\.ko|etc/hostid'
root # lsinitrd /run/zbm-initrd.img | grep -E 'cryptsetup-keys\.d|\.key$|etc/zfsbootmenu/crypttab'
root # rm /run/zbm-initrd.img
```

- `vmlinuz.EFI` and `vmlinuz-backup.EFI` exist.
- `zbm-kcl` prints the command line with `zbm.luks=` and your UUIDs.
- The image contains `cryptsetup`, `zfs.ko` and `etc/hostid`.
- The second `grep` prints **nothing**: no key file and no crypttab are in the image. If it prints a line, do not boot
  this image; find out why first.

`zbm-kcl -a ARG` and `-r ARG` change the embedded command line without a rebuild; the next `generate-zbm` replaces it
with `Kernel.CommandLine` again ([Check the result](../../general/building.rst#check-the-result)).

### Alternative: building the image with mkinitcpio

`generate-zbm` can build ZFSBootMenu's image with mkinitcpio instead of dracut
([mkinitcpio](../../general/mkinitcpio.rst)).
Add to `Global`:

```
Global:
  InitCPIO: true
```

It then uses `/etc/zfsbootmenu/mkinitcpio.conf` (not the system's `/etc/mkinitcpio.conf`, and not its drop-ins) and
adds the `zfsbootmenu` hook itself. Build options such as `zfsbootmenu_runtime=rust` go into that file. Never add the
`encrypt` or `sd-encrypt` hook to it: ZFSBootMenu unlocks the containers itself. The `autodetect` hook in its default
`HOOKS` makes the image fit this computer only.

This fork's VM tests do not cover images built with mkinitcpio. Prefer dracut unless you have a reason; check an
mkinitcpio-built image with `lsinitcpio` instead of `lsinitrd`.

## Firmware entries and the first boot

### Secure Boot

The image built here is not signed. With Secure Boot enabled, the firmware refuses to start it. Check the state:

```
root # bootctl status 2>/dev/null | grep -i 'secure boot'
root # od -A n -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa4d-00e098ca253c
```

The last number of the `od` output is `0` when Secure Boot is off. Signing images (`contrib/zbm-sign.pl` with `sbctl`
or `sbsigntools`, [Secure Boot signing](../../general/building.rst#secure-boot-signing)) is not covered here.

### Creating the entries

Create one entry for the image and one for its backup **without** changing the boot order, so that the old boot
loader stays the default:

```
root # efibootmgr --create-only -d "${ESP_DISK}" -p "${ESP_PARTNUM}" \
  -L "ZFSBootMenu (Backup)" -l '\EFI\zbm\vmlinuz-backup.EFI'
root # efibootmgr --create-only -d "${ESP_DISK}" -p "${ESP_PARTNUM}" \
  -L "ZFSBootMenu" -l '\EFI\zbm\vmlinuz.EFI'
root # efibootmgr
```

The ESP is FAT, so path names are not case-sensitive; the separators are backslashes. If your `efibootmgr` does not
know `--create-only` (`-C`), use `-c` instead: it **puts the new entry first** in `BootOrder`. Then restore the old
order, which is in `/root/efibootmgr-before.txt`:

```
root # grep BootOrder /root/efibootmgr-before.txt
root # efibootmgr -o XXXX,YYYY
```

**Check:**

```
root # efibootmgr -v | grep -i zfsbootmenu
root # efibootmgr | grep BootOrder
root # grep BootOrder /root/efibootmgr-before.txt
```

Two entries point at `\EFI\zbm\vmlinuz.EFI` and `\EFI\zbm\vmlinuz-backup.EFI` on the ESP's partition, and
`BootOrder` is the same as before.

### Keeping the old boot loader able to import the pool

The first boot through ZFSBootMenu imports the pool with this system's host ID, and Arch, started by ZFSBootMenu,
imports it with the same ID (see [The host ID](#the-host-id)). Arch does not export the root pool at shutdown, so the
pool's labels then hold that host ID. The old boot loader starts the old initramfs, which the archzfs `zfs` hook
built without `/etc/hostid`: it imports with host ID 0, and `zpool import` refuses the pool ("pool was previously in
use from another system"). The `zfs` hook adds `-f` only with `zfs_force=1`, so the boot stops in the busybox shell.

Do this **before** the first boot through ZFSBootMenu. First look whether the old initramfs has a host ID after all.
Its path is in the old boot loader's configuration; for GRUB with a separate `/boot` partition, mount that partition
read-only first (`mount -o ro /dev/OLD-BOOT-PARTITION /mnt`) and look below `/mnt`:

```
root # ls "${ESP_MNT}"
root # lsinitcpio "${ESP_MNT}/initramfs-linux.img" | grep 'etc/hostid'
```

If it lists `etc/hostid`, and that file has the value `hostid` prints, the old boot loader needs nothing. Otherwise
give the old kernel the host ID on its command line, as ZFSBootMenu does:

```
root # echo "spl.spl_hostid=0x$(hostid)"
```

Append the printed option to the kernel command line of the old boot loader's entry for this system:

- **systemd-boot:** the `options` line of each `${ESP_MNT}/loader/entries/*.conf` that starts this system.
- **rEFInd:** each quoted option string in `refind_linux.conf` next to the old kernel on the ESP, or the `options`
  line of a manual stanza in `refind.conf`.
- **GRUB:** the `linux` lines of `grub/grub.cfg` on the old `/boot` partition (mount it read-write at `/mnt`), or on
  the ESP if `/boot` was the ESP.
- **EFISTUB:** the firmware entry's arguments, which cannot be edited: create a second entry (below).

For GRUB, edit `grub.cfg` by hand. Do not run `grub-mkconfig` now: `/boot/grub` is no longer the directory GRUB
reads, and a later run would drop your edit. For EFISTUB, copy the old entry's arguments from
`/root/efibootmgr-before.txt` (everything after the file path) and create a new entry with the option added, which
you use as the old boot loader from now on:

```
root # efibootmgr --create-only -d "${ESP_DISK}" -p "${ESP_PARTNUM}" -L "Arch Linux (old, host ID)" \
  -l '\vmlinuz-linux' -u 'OLD-ARGUMENTS spl.spl_hostid=0xHOSTID'
```

Replace the loader path with the one of the old entry. A unified kernel image with a built-in command line cannot be
given the option this way; for it, use the one-off way in
[Getting back to the old boot loader](#getting-back-to-the-old-boot-loader) when you need the fallback.

Adding `zfs_force=1` permanently instead would also work, but it turns off the protection against importing a pool
that another system has in use; the host ID keeps it.

**Check:** reboot **with the old boot loader** now, then:

```
root # cat /proc/cmdline
root # printf '%08x\n' "$(cat /sys/module/spl/parameters/spl_hostid)"; hostid
```

The command line contains `spl.spl_hostid=`, and both lines print the same eight hexadecimal digits (not
`00000000`).

### Booting ZFSBootMenu once

`BootNext` makes the firmware start an entry for the next boot only. If ZFSBootMenu fails, a reset returns to the old
boot loader. Use the number of the `ZFSBootMenu` entry (`Boot0003` in `efibootmgr` means `0003`):

```
root # efibootmgr -n 0003
root # reboot
```

What happens:

1. ZFSBootMenu starts and asks to unlock the LUKS volume (once, also for several containers that share the
   passphrase).
2. It imports the pool and shows `${ROOT_DS}` with a countdown. Press **Enter**, or wait. **Escape** during the
   countdown opens the menu (the screen names the keys); in the menu, **Ctrl+R** opens a recovery shell.
3. The screen goes dark for a moment while `kexec` starts Arch's kernel.
4. Arch starts **without** asking for the passphrase again.

**Check** after logging in as root:

```
root # cat /proc/cmdline
root # findmnt -n -o SOURCE /
root # zpool status "${POOL}"
root # cat /sys/module/spl/parameters/spl_hostid; hostid
root # systemctl --failed
root # efibootmgr | grep -E 'BootCurrent|BootOrder'
```

- The command line is the property's, plus `zfs=${ROOT_DS}` (or your prefix) and `spl.spl_hostid=`, and no
  `BOOT_IMAGE=` or `initrd=`.
- `/` is `${ROOT_DS}`, the pool is `ONLINE`, and no unit failed.
- `spl_hostid` is not `0` and matches `hostid` (the first is decimal, the second hexadecimal:
  `printf '%08x\n' $(cat /sys/module/spl/parameters/spl_hostid)`).
- `BootCurrent` is the ZFSBootMenu entry, and `BootOrder` is unchanged: the next reboot starts the old boot loader.

Reboot once more with the old boot loader, to confirm the fallback still works after a boot through ZFSBootMenu:
the system starts without stopping in the busybox shell, and `spl_hostid` is again the system's host ID.

### Making ZFSBootMenu the default

After a few good boots with `BootNext`, put ZFSBootMenu first, its backup second, and the old boot loader after them:

```
root # efibootmgr
root # efibootmgr -o ZBM,BACKUP,OLD
```

Replace `ZBM`, `BACKUP` and `OLD` with the four-digit numbers, and keep any other entries you want after them.

Keep a copy of a known-good image that no run of `generate-zbm` overwrites, with its own entry. The backup image is
replaced by the previous image at every build, so after two bad builds in a row it is bad too:

```
root # cp "${ESP_MNT}/EFI/zbm/vmlinuz.EFI" "${ESP_MNT}/EFI/zbm/known-good.EFI"
root # efibootmgr --create-only -d "${ESP_DISK}" -p "${ESP_PARTNUM}" \
  -L "ZFSBootMenu (known good)" -l '\EFI\zbm\known-good.EFI'
```

Refresh the copy now and then, after a boot with a new image has worked.

**Check:** `efibootmgr | grep BootOrder` starts with the ZFSBootMenu entry; a reboot without `BootNext` starts
ZFSBootMenu.

### Optional: the removable-media fallback path

Some firmware ignores or loses boot entries and starts `\EFI\BOOT\BOOTX64.EFI` instead. Copying the image there helps
on such firmware, but that path is shared: Windows, GRUB (`--removable`), systemd-boot (`bootctl install`) and other
installers may have put their own loader there, and may put it back later. Look first:

```
root # ls -l "${ESP_MNT}/EFI/BOOT/" 2>/dev/null
```

If the directory is empty or missing, or holds a loader you no longer need:

```
root # mkdir -p "${ESP_MNT}/EFI/BOOT"
root # cp "${ESP_MNT}/EFI/zbm/vmlinuz.EFI" "${ESP_MNT}/EFI/BOOT/BOOTX64.EFI"
```

The copy is not updated by `generate-zbm`; repeat the `cp` after updates, or add it as a post-hook.

## Keeping it working

The image contains the kernel, the ZFS module and tools, cryptsetup and the other unlock tools of **this system at
the time of the build**. Rebuild it after updates of the ZFS packages, of the kernel the image uses, of cryptsetup or
the hardware-key tools, and after changing `/etc/zfsbootmenu/`. A ZFSBootMenu with an older ZFS than the pool's
features may not import the pool: rebuild **before** `zpool upgrade` or `zpool set compatibility=...`.

Arch's own initramfs needs nothing new: the kernel packages rebuild it with the key file through mkinitcpio's pacman
hook, as before.

### Rebuilding the image automatically

A pacman hook rebuilds the image after the transactions that need it:

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
Target = tpm2-tools
Target = libfido2
Target = gnupg

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
- The other targets are packages whose files end up in the image: `systemd` supplies the EFI stub and the
  cryptsetup token plugins, `dracut` builds the image, `kexec-tools` and (for the shell runtime) `fzf` are inside it.
  Add the packages of any other tool your image includes, such as `yubikey-personalization` or `clevis`. `systemd`
  updates often; each one costs a rebuild of a few seconds to a minute.
- `When = PostTransaction` hooks run in the order of their file names. `95-` runs after mkinitcpio's
  `90-mkinitcpio-install.hook` and after DKMS's hooks (check their numbers with `ls /usr/share/libalpm/hooks/`), so
  the ZFS module for a new kernel has been built when `generate-zbm` runs.
- Remove the `Target` lines of packages you do not have, or keep them: a trigger for a package that is not involved
  in a transaction does nothing.
- If the build fails (for example because DKMS could not build ZFS for a new kernel), pacman shows `generate-zbm`'s
  error. Look at `ls -l "${ESP_MNT}/EFI/zbm/"` before rebooting, fix the cause, and run `generate-zbm` again; the
  known-good copy (see [Making ZFSBootMenu the default](#making-zfsbootmenu-the-default)) is the safe choice meanwhile.
  If DKMS failed for a new kernel, `Kernel.Path` can pin the image to a kernel that has ZFS.

**Check:** reinstall the **installed** version of `zfs-utils` from pacman's package cache, which triggers the hook
without updating anything. Do not use `pacman -S zfs-utils` for this: it installs the version in the sync database,
which may be newer than the installed one and no longer match the installed `zfs-dkms` or `zfs-linux`.

```
root # ls /var/cache/pacman/pkg/zfs-utils-"$(pacman -Q zfs-utils | cut -d' ' -f2)"-*.pkg.tar.zst
root # pacman -U /var/cache/pacman/pkg/zfs-utils-"$(pacman -Q zfs-utils | cut -d' ' -f2)"-*.pkg.tar.zst
root # ls -l --time-style=full-iso "${ESP_MNT}/EFI/zbm/"
```

The transaction ends with `Rebuilding the ZFSBootMenu image...`; `vmlinuz.EFI` has a new time, and
`vmlinuz-backup.EFI` is the image that was there before. If the cache no longer holds the package (`paccache` or
`pacman -Sc` removed it), skip this check and watch for the message at the next real update of a target package.

### Updating ZFSBootMenu

Save your configuration, update the source, build, install, restore the configuration and rebuild the image:

```
root # cp -a /etc/zfsbootmenu/config.yaml /root/zbm-config.yaml.keep
root # cd /usr/local/src/zfsbootmenu
root # git checkout -- crates/zbm-platform/src/version.rs legacy/zfsbootmenu/zbm-release
root # git status --short
root # git pull
root # make
root # make install
root # cp -a /root/zbm-config.yaml.keep /etc/zfsbootmenu/config.yaml
root # generate-zbm
```

`make install` writes the version into two files of the checkout, `crates/zbm-platform/src/version.rs` and
`legacy/zfsbootmenu/zbm-release`, and `git pull` refuses to run when the update changes either of them. The
`git checkout --` line restores both; `git status --short` must then print nothing. If it still lists files, you
changed them yourself: keep a copy and restore them the same way. `git pull` shows what changed; read `git log`
before building a development branch. If you edited `/etc/zfsbootmenu/mkinitcpio.conf`, save and restore it the same
way as `config.yaml`.

**Check:** `generate-zbm --showver` prints the new version, and the next boot with `BootNext` (as in
[Booting ZFSBootMenu once](#booting-zfsbootmenu-once)) works before you rely on the new image.

### Changing the passphrase

```
root # for p in ${LUKS_PARTS}; do echo "${p}"; cryptsetup luksChangeKey "${p}"; done
```

Use the same new passphrase on every container, so that ZFSBootMenu keeps asking once. The key file's slot is not
affected, and no image needs rebuilding. Make new header backups afterwards: the old ones still accept the old
passphrase.

### Replacing the key file

Create a new key file, add it, test it, and only then remove the old one. The initramfs must be rebuilt, because it
holds a copy of the key file:

```
root # install -m 0400 /dev/null "${KEY_FILE}.new"
root # dd if=/dev/urandom of="${KEY_FILE}.new" bs=64 count=1
root # for p in ${LUKS_PARTS}; do
  cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 "${p}" "${KEY_FILE}.new"
  cryptsetup open --test-passphrase --key-file "${KEY_FILE}.new" "${p}" && \
    cryptsetup luksRemoveKey "${p}" "${KEY_FILE}"
done
root # mv "${KEY_FILE}.new" "${KEY_FILE}"
root # mkinitcpio -P
```

With dracut (way C), rebuild the images as in [Way C](#way-c-dracut) instead of `mkinitcpio -P`. Then repeat the
checks of [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs): every image is
`-rw-------` and contains the new key file.

`luksAddKey` asks for the passphrase. Old initramfs images in snapshots still contain the old key, which no longer
opens anything.

### The old boot loader after kernel updates

The old boot loader starts the kernel and initramfs left on the ESP (or the old `/boot` partition). They stay as they
were, while each kernel update replaces `/usr/lib/modules` with the new version's modules. After the **first kernel
update**, the old kernel finds no modules of its own, and the old boot loader can no longer boot the system.

Copying new kernels and initramfs images back to the ESP is not an option: the new initramfs contains the key file.
Prove ZFSBootMenu before the next kernel update, and rely on the rescue medium after it. Once ZFSBootMenu is the
default and has worked for a while, you can remove the old loader's entry and files (keep
`/root/esp-before-zfsbootmenu.tar`).

## Optional next steps

Each method below works on top of the passphrase, which stays as a fallback. The procedures are in
[luks.rst](../../general/luks.rst#hardware-keys-and-encrypted-key-files) and
[administration.rst](../../luks/administration.rst); rebuild the image after installing the tools a method needs.

- **TPM2.** `systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 <device>` on every container, optionally with
  `--tpm2-with-pin=yes`. Bind to **PCR 7** (Secure Boot state), which stays stable across updates of ZFSBootMenu and
  the kernel; PCRs 4, 8, 9 and 11 change with them. With the Rust runtime, PCR 9 can be added with the value that
  `zbm measure unseal` prints ([Measured boot](../../general/luks.rst#measured-boot)). That needs ZFSBootMenu's event
  log in the booted system: add the `zfsbootmenu-measured` hook to `HOOKS` in `/etc/mkinitcpio.conf` (after
  `filesystems`), run `mkinitcpio -P`, boot through ZFSBootMenu, then run
  `/usr/libexec/zfsbootmenu/zbm measure unseal`. A dracut-built system initramfs (way C) includes the module
  `zfsbootmenu-measured` by default. With systemd 262 and later,
  `--tpm2-with-pin=yes` uses Argon2id: fix its cost with `--tpm2-argon2id-memory=1G --tpm2-argon2id-iterations=3`
  and use the Rust runtime, or enrol with `--tpm2-with-pin=direct` if the shell runtime must open the token
  ([TPM2](../../general/luks.rst#tpm2)). A TPM2 token without a PIN unlocks the pool for anyone who can switch the
  computer on.
- **FIDO2 security keys** (YubiKey 5, Nitrokey 3): `systemd-cryptenroll --fido2-device=auto <device>`; each container
  needs its own touch at boot.
- **YubiKey challenge-response** is configured in `/etc/zfsbootmenu/crypttab` (the option `ykchalresp=`) and needs
  the image to read that crypttab. `zbm.luks=` on the command line takes precedence over the crypttab, so remove
  `zbm.luks=` from `Kernel.CommandLine` when you add one
  ([Choosing which volumes to unlock](../../general/luks.rst#choosing-which-volumes-to-unlock)).
- **GnuPG-encrypted key files on an OpenPGP card** can be named in that crypttab too, or on the command line with
  `zbm.luks_key=/PATH/zroot.key.gpg`; a file named only on the command line must be added to the image with
  `install_items+=" /PATH/zroot.key.gpg "` in a file in `/etc/zfsbootmenu/dracut.conf.d/`
  ([Unlocking without a crypttab](../../general/luks.rst#unlocking-without-a-crypttab)). Only an encrypted key file
  belongs in the image, never a plain one.
- **A key file on a USB stick** for ZFSBootMenu: add `zbm.luks_key=/zroot.key:LABEL=ZROOTKEY` to
  `Kernel.CommandLine`, and `add_drivers+=" usb_storage uas "` to a file in `/etc/zfsbootmenu/dracut.conf.d/`
  ([Using a key file on a USB stick](../../luks/administration.rst#using-a-key-file-on-a-usb-stick)). Anyone with the
  stick and the computer can unlock the pool.
- **TRIM and the dm-crypt work-queue flags** can be stored in each LUKS2 header, so that ZFSBootMenu, the `encrypt`
  hook and systemd apply them without options. On an open container:
  `cryptsetup refresh --allow-discards --perf-no_read_workqueue --perf-no_write_workqueue --persistent <name>`.
  Passing TRIM through reveals which blocks are unused, not their content.

## Troubleshooting

In ZFSBootMenu, **Ctrl+R** (or **Alt+R**) in the menu opens the recovery shell; `exit` returns to the menu. Add
`loglevel=7` to the command line (`zbm-kcl -a loglevel=7 ...`, or press **Ctrl+E** on the boot environment to edit the
command line once) and run `zlogtail` in the recovery shell to see what start-up did.

### ZFSBootMenu shows no pool

When no pool can be imported, ZFSBootMenu keeps retrying; press **Escape** for the recovery shell, then:

```
root # cat /proc/cmdline
root # zluks list
root # zluks
root # exit
```

- If `/proc/cmdline` contains `zbm.luks=`, every value must be the UUID of a **LUKS container**
  (`cryptsetup luksUUID`). A pool GUID or a file system UUID there is the usual mistake. Fix `Kernel.CommandLine` and
  run `generate-zbm`. Without `zbm.luks=` (and without an embedded crypttab), ZFSBootMenu tries every LUKS volume it
  finds.
- If `zluks list` misses a container, the image lacks the disk's driver: add it with `add_drivers+=" ... "` in a file
  in `/etc/zfsbootmenu/dracut.conf.d/`.
- `zluks` unlocks by hand; after `exit` ZFSBootMenu imports the pool.

### The pool is refused: last imported by another system

After a rescue medium imported the pool without exporting it, the pool carries another host ID. ZFSBootMenu's default
import policy (`zbm.import_policy=hostid`) adopts it when it can. Otherwise, in the recovery shell:

```
root # zpool import -f -N POOL
root # exit
```

Once Arch runs, `hostid` and `/etc/hostid` are the system's again. Always export the pool on a rescue medium.

The same refusal in the **old boot loader's** initramfs (the busybox shell after "pool was previously in use from
another system") means that the old kernel's command line lacks `spl.spl_hostid=`; see
[Keeping the old boot loader able to import the pool](#keeping-the-old-boot-loader-able-to-import-the-pool), and
[Getting back to the old boot loader](#getting-back-to-the-old-boot-loader) for the way in this once.

### ZFSBootMenu lists the pool but no kernels

ZFSBootMenu looks only in `/boot` of each boot environment. On the running system:

```
root # findmnt -n -o SOURCE -T /boot
root # ls -l /boot
```

`/boot` must belong to the root dataset and hold `vmlinuz-linux` with `initramfs-linux.img`. If the ESP or another
partition is still mounted at `/boot`, the kernels are there and not in the dataset; see
[Moving /boot into the root dataset](#moving-boot-into-the-root-dataset).

### The image is missing cryptsetup

ZFSBootMenu never asks for a passphrase and keeps retrying the pool import ("Unable to import pool, retrying"). In
its recovery shell, `command -v cryptsetup` prints nothing; with the shell runtime `zluks` is missing too, while with
the Rust runtime `zluks` exists but cannot open the containers. `cryptsetup` was not installed when the image was
built (dracut then leaves LUKS out without a message):

```
root # pacman -S --needed cryptsetup
root # generate-zbm
```

### Arch asks for the passphrase again

Arch's initramfs could not use the key file:

```
root # cat /proc/cmdline
root # zfs get org.zfsbootmenu:commandline "${ROOT_DS}"
root # lsinitcpio /boot/initramfs-linux.img | grep -E 'cryptsetup-keys\.d|crypttab'
```

- Way A: the command line needs `cryptkey=rootfs:` followed by the key file's path **as it is inside the initramfs**
  (the same path as on the system).
- Way B: the crypttab in the initramfs (or `rd.luks.key=`) must name the key file, with the right UUID.
- If the key file is not in the image, check `/etc/mkinitcpio.conf.d/90-zbm-key.conf` (and a preset `*_config=`
  file, which disables the drop-ins) and run `mkinitcpio -P`; with dracut, check `install_items` and rebuild.
- If the passphrase is asked twice in ZFSBootMenu itself, the containers have different passphrases; that is
  expected.

### Arch drops to an emergency shell

The kernel started, but its initramfs could not unlock the containers, import the pool or mount the root. In the
emergency shell:

```
root # cat /proc/cmdline
root # ls /dev/mapper
root # zpool import
```

- No mapping in `/dev/mapper`: the unlock parameters or the key file are wrong (see above).
- The pool is listed by `zpool import` but was not imported: the root argument does not match what the hook reads.
  The archzfs `zfs` hook reads `zfs=...`, `root=ZFS=...` or `root=zfs`; check `org.zfsbootmenu:rootprefix`.
- The root was mounted read-only and the boot stopped later: `rw` is missing from the property.

Reboot, choose the old boot loader in the firmware's boot menu, and fix the property or the initramfs from there.

### Getting back to the old boot loader

- For one boot: the firmware's boot menu (often **F8**, **F11**, **F12** or **Esc** at power-on; it differs between
  vendors), or `efibootmgr -n <old entry>` from a running system.
- If the old initramfs then stops in the busybox shell because the pool "was previously in use from another
  system", either edit the entry once in the old boot loader's menu (GRUB: **e**; systemd-boot: **e**; rEFInd:
  **F2** or **Insert**) and append `spl.spl_hostid=0xHOSTID` (the value `hostid` prints) or `zfs_force=1`, or, in the
  busybox shell, import and mount the root by hand and continue the boot:

  ```
  # zpool import -f -N POOL
  # mount -t zfs -o zfsutil POOL/ROOT/NAME /new_root
  # exit
  ```

  Then add `spl.spl_hostid=` permanently, as in
  [Keeping the old boot loader able to import the pool](#keeping-the-old-boot-loader-able-to-import-the-pool).
- For good: `efibootmgr -o` with the old order from `/root/efibootmgr-before.txt`.
- ZFSBootMenu's recovery shell has `reboot`; in the Rust runtime's menu, `power firmware` reboots into the firmware
  setup.

## Removing ZFSBootMenu

1. Make the old boot loader the default again and delete ZFSBootMenu's entries:

   ```
   root # efibootmgr -o OLD,...
   root # efibootmgr -b ZBM -B
   root # efibootmgr -b BACKUP -B
   root # efibootmgr -b KNOWN -B
   ```

   `ZBM`, `BACKUP` and `KNOWN` are the four-digit numbers of the entries `ZFSBootMenu`, `ZFSBootMenu (Backup)` and
   `ZFSBootMenu (known good)`.

2. Remove the image, the pacman hook and the property:

   ```
   root # rm -r "${ESP_MNT}/EFI/zbm"
   root # rm /etc/pacman.d/hooks/95-zfsbootmenu.hook
   root # zfs inherit org.zfsbootmenu:commandline "${BE_PARENT}"
   root # zfs inherit org.zfsbootmenu:rootprefix "${BE_PARENT}"
   ```

   Remove `${ESP_MNT}/EFI/BOOT/BOOTX64.EFI` only if you copied ZFSBootMenu there.

3. Remove the installed files (the list in [Building and installing](#building-and-installing)):
   `/usr/bin/generate-zbm`, `/usr/bin/zbm-kcl`, `/usr/libexec/zfsbootmenu`, `/usr/share/zfsbootmenu`,
   `/usr/lib/dracut/modules.d/90zfsbootmenu*`, `/usr/lib/initcpio/hooks/zfsbootmenu*`,
   `/usr/lib/initcpio/install/zfsbootmenu*`, `/usr/lib/initcpio/zfsbootmenu-measured.service`, `/etc/zfsbootmenu`,
   `/usr/share/examples/zfsbootmenu`, and the manual pages it installed. If you added `zfsbootmenu-measured` to
   `HOOKS` of the system's `/etc/mkinitcpio.conf`, remove it there **first** and run `mkinitcpio -P`; otherwise the
   next build fails on the missing hook. To see every path `make install` creates, install into an empty staging
   directory and list it: `make install DESTDIR=/tmp/zbm-root && find /tmp/zbm-root`.

4. If the old boot loader is to boot this system for good, it needs kernels it can read, outside the pool. **First**
   remove the key file from the initramfs (`rm /etc/mkinitcpio.conf.d/90-zbm-key.conf`, or `install_items` with
   dracut) and from the command line of the old boot loader; remove the key slot
   (`cryptsetup luksRemoveKey <device> "${KEY_FILE}"` on every container) and then the file. Only then move the ESP
   (or the old `/boot` partition) back to `/boot` in `/etc/fstab`, mount it, and copy the kernels and build their
   images there without updating any package, as in step 7 of
   [Moving /boot into the root dataset](#moving-boot-into-the-root-dataset):

   ```
   root # (cd / && printf '%s\n' usr/lib/modules/*/vmlinuz | /usr/share/libalpm/scripts/mkinitcpio install)
   ```

   With dracut (way C), use your own hook instead. Keep `spl.spl_hostid=` in the old boot loader's command line, or
   have the initramfs include `/etc/hostid`, so that its host ID matches the one in the pool's labels.

**Check:** `efibootmgr` shows no ZFSBootMenu entry, the old boot loader boots the current kernel, and `lsinitcpio`
of every initramfs on the ESP lists no file below `etc/cryptsetup-keys.d`.

## Security notes

- **The key file in Arch's initramfs** opens the containers. It is safe because the initramfs and
  `/etc/cryptsetup-keys.d` are inside the encrypted pool: nobody can read them without the passphrase. It stops
  being safe the moment an initramfs with the key is written anywhere else: the ESP, an unencrypted partition, a
  backup, or a UKI. Snapshots and `zfs send` streams of the root dataset contain it too; treat them like the pool.
- **The ESP is not encrypted.** Anyone with access to the disk, or root on the system, can replace ZFSBootMenu's
  image with one that records the passphrase. LUKS protects data at rest, for example a stolen powered-off laptop,
  not against someone who can modify the computer and wait for you to type the passphrase. Secure Boot with signed
  images, or measured boot with a TPM, narrows that; neither is set up by this article.
- **ZFSBootMenu's image holds no key.** The build never embeds plain key files and warns when a crypttab names one;
  only GnuPG-encrypted key files are embedded.
- **Hooks run as root** in ZFSBootMenu, before the system starts: `generate-zbm` pre- and post-hooks on this system,
  and user hooks in the image. Install only hooks you have read.
- **The recovery shell and the command-line editor** are available to anyone at the keyboard once the pool is
  unlocked, and before it with a key that unlocks it by itself (TPM2 without PIN, a key stick). The Rust runtime's
  locked menu protects them only for a signed image under Secure Boot
  ([Locked menu](../../general/building.rst#locked-menu)).
- **This is a development branch.** `claude/zbm-refactor` changes often and is not a release. Read what changed before
  updating, keep the backup and known-good entries, and keep the rescue medium.

The threat model of this fork, with what it protects against and what it does not, is in
[docs/dev/threat-model.md](../../dev/threat-model.md).

## See also

- This repository: [ZFS Pools on LUKS Volumes](../../general/luks.rst),
  [Building ZFSBootMenu](../../general/building.rst), [LUKS administration](../../luks/administration.rst),
  [Commands, Tools and Options](../../luks/tools.rst), [UEFI Booting](../../general/uefi-booting.rst),
  [Native Encryption](../../general/native-encryption.rst) (for ZFS native encryption, alone or with LUKS),
  [zfsbootmenu(7)](../../man/zfsbootmenu.7.rst), [generate-zbm(5)](../../man/generate-zbm.5.rst), and the manual pages
  `zfsbootmenu-luks(7)` and `zluks(8)`
- [Gentoo Linux on a LUKS-encrypted ZFS root](../gentoo/zfs-luks-v4.md), a new installation with the same design
- Arch Wiki: *ZFS*, *Install Arch Linux on ZFS*, *dm-crypt/System configuration*, *dm-crypt/Encrypting an entire
  system*, *mkinitcpio*, *EFI system partition*, *EFISTUB*, *Unified Extensible Firmware Interface*, *Dracut*
- [archzfs](https://github.com/archzfs/archzfs) and its [wiki](https://github.com/archzfs/archzfs/wiki)
- [OpenZFS documentation](https://openzfs.github.io/openzfs-docs/)
- [systemd-cryptsetup-generator(8)][sd-cryptsetup-generator] — the `luks.*` and `rd.luks.*` kernel parameters
- [crypttab(5)](https://www.freedesktop.org/software/systemd/man/latest/crypttab.html)

[sd-cryptsetup-generator]: https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html
