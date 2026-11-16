# Adding ZFSBootMenu to an existing LUKS-encrypted ZFS root (Gentoo)

This article is for a Gentoo system that **already runs** from a ZFS pool inside one or more **LUKS2** containers. It
boots today through another boot loader (GRUB, systemd-boot, rEFInd or the firmware's EFI stub), and the
distribution's initramfs asks for the LUKS passphrase. The article switches the system to **ZFSBootMenu** without
reinstalling anything, and keeps the old boot loader as a fallback until ZFSBootMenu has proven itself.

It uses the ZFSBootMenu build from **this repository**, branch `claude/zbm-refactor`. Upstream ZFSBootMenu cannot
unlock LUKS volumes, and this repository's default branch, `master`, does not contain the LUKS support either.

It covers both the **OpenRC** and the **systemd** profile, distribution kernels (`sys-kernel/gentoo-kernel-bin`,
`sys-kernel/gentoo-kernel`) and kernels built by hand. The initramfs of the installed system is built with **dracut**.

> **Important**
> `claude/zbm-refactor` is a development branch. Its automated tests do not boot Gentoo. Keep the old boot loader,
> the rescue medium and the backups described below until ZFSBootMenu has booted the system several times.

## Contents

- [Overview](#overview)
  - [What changes in the boot chain](#what-changes-in-the-boot-chain)
  - [Why the container is unlocked twice](#why-the-container-is-unlocked-twice)
  - [What stays unencrypted](#what-stays-unencrypted)
  - [Conventions](#conventions)
- [Before you start](#before-you-start)
  - [Setting variables](#setting-variables)
  - [Snapshotting the boot environments](#snapshotting-the-boot-environments)
  - [Backing up the LUKS headers and the ESP](#backing-up-the-luks-headers-and-the-esp)
  - [Preparing a rescue medium](#preparing-a-rescue-medium)
- [Checking and fixing the layout](#checking-and-fixing-the-layout)
  - [The root dataset and bootfs](#the-root-dataset-and-bootfs)
  - [The host ID](#the-host-id)
  - [Moving /boot into the root dataset](#moving-boot-into-the-root-dataset)
  - [Several LUKS containers](#several-luks-containers)
  - [Swap](#swap)
- [The second unlock with a key file](#the-second-unlock-with-a-key-file)
  - [Creating the key file](#creating-the-key-file)
  - [genkernel users: switching to dracut](#genkernel-users-switching-to-dracut)
  - [Configuring dracut: the crypttab way](#configuring-dracut-the-crypttab-way)
  - [Alternative on systemd: luks parameters](#alternative-on-systemd-luks-parameters)
  - [The command line of the boot environment](#the-command-line-of-the-boot-environment)
  - [Keeping the initramfs readable by root only](#keeping-the-initramfs-readable-by-root-only)
  - [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs)
- [Installing ZFSBootMenu](#installing-zfsbootmenu)
  - [Dependencies](#dependencies)
  - [Building and installing](#building-and-installing)
- [Configuring and building the image](#configuring-and-building-the-image)
  - [config.yaml](#configyaml)
  - [Choosing the runtime](#choosing-the-runtime)
  - [Building the image](#building-the-image)
- [Firmware entries and the first boot](#firmware-entries-and-the-first-boot)
  - [Creating the entries without changing the boot order](#creating-the-entries-without-changing-the-boot-order)
  - [The test boot](#the-test-boot)
  - [Making ZFSBootMenu the default](#making-zfsbootmenu-the-default)
  - [Optional: the removable-media fallback path](#optional-the-removable-media-fallback-path)
  - [Secure Boot](#secure-boot)
- [Keeping it working](#keeping-it-working)
  - [After ZFS and kernel updates](#after-zfs-and-kernel-updates)
  - [Keeping a known-good image](#keeping-a-known-good-image)
  - [Changing the passphrase](#changing-the-passphrase)
  - [Replacing the key file](#replacing-the-key-file)
  - [Updating ZFSBootMenu](#updating-zfsbootmenu)
- [Optional next steps](#optional-next-steps)
- [Troubleshooting](#troubleshooting)
- [Removing ZFSBootMenu](#removing-zfsbootmenu)
- [Security notes](#security-notes)
- [See also](#see-also)

## Overview

### What changes in the boot chain

Today:

1. The firmware starts GRUB, systemd-boot, rEFInd or the kernel's EFI stub from the ESP.
2. The boot loader loads the kernel and initramfs from the ESP or from an unencrypted `/boot` partition.
3. The initramfs asks for the passphrase, unlocks the container, imports the pool and mounts the root dataset.

With ZFSBootMenu:

1. The firmware starts the ZFSBootMenu image `\EFI\zbm\vmlinuz.EFI` from the ESP.
2. ZFSBootMenu asks for the LUKS passphrase, unlocks the container(s) and imports the pool.
3. It shows the boot environments and loads the kernel and initramfs from `/boot` **inside the pool**.
4. It exports the pool, closes the container (the kernel wipes the volume key) and starts the kernel with `kexec`.
5. The initramfs unlocks the container again with a **key file** it carries, without asking, imports the pool and
   mounts the root dataset.

### Why the container is unlocked twice

ZFSBootMenu is a small Linux system of its own. It boots the installed system with `kexec`, which starts a new kernel
with no open dm-crypt mappings. ZFSBootMenu also closes its mappings on purpose before `kexec`, so that the volume keys
are wiped from memory. The installed system's initramfs therefore has to unlock the container(s) a second time,
exactly as it does today (see `docs/general/luks.rst`, *Booting the system afterwards*).

To avoid typing the passphrase twice, this article adds a second LUKS key slot backed by a random **key file**, and
puts that key file into the installed system's initramfs. That is only safe if the initramfs is stored **inside the
encrypted pool**, where it cannot be read before the passphrase has been typed. This is why `/boot` must become a
directory of the root dataset: kernels and initramfs images then live on ZFS, inside LUKS. ZFSBootMenu reads them from
there.

| Key | Stored where | Used by |
|---|---|---|
| Passphrase (existing slot) | your memory | ZFSBootMenu; the old boot loader; the rescue medium |
| Key file (new slot) | `/etc/cryptsetup-keys.d/` and the initramfs, in the pool | the initramfs, after `kexec` |

### What stays unencrypted

The EFI system partition (ESP) stays unencrypted, as it must. After this article it holds:

- the ZFSBootMenu image and its backup, which contain no key, no crypttab and nothing secret: only a kernel,
  ZFSBootMenu's own initramfs and its command line (which names the LUKS UUID);
- the old boot loader's files, unchanged, as the fallback. The old initramfs on the ESP (or on the old `/boot`
  partition) was built before the key file existed, and this article checks that it stays that way.

A ZFSBootMenu build never copies a plain key file named in its own crypttab (it warns instead), but it does not
check files added with `install_items` or through `/etc/dracut.conf` (see `docs/general/building.rst`, *The
build-time crypttab*). The checks in [Building the image](#building-the-image) confirm that no key file is in the
image.

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run as root on the **installed system**; do not type the prompt |
| `live #` | run as root on the **rescue medium** |
| `zbm #` | run in ZFSBootMenu's **recovery shell** (see [Troubleshooting](#troubleshooting)) |
| `initramfs #` | run in the **emergency shell** of the installed system's initramfs (dracut) |
| **FILE** `/path` | create or edit this file with the content shown |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables); only on the installed system |
| `CAPITALS` | a value you must replace |

Each step ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Before you start

### Setting variables

Look at the current layout first:

```
root # lsblk -o NAME,SIZE,TYPE,FSTYPE,PARTTYPENAME,MOUNTPOINTS
root # zpool list -v
root # zpool status -P
root # zfs list -o name,mountpoint,canmount,mounted
root # findmnt /
root # findmnt /boot; findmnt /boot/efi; findmnt /efi
root # dmsetup ls --target crypt
root # cat /proc/cmdline
```

Note:

- the pool name and the root dataset (`findmnt /` shows it, for example `zroot/ROOT/gentoo`);
- the mapping name of each LUKS container that holds the pool (`zpool status -P` shows `/dev/mapper/<name>`);
- which file system is mounted at `/boot`, and where the ESP is mounted (`PARTTYPENAME` *EFI System*).

Then set the variables. The example has one container; see [Several LUKS containers](#several-luks-containers) for
more:

```
root # export POOL=zroot
root # export BE=gentoo                 # the root dataset is ${POOL}/ROOT/${BE}
root # export LUKS_NAME=luks-YOUR-UUID  # the mapping name shown by zpool status -P
root # export LUKS_PART="$(cryptsetup status "${LUKS_NAME}" | awk '$1 == "device:" { print $2 }')"
root # export LUKS_UUID="$(cryptsetup luksUUID "${LUKS_PART}")"
root # export ESP_PART=/dev/nvme0n1p1    # the ESP, wherever it is mounted today
root # export ESP_DISK="/dev/$(lsblk -no PKNAME "${ESP_PART}")"
root # export ESP_PARTNUM="$(cat "/sys/class/block/$(basename "$(readlink -f "${ESP_PART}")")/partition")"
root # export KEYFILE="/etc/cryptsetup-keys.d/${POOL}.key"
```

`ESP_DISK` and `ESP_PARTNUM` assume an ESP on a plain partition. An ESP mirrored with mdraid has no single disk (`lsblk`
prints `md127` or similar); see `docs/general/mdraid-esp.rst` for the boot entries in that case.

Save them, so that they can be loaded again after a reboot (`source /root/zbm-migration.env`):

```
root # cat > /root/zbm-migration.env <<EOF
export POOL="${POOL}"
export BE="${BE}"
export LUKS_NAME="${LUKS_NAME}"
export LUKS_PART="${LUKS_PART}"
export LUKS_UUID="${LUKS_UUID}"
export ESP_PART="${ESP_PART}"
export ESP_DISK="${ESP_DISK}"
export ESP_PARTNUM="${ESP_PARTNUM}"
export KEYFILE="${KEYFILE}"
EOF
```

**Check:**

```
root # cat /root/zbm-migration.env
root # cryptsetup isLuks --type luks2 "${LUKS_PART}" && echo LUKS2
root # lsblk -o NAME,PARTTYPENAME "${ESP_PART}"
```

Every variable has a value, the container is `LUKS2`, and the ESP's type is *EFI System*. This article, like the rest
of this repository's documentation, assumes LUKS2; `cryptsetup luksDump "${LUKS_PART}"` shows the version.

### Snapshotting the boot environments

A recursive snapshot of the boot environments lets you undo every change this article makes inside the pool
(`/etc/fstab`, `/etc/hostid`, the dracut configuration, the key file):

```
root # zfs snapshot -r "${POOL}/ROOT@before-zfsbootmenu"
```

It does not cover the ESP, the LUKS headers (a key slot added later stays after a rollback; remove it with
`cryptsetup luksRemoveKey`) or the firmware's boot entries; the next steps back those up.

**Check:**

```
root # zfs list -t snapshot -o name,creation | grep before-zfsbootmenu
```

### Backing up the LUKS headers and the ESP

A damaged LUKS header makes the data unrecoverable, even with the right passphrase. Back up the header of every
container, and the ESP's content, to a USB stick or another machine, **not** to this disk. Mount the stick first; the
example uses `/mnt/usb`:

```
root # cryptsetup luksHeaderBackup "${LUKS_PART}" --header-backup-file "/mnt/usb/${LUKS_UUID}.header"
root # tar -C "$(findmnt -no TARGET "${ESP_PART}")" -cpf /mnt/usb/esp-before-zfsbootmenu.tar .
root # efibootmgr -v > /mnt/usb/efibootmgr-before-zfsbootmenu.txt
root # cp /etc/fstab /proc/cmdline /mnt/usb/
```

The last two commands record the firmware's boot entries and the current kernel command line, which you will need to
restore the boot order or to write the new command line.

> **Note**
> A header backup holds the key slots as they are now, and every slot wraps the same volume key. A passphrase or key
> file removed later still opens a container restored from, or combined with, an older backup. Make a new backup
> after you change or remove keys, and destroy the backups that still hold a slot you removed; see
> `docs/luks/administration.rst`, *Backing up LUKS headers*. This first backup is made before the key file's slot
> exists, so it never holds that slot.

**Check:**

```
root # ls -l /mnt/usb/
root # cryptsetup luksDump "/mnt/usb/${LUKS_UUID}.header" | head -n 5
```

### Preparing a rescue medium

If the system no longer boots, you need a live system that can open LUKS and import the pool. It must have
**cryptsetup** and a **ZFS version at least as new as the pool's features**. The Gentoo minimal installation CD has
no ZFS, and many live ISOs that have it ship an older OpenZFS than Gentoo. Compare:

```
root # zfs version
root # zpool get all "${POOL}" | grep -E 'feature@' | grep -v disabled
live # zfs version
```

A live system with an older major version (for example 2.2 against a host on 2.3 or 2.4) may refuse a pool that has
newer features **active**. `zpool import` on the live system lists such a pool with the features it does not support.

Once ZFSBootMenu is built (later in this article), its image is a second rescue path with the same ZFS and LUKS
support as the host and a recovery shell: copy `vmlinuz.EFI` to `\EFI\BOOT\BOOTX64.EFI` on a FAT-formatted USB stick.

Do **not** run `zpool upgrade` while you rely on an older rescue medium, an older ZFSBootMenu image or a boot loader
that reads the pool (GRUB): each of them may then refuse the pool. The `sys-fs/zfs` ebuild warns about this for GRUB.

Test it **before** you change anything: boot it, then open the container and import the pool read-only:

```
live # cryptsetup open /dev/YOUR-LUKS-PARTITION "luks-$(cryptsetup luksUUID /dev/YOUR-LUKS-PARTITION)"
live # zpool import -f -N -o readonly=on -R /mnt/gentoo zroot
live # zfs list zroot/ROOT
live # zpool export zroot
live # cryptsetup close "luks-$(cryptsetup luksUUID /dev/YOUR-LUKS-PARTITION)"
```

`-f` is needed because the pool was not exported when the installed system shut down. Always export the pool before
leaving the live system; otherwise the next import on the installed system may refuse it as *in use by another
system*.

To repair the system later, import it writable and chroot into it:

```
live # cryptsetup open /dev/YOUR-LUKS-PARTITION "luks-$(cryptsetup luksUUID /dev/YOUR-LUKS-PARTITION)"
live # zpool import -f -N -R /mnt/gentoo zroot
live # zfs mount zroot/ROOT/gentoo
live # zfs mount -a
live # mount /dev/YOUR-ESP /mnt/gentoo/boot/efi
live # mount --types proc /proc /mnt/gentoo/proc
live # mount --rbind /sys /mnt/gentoo/sys && mount --make-rslave /mnt/gentoo/sys
live # mount --rbind /dev /mnt/gentoo/dev && mount --make-rslave /mnt/gentoo/dev
live # mount --bind /run /mnt/gentoo/run && mount --make-slave /mnt/gentoo/run
live # chroot /mnt/gentoo /bin/bash
```

Leave with `exit`, then `umount -R -l /mnt/gentoo/{dev,sys,proc,run}`, `umount /mnt/gentoo/boot/efi`, `zfs umount -a`,
`zpool export zroot` and `cryptsetup close`. Use the ESP's mount point of the moment: `/boot/efi` once
[Moving /boot into the root dataset](#moving-boot-into-the-root-dataset) is done, `/boot` before.

**Check:** the live system imported the pool, `zfs list` showed your datasets, and the export succeeded.

## Checking and fixing the layout

### The root dataset and bootfs

ZFSBootMenu treats every dataset with `mountpoint=/` as a boot environment and boots the pool's `bootfs` after a
countdown. `canmount=noauto` keeps `zfs mount -a` from mounting several boot environments on `/`; the initramfs mounts
the right one itself.

```
root # zfs get -o name,property,value mountpoint,canmount "${POOL}/ROOT/${BE}"
root # zfs set canmount=noauto "${POOL}/ROOT/${BE}"
root # zpool set bootfs="${POOL}/ROOT/${BE}" "${POOL}"
```

Setting `canmount=noauto` does not unmount the running root.

- If the root dataset has `mountpoint=legacy` (mounted through `/etc/fstab` or `root=`), leave it and set
  `zfs set org.zfsbootmenu:active=on "${POOL}/ROOT/${BE}"`, which makes ZFSBootMenu show it (see
  `docs/man/zfsbootmenu.7.rst`, *org.zfsbootmenu:active*).
- If the root dataset is not below a `${POOL}/ROOT` container (for example `zroot/gentoo`), that works as well. Do not
  rename a running root. Set the `org.zfsbootmenu:commandline` property described below on the root dataset itself,
  wherever this article says `${POOL}/ROOT`.
- Other datasets with `mountpoint=/`, such as old boot environments, also appear in the menu. Hide one with
  `zfs set org.zfsbootmenu:active=off <dataset>`.

**Check:**

```
root # zfs get -H -o value mountpoint,canmount "${POOL}/ROOT/${BE}"
root # zpool get -H -o value bootfs "${POOL}"
```

The output is `/`, `noauto` and `${POOL}/ROOT/${BE}` (or `legacy` and the `active` property, as above).

### The host ID

ZFS records in the pool the host ID of the system that imported it, and refuses a normal import by another host ID
while the pool was not exported. Three parties import this pool and must agree: the installed system, its initramfs
(dracut's `zfs` module copies `/etc/hostid` into it) and ZFSBootMenu (`generate-zbm` copies `/etc/hostid` into the
image). By default ZFSBootMenu also passes the host ID it used to the booted kernel as `spl.spl_hostid=`, or as
`spl_hostid=00000000` when that host ID is zero, which dracut's `zfs` module turns into an all-zero `/etc/hostid` in
the initramfs (`zbm.set_hostid`, see `docs/man/zfsbootmenu.7.rst`). A non-zero `spl.spl_hostid` makes the ZFS module
of the booted system **ignore** `/etc/hostid` until the next boot. The value ZFS uses is in
`/sys/module/spl/parameters/spl_hostid` (decimal; `0` means it reads `/etc/hostid`).

If `/etc/hostid` exists, keep it. Otherwise, write the host ID that the pool already records, so that nothing changes
for the running system:

```
root # ls -l /etc/hostid
root # hostid
root # zdb -l "/dev/mapper/${LUKS_NAME}" | grep -m 1 -w hostid
```

`zdb -l` prints the recorded ID in decimal, for example `hostid: 2970574860`. Convert it and write the file:

```
root # zgenhostid "$(printf '%08x' 2970574860)"
```

If the label shows no `hostid` line, or `hostid: 0`, keep the value the system reports now: `zgenhostid "$(hostid)"`.
`zgenhostid` refuses to overwrite an existing `/etc/hostid`; do not force it unless you know why the file differs from
the pool.

**Check:**

```
root # hostid
root # printf '%d\n' "0x$(hostid)"
```

The decimal value equals the `hostid` in the pool label (or the label has none).

### Moving /boot into the root dataset

ZFSBootMenu looks for kernels in `/boot` of each boot environment: files named `vmlinuz-*`, `vmlinux-*`, `linux-*`,
`linuz-*` or `kernel-*`, each with an initramfs such as `initramfs-<version>.img` beside it. `/boot` must therefore be
a plain **directory of the root dataset**, not a separate partition or dataset.

Find your case, and keep a copy of `/etc/fstab` to go back to:

```
root # findmnt -no SOURCE,FSTYPE /boot
root # cp /etc/fstab /etc/fstab.before-zfsbootmenu
```

| `/boot` today | Typical loader | What to do |
|---|---|---|
| not a mount point (a directory of the root dataset) | GRUB | only the USE flag changes below |
| the ESP (`vfat`) | systemd-boot, rEFInd, EFI stub | [Case A](#case-a-boot-is-the-esp) |
| `systemd-1 autofs` (an automount; the ESP below it) | systemd-boot | [Case A](#case-a-boot-is-the-esp) |
| an unencrypted partition (`ext4`, `xfs`) | GRUB | [Case B](#case-b-boot-is-a-separate-partition) |
| a separate ZFS dataset or boot pool | GRUB | [Case C](#case-c-boot-is-a-zfs-dataset) |

This article mounts the ESP at **`/boot/efi`**, which matches ZFSBootMenu's default configuration. `/efi` works as
well; then use `/efi` for `Global.BootMountPoint` and `/efi/EFI/zbm` for `EFI.ImageDir` in
[config.yaml](#configyaml).

The old files stay where they are: the old boot loader keeps booting the old kernel and initramfs from the ESP or the
old `/boot` partition. Do **not** update the old loader's configuration from now on (`grub-mkconfig`, `bootctl
update`, `kernel-install` with a BLS layout); the USE flag changes below stop the automatic updates.

> **Warning**
> Do the move **before** you create the key file in the next section. An initramfs built while `/boot` is still the
> ESP or an unencrypted partition would put the key file there.

#### Case A: /boot is the ESP

On the systemd profile, stop the automount first if there is one, or `umount` would be undone at the next access:

```
root # systemctl stop boot.automount 2>/dev/null
root # umount /boot
root # findmnt /boot
root # ls -A /boot
```

`findmnt` prints nothing now. `ls -A` shows what the root dataset holds below the old mount point. It is usually
empty. If it is not, move the files away (`mv /boot /boot.hidden && mkdir /boot`) and look at them later.

```
root # mkdir -p /boot/efi
root # nano -w /etc/fstab        # change the ESP's mount point from /boot to /boot/efi
```

**FILE** `/etc/fstab` — *the ESP's line, for example*

```
UUID=XXXX-XXXX  /boot/efi  vfat  umask=0077  0 2
```

If `/etc/fstab` has no line for the ESP, systemd's `systemd-gpt-auto-generator` mounted it; add the line with the
ESP's UUID (`blkid -s UUID -o value "${ESP_PART}"`). The generator creates no mount for a mount point listed in fstab
or for a directory that contains files, and `/boot` now contains `efi`. Then:

```
root # systemctl daemon-reload 2>/dev/null
root # mount /boot/efi
```

#### Case B: /boot is a separate partition

```
root # umount /boot/efi 2>/dev/null; umount /efi 2>/dev/null
root # umount /boot
root # ls -A /boot
root # mkdir -p /boot/efi
root # nano -w /etc/fstab        # comment out the /boot line; mount the ESP at /boot/efi
root # systemctl daemon-reload 2>/dev/null
root # mount /boot/efi
```

Keep the old `/boot` partition and do not format it: GRUB reads its configuration and the old kernels from it. To look
at it, mount it elsewhere, for example `mount -o ro /dev/OLD-BOOT /mnt/oldboot`.

#### Case C: /boot is a ZFS dataset

ZFSBootMenu does not mount a separate `/boot` dataset or a boot pool (such as `bpool/BOOT/gentoo` of GRUB setups).
Unmount it and keep it from mounting again; GRUB keeps reading it. The kernels are installed into the root dataset's
`/boot` below, so nothing needs to be copied:

```
root # export BOOT_DS="$(findmnt -no SOURCE /boot)"     # for example bpool/BOOT/gentoo
root # umount /boot/efi 2>/dev/null
root # zfs umount "${BOOT_DS}"
root # zfs set canmount=noauto "${BOOT_DS}"
root # ls -A /boot
```

Remove a `/boot` line from `/etc/fstab` if the dataset had `mountpoint=legacy`. Then mount the ESP at `/boot/efi` as in
case A.

**Check** (all cases):

```
root # findmnt --verify --verbose
root # findmnt /boot/efi
```

`findmnt --verify` reports no errors, and `/boot/efi` is the ESP. To go back, copy `/etc/fstab.before-zfsbootmenu`
to `/etc/fstab`.

#### Turning off the old loader's kernel installation

Gentoo installs kernels with `sys-kernel/installkernel`. Its USE flags decide where kernels go:

| USE flag | Effect |
|---|---|
| `grub` | runs `grub-mkconfig` after each installation |
| `systemd-boot` | the BLS layout: kernels and initramfs images go onto the ESP (needs `systemd`) |
| `uki`, `ukify` | builds unified kernel images, which go onto the ESP |
| `efistub` | installs kernels onto the ESP and creates firmware entries for them |
| `refind` | only copies an icon next to the kernel; harmless |
| `dracut`, `ugrd` | the initramfs generator (one of them) |
| `systemd` | which backend installs: systemd's `kernel-install` with it, Gentoo's traditional script without it |

With `grub`, `systemd-boot`, `uki` and `efistub` disabled, installkernel uses the *compat* layout and installs into
`/boot`. Keep `dracut`, which builds the initramfs, and leave `systemd` as your profile sets it.

```
root # emerge --pretend --verbose sys-kernel/installkernel
```

**FILE** `/etc/portage/package.use/installkernel`

```
sys-kernel/installkernel dracut -grub -systemd-boot -uki -ukify -efistub -ugrd
```

If GRUB today reads `/boot` from the encrypted root dataset itself (the first row of the case table), you may keep
`grub`: `grub-mkconfig` then keeps the fallback's menu current.

installkernel writes `/usr/lib/kernel/install.conf` from its USE flags. Both backends read
`/etc/kernel/install.conf` instead when it exists, so a copy there pins the layout even if the USE flags change later:

**FILE** `/etc/kernel/install.conf`

```
# Kernels and initramfs images go to /boot on ZFS, never onto the ESP: the initramfs will hold the key file
layout=compat
initrd_generator=dracut
uki_generator=none
```

This file replaces `/usr/lib/kernel/install.conf` as a whole. Compare it with `cat /usr/lib/kernel/install.conf`
after the rebuild below and keep any other line you need.

```
root # emerge --ask --oneshot --changed-use sys-kernel/installkernel
```

> **Note**
> When `sys-apps/systemd[boot]` or `sys-apps/systemd-utils[boot]` is installed (this article installs it for the EFI
> stub) or `sys-boot/grub` is, installkernel prints that the `systemd-boot` (or `grub`) USE flag *should* be enabled.
> Ignore that message here: `systemd-boot` would install the initramfs with the key file onto the ESP.

On the **systemd** profile (installkernel with USE `systemd`), kernels are installed by systemd's `kernel-install`,
and Gentoo's `90-compat.install` plugin names them `kernel-<version>`. On **OpenRC** (USE `-systemd`), the
traditional script names them `vmlinuz-<version>`. Both put `initramfs-<version>.img` beside them; the traditional
script also `config-<version>` and `System.map-<version>`. It first renames files of a version that is installed again
to `*.old` (`vmlinuz-<version>.old`, `initramfs-<version>.img.old`, ...). ZFSBootMenu finds no initramfs for a `.old`
kernel and leaves it out of the menu, and `generate-zbm` skips it; see [Security notes](#security-notes) for why the
`.old` initramfs files matter later.

#### Reinstalling the kernels

Check the USE flags that decide what goes into the initramfs first:

```
root # zfs version
root # emerge --pretend --verbose --nodeps sys-fs/zfs sys-kernel/gentoo-kernel-bin
root # emerge --pretend --verbose --nodeps sys-fs/zfs-kmod      # only with ZFS 2.3 or older
```

- `sys-fs/zfs` and, with ZFS 2.3 or older, `sys-fs/zfs-kmod` have `rootfs`, `initramfs` and, with distribution
  kernels, `dist-kernel`. From ZFS 2.4 on, `sys-fs/zfs` builds the kernel modules itself (USE `modules`) and
  `sys-fs/zfs-kmod` is no longer installed. With `-initramfs`, the package writes `omit_drivers` for `zfs` into
  `/usr/lib/dracut/dracut.conf.d/`, which removes `zfs.ko` from **every** initramfs dracut builds, the ZFSBootMenu
  image included.
- The distribution kernel has `initramfs` and not `generic-uki`. `generic-uki` installs a prebuilt generic initramfs
  instead of one built by dracut on this machine (it requires installkernel without `dracut`). Such an initramfs
  cannot contain the key file.

```
root # cat /usr/lib/dracut/dracut.conf.d/10-zfs*.conf
```

It shows `add_drivers+=" zfs ... "`, not `omit_drivers`.

Then install the kernel(s) again, so that kernel and initramfs land in `/boot` on ZFS:

- Distribution kernels: `emerge --config` needs one exact version when several are installed. Run it for each:

  ```
  root # for p in $(portageq match / sys-kernel/gentoo-kernel-bin); do emerge --config "=${p}"; done
  ```

  Use `sys-kernel/gentoo-kernel` if that is the kernel package.
- Kernels built by hand: `make -C /usr/src/linux install`, which runs `installkernel`. Without USE `dracut` on
  `installkernel`, build the initramfs yourself: `dracut --force /boot/initramfs-<version>.img <version>`.

The initramfs built now has no key file yet; that comes in the next section. It is still a complete initramfs that
asks for the passphrase, as before.

Keep the kernel version you are running installed until ZFSBootMenu is proven: the old boot loader can only boot
kernels whose modules are still in `/lib/modules`. For distribution kernels, `emerge --noreplace
=sys-kernel/gentoo-kernel-bin-<version>` keeps that version from being removed by `emerge --depclean`.

**Check:**

```
root # findmnt /boot
root # ls -l /boot
root # ls /boot/efi
root # find /lib/modules/"$(uname -r)" -name 'zfs.ko*'
root # tail -n 3 /var/log/installkernel.log
```

- `findmnt /boot` prints nothing (it is a directory of the root dataset).
- `/boot` holds `kernel-<version>` (systemd profile) or `vmlinuz-<version>` (OpenRC) and `initramfs-<version>.img` for
  the running kernel.
- `/boot/efi` holds the old loader's files and no new kernel or initramfs.
- `zfs.ko` exists for the running kernel.
- The log's `LAYOUT` column says `compat` and its `BOOT_ROOT` column `/boot`.

### Several LUKS containers

A pool on several containers (a mirror, a RAIDZ, a special vdev) works the same way. Repeat the steps for each:

- back up every header;
- add the **same** key file to every container, one key slot each (in
  [Creating the key file](#creating-the-key-file));
- list every container in the initramfs configuration (`rd.luks.uuid=` or `luks.uuid=`) and in `zbm.luks=`.

Use the **same passphrase** on all of them: ZFSBootMenu tries a passphrase that worked on the remaining containers
before it asks again, so you type it once (`docs/general/luks.rst`, *How it works*). `cryptsetup luksChangeKey` aligns
them.

`docs/luks/administration.rst` defines a helper, `pool_luks_devices`, that prints the LUKS partitions behind a pool.
The loops in this article can use it:

```
root # pool_luks_devices() {
  local vdev
  for vdev in $( zpool status -LP "$1" | awk '$1 ~ "^/dev/" { print $1 }' ); do
    [ -e "/sys/class/block/${vdev##*/}/dm/uuid" ] || continue
    grep -q '^CRYPT-' "/sys/class/block/${vdev##*/}/dm/uuid" || continue
    echo "/dev/$( ls "/sys/class/block/${vdev##*/}/slaves" )"
  done
}
root # pool_luks_devices "${POOL}"
```

Cache (L2ARC) and spare vdevs are found by the path recorded in the pool, so they must be opened under the same mapping
name as on the running system (`docs/general/luks.rst`, *crypttab*). Data, log and special vdevs are found by their
labels whatever the mapping is called.

### Swap

Swap must not ask for a passphrase a second time:

- **Swap encrypted with a random key** (a crypttab or `/etc/conf.d/dmcrypt` line with `/dev/urandom`) asks for
  nothing. Leave it.
- **Swap in a LUKS container with a passphrase** asks again at every boot. Add the key file of the next section to it
  as well (`cryptsetup luksAddKey` with the same options), and name the key file in the running system's
  configuration for it: `/etc/crypttab` on systemd, `/etc/conf.d/dmcrypt` on OpenRC. The swap container does not need
  to go into the initramfs unless you resume from hibernation.
- **Swap on a zvol** is not encrypted by LUKS separately and asks for nothing.

This article does not cover hibernation. If `/proc/cmdline` contains `resume=`, find out whether hibernation is in
use before you copy that parameter in
[The command line of the boot environment](#the-command-line-of-the-boot-environment).

**Check:**

```
root # swapon --show
root # dmsetup ls --target crypt
```

## The second unlock with a key file

### Creating the key file

Create a random 64-byte key file and add it to the container. `cryptsetup` asks for the passphrase:

```
root # install -d -m 0700 /etc/cryptsetup-keys.d
root # install -m 0400 /dev/null "${KEYFILE}"
root # dd if=/dev/urandom of="${KEYFILE}" bs=64 count=1
root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 "${LUKS_PART}" "${KEYFILE}"
```

For several containers, run the last command for each (`for p in $(pool_luks_devices "${POOL}"); do ...; done`).

Why a fast key derivation is safe here: a passphrase needs a slow, memory-hard derivation (LUKS2 uses Argon2id by
default) because people choose guessable passphrases. The key file holds 512 random bits; nobody can guess it, however
fast each guess is. `--pbkdf pbkdf2 --pbkdf-force-iterations 1000` therefore costs no security and makes the second
unlock almost instant. The passphrase slot keeps its slow default.

**Check:**

```
root # ls -l "${KEYFILE}"
root # cryptsetup open --test-passphrase --verbose --key-file "${KEYFILE}" "${LUKS_PART}"
root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The file is `-r--------` and owned by root, `cryptsetup` reports `Key slot N unlocked.` for the new slot, and the
passphrase slot is still listed.

### genkernel users: switching to dracut

This article uses **dracut** for the installed system's initramfs. genkernel's initramfs (`crypt_root=`, `dozfs`,
`root_key=`) reads keys from a separate device and has no way, described here, to carry a key file inside the
initramfs. To switch:

```
root # emerge --ask sys-kernel/dracut
```

Enable USE `dracut` on `sys-kernel/installkernel` (see
[Turning off the old loader's kernel installation](#turning-off-the-old-loaders-kernel-installation)), rebuild it, and
install the kernel again as in [Reinstalling the kernels](#reinstalling-the-kernels). Kernels built by hand no longer
need `genkernel initramfs`; `make install` builds the initramfs through `installkernel`. The genkernel parameters
(`crypt_root=`, `dozfs`, `real_root=`, `root_key=`, `root_keydev=`) are not carried over to the new command line.

`root=ZFS=<dataset>`, genkernel's form, is understood by dracut's `zfs` module as well, so the
[root prefix](#the-command-line-of-the-boot-environment) needs no change.

### Configuring dracut: the crypttab way

This way works on **both profiles**. An `/etc/crypttab` in the initramfs names the key file, and an `rd.luks.uuid=`
parameter on the command line names the containers to open:

- **OpenRC** (no systemd in the initramfs): dracut's `crypt` module starts its unlock script only for the containers
  named by `rd.luks.uuid=` (or for every LUKS volume with `rd.auto=1`, which is off by default). The script then looks
  up the container's line in `/etc/crypttab` for the mapping name and the key file, and reads the key file from inside
  the initramfs. Without `rd.luks.uuid=`, the crypttab alone opens nothing.
- **systemd**: `systemd-cryptsetup-generator` in the initramfs reads `/etc/crypttab`. When `rd.luks.uuid=` is given,
  it opens only the listed containers and still takes the name, key file and options from their crypttab lines; other
  lines (swap, data disks whose keys are not in the initramfs) are ignored in the initramfs. `rd.luks.*` parameters
  are read only in the initramfs, so the running system reads `/etc/crypttab` as before.

The `rd.luks.uuid=` parameter goes into the boot environment's command line in
[The command line of the boot environment](#the-command-line-of-the-boot-environment). Use the mapping name the
system uses today (`${LUKS_NAME}`), so that nothing else changes on the running system.

**FILE** `/etc/crypttab`

```
# <name>               <device>              <key file>                         <options>
luks-YOUR-LUKS-UUID    UUID=YOUR-LUKS-UUID   /etc/cryptsetup-keys.d/zroot.key   luks
```

To write the line from the variables:

```
root # printf '%s UUID=%s %s luks\n' "${LUKS_NAME}" "${LUKS_UUID}" "${KEYFILE}" >> /etc/crypttab
```

Write the device as `UUID=` with the LUKS UUID, as above: dracut's script on OpenRC matches the line by that form. If
`/etc/crypttab` already has a line for this container, change that line instead of adding a second one.

If TRIM is passed through today (`cryptsetup status "${LUKS_NAME}"` shows `discards` under `flags`) and the flag is not
stored in the header, add `discard` to the options on systemd. dracut's script on OpenRC does not know `discard`; it
takes `allow-discards` instead. Storing the flag in the LUKS2 header works for both, and for ZFSBootMenu; see
[Optional next steps](#optional-next-steps).

**FILE** `/etc/dracut.conf.d/10-zfs-luks.conf`

```
# ZFS root, and the crypt module that unlocks the container
add_dracutmodules+=" zfs crypt "
# The crypttab and the key file it names
install_items+=" /etc/cryptsetup-keys.d/zroot.key /etc/crypttab "
# NVMe only: host-only detection does not always see through ZFS and LUKS to the disk
add_drivers+=" nvme "
```

Use your key file's path in `install_items`. Leave out `add_drivers` if the disks are not NVMe. In host-only mode,
dracut copies only the crypttab lines of the devices the root needs; otherwise it copies the whole file. With
`rd.luks.uuid=` on the command line, other lines are not opened in the initramfs either way.

> **Warning**
> Put this file in `/etc/dracut.conf.d/`, never in `/etc/dracut.conf`. `generate-zbm` builds the ZFSBootMenu image
> with its own configuration directory, `/etc/zfsbootmenu/dracut.conf.d`, instead of `/etc/dracut.conf.d`, but dracut
> still reads the main file `/etc/dracut.conf` and the packages' directory `/usr/lib/dracut/dracut.conf.d/`. A key file
> named in either would end up in the ZFSBootMenu image on the unencrypted ESP.

On OpenRC, `/etc/crypttab` is not read by the running system (OpenRC uses `/etc/conf.d/dmcrypt`), so the line only
affects the initramfs. On systemd, the running system reads it too, finds the container already open, and leaves it.

### Alternative on systemd: luks parameters

With the **systemd** profile, the initramfs runs systemd, whose `systemd-cryptsetup-generator` also reads the
`luks.*` kernel parameters. This is the way `docs/guides/gentoo/zfs-luks-v4.md` uses, without any crypttab:

**FILE** `/etc/dracut.conf.d/10-zfs-luks.conf`

```
# Do not store detected rd.luks.* options in the initramfs; the parameters below are used instead
hostonly_cmdline="no"
add_dracutmodules+=" zfs crypt "
install_items+=" /etc/cryptsetup-keys.d/zroot.key "
add_drivers+=" nvme "
```

The parameters go into the boot environment's command line (next section):

```
luks.crypttab=no luks.uuid=YOUR-LUKS-UUID luks.key=YOUR-LUKS-UUID=/etc/cryptsetup-keys.d/zroot.key
```

The container is then opened as `luks-<UUID>`. For several containers, repeat `luks.uuid=` and `luks.key=` for each.

The `luks.*` forms are read by the **running** system too (systemd-cryptsetup-generator(8)):

- `luks.crypttab=no` makes it ignore `/etc/crypttab`;
- `luks.uuid=` alone already restricts it: when `/etc/crypttab` exists, only the UUIDs on the command line are opened.

So if `/etc/crypttab` lists other volumes, such as swap or a data disk, use the forms that only the initramfs reads:
`rd.luks.crypttab=no`, `rd.luks.uuid=` and `rd.luks.key=`. Or use the crypttab way above.

On **OpenRC** there is no systemd in the initramfs, and the `luks.*` parameters do nothing. dracut's own
`rd.luks.key=<path>[:<device>]` names a key file on a device that dracut mounts or searches, not one inside the
initramfs; a key file inside the initramfs is read only through the key file field of `/etc/crypttab`. Use the
crypttab way on OpenRC.

### The command line of the boot environment

ZFSBootMenu does not use the old boot loader's command line. It passes the value of the ZFS property
`org.zfsbootmenu:commandline` to the kernel it boots, and adds `root=` and, by default, `spl.spl_hostid=` itself. Set
the property on `${POOL}/ROOT`, so that every boot environment below it inherits it.

Start from the current command line:

```
root # cat /proc/cmdline
```

| Parameter in `/proc/cmdline` | Copy it? |
|---|---|
| `BOOT_IMAGE=`, `initrd=` | no: set by GRUB or the EFI stub loader |
| `root=`, `real_root=`, `rootfstype=` | no: ZFSBootMenu adds `root=` |
| `rd.luks.uuid=` | replace it with the pool's containers (crypttab way, below) |
| `rd.luks.key=`, `rd.luks.name=`, `rd.luks.options=`, `luks.*` | no: replaced by the new configuration |
| `crypt_root=`, `root_key=`, `root_keydev=`, `dozfs` | no: genkernel's |
| `spl_hostid=`, `spl.spl_hostid=` | no: set by ZFSBootMenu |
| `ro`, `rw` | not needed; the initramfs mounts the ZFS root |
| `quiet`, `splash`, `console=`, `loglevel=`, `mitigations=`, `init=`, `rd.vconsole.*` | yes, if you want them |
| driver options such as `nvidia-drm.modeset=1`, `i915.*`, `amdgpu.*` | yes, if you want them |
| `resume=` | only if hibernation is known to work here (see [Swap](#swap)) |

With the crypttab way (both profiles), name every container of the pool with `rd.luks.uuid=`:

```
root # zfs set org.zfsbootmenu:commandline="quiet rd.luks.uuid=${LUKS_UUID}" "${POOL}/ROOT"
```

For several containers, repeat `rd.luks.uuid=<UUID>` for each.

With the kernel-parameter way (systemd only):

```
root # zfs set org.zfsbootmenu:commandline="quiet luks.crypttab=no luks.uuid=${LUKS_UUID} \
luks.key=${LUKS_UUID}=${KEYFILE}" "${POOL}/ROOT"
```

Replace `quiet` with the parameters you keep. Do not put `root=` into the property.

**The root prefix.** ZFSBootMenu builds the `root=` argument from a prefix and the dataset name. For a boot
environment whose `/etc/os-release` says `ID=gentoo`, the default prefix is `root=ZFS=`, which gives
`root=ZFS=zroot/ROOT/gentoo` (`docs/man/zfsbootmenu.7.rst`, *org.zfsbootmenu:rootprefix*). dracut's `zfs` module
understands both `root=ZFS=` and `root=zfs:`, so nothing needs to be set. Set the property only if the boot environment
does not identify as Gentoo or its initramfs expects another form:

```
root # zfs set org.zfsbootmenu:rootprefix="root=zfs:" "${POOL}/ROOT"
```

**Check:**

```
root # zfs get -o name,value,source org.zfsbootmenu:commandline,org.zfsbootmenu:rootprefix "${POOL}/ROOT/${BE}"
```

The command line is inherited from `${POOL}/ROOT` and contains `rd.luks.uuid=` (crypttab way) or `luks.uuid=` and
`luks.key=` (kernel-parameter way) with the LUKS UUID; `rootprefix` is `-` unless you set it.

### Keeping the initramfs readable by root only

The initramfs now contains the key file. Anyone who can read the initramfs file can read the key. Both backends of
`sys-kernel/installkernel` install the initramfs into `/boot` with mode **`0644`** (readable by every user). They
also refresh a version-less copy, `/boot/initramfs.img`, when one already exists (common after GRUB setups), and the
traditional backend keeps the previous build of a version as `initramfs-<version>.img.old`. A small plugin makes
every initramfs file in `/boot` readable by root only after every kernel installation, including the initramfs
rebuilds that Portage triggers after ZFS updates.

The plugin directory depends on the backend of `sys-kernel/installkernel`:

| `installkernel` USE | Backend | Plugin directory | Rules |
|---|---|---|---|
| `systemd` (systemd profile default) | systemd's `kernel-install` | `/etc/kernel/install.d/` | ends in `.install` |
| `-systemd` (OpenRC default) | Gentoo's `installkernel` script | `/etc/kernel/postinst.d/` | must be executable |

Plugins run in the order of their names, shipped ones from `/usr/lib/kernel/`. The initramfs is built before this
plugin runs: by `52-dracut.install` in `install.d` (systemd) or in `preinst.d` (traditional). A name starting with
`95` runs after Gentoo's `90-compat.install` and `90-zz-update-static.install`. Look at what your version ships:

```
root # ls /usr/lib/kernel/install.d/ /usr/lib/kernel/preinst.d/ /usr/lib/kernel/postinst.d/ 2>/dev/null
```

**FILE** `/etc/kernel/install.d/95-initramfs-mode.install` (or `/etc/kernel/postinst.d/95-initramfs-mode.install`)

```
#!/bin/sh
# The initramfs images in /boot hold the LUKS key file: make them readable by root only.
# kernel-install passes "add" or "remove" first; the traditional installkernel passes the version.
[ "${1}" = "remove" ] && exit 0
for f in /boot/initramfs*; do
	[ -f "${f}" ] && [ ! -L "${f}" ] && chmod 0600 "${f}"
done
exit 0
```

```
root # mkdir -p /etc/kernel/install.d          # or /etc/kernel/postinst.d
root # nano -w /etc/kernel/install.d/95-initramfs-mode.install
root # chmod 0755 /etc/kernel/install.d/95-initramfs-mode.install
```

The plugin changes every initramfs file in `/boot` (`initramfs-<version>.img`, `initramfs.img`, `*.old`), not only
the new one, so it does not depend on the arguments the backend passes. It skips symbolic links; their targets are
covered. As a second layer, `chmod 0700 /boot` keeps other users out of the directory as a whole.

### Rebuilding and checking the initramfs

Rebuild the initramfs of every installed kernel:

```
root # for p in $(portageq match / sys-kernel/gentoo-kernel-bin); do emerge --config "=${p}"; done
```

Use `sys-kernel/gentoo-kernel` if that is the kernel package. For a kernel built by hand, run
`make -C /usr/src/linux install` again, or `dracut --force /boot/initramfs-<version>.img <version>` followed by
`chmod 0600 /boot/initramfs-<version>.img` (the plugin does not run for a direct `dracut` call; what mode dracut
itself gives its output was not checked for this article).

**Check:**

```
root # INITRD="/boot/initramfs-$(uname -r).img"
root # find /boot -maxdepth 1 -name 'initramfs*' -type f -perm /077
root # lsinitrd "${INITRD}" | grep -E 'cryptsetup-keys\.d|etc/hostid|zfs\.ko|bin/cryptsetup$|systemd-cryptsetup$'
root # lsinitrd -f etc/crypttab "${INITRD}"
root # lsinitrd -f etc/cmdline.d/20-crypt.conf "${INITRD}"
```

- `find` prints **nothing**: no initramfs file is readable by anyone but root. If it prints a file, check the plugin.
- The `grep` lists the key file, `etc/hostid`, `zfs.ko` and `cryptsetup`. On the systemd profile it also lists
  `systemd-cryptsetup`.
- With the crypttab way, `etc/crypttab` contains your line with the key file, and perhaps lines of other volumes
  (ignored, see above). With the kernel-parameter way it prints nothing.
- `etc/cmdline.d/20-crypt.conf` is what dracut detected in host-only mode (`rd.luks.uuid=luks-<UUID>`), or nothing.
  It is no substitute for the parameter in the property: detection through ZFS does not always work.

Then make sure that **no** file outside the pool contains the key: the ESP, and the old `/boot` partition of case B if
you mounted it at `/mnt/oldboot`. Unified kernel images (`*.efi`) carry an initramfs too, so look at every file that
was written after the key file:

```
root # find /boot/efi /mnt/oldboot -type f -newer "${KEYFILE}" 2>/dev/null
root # find /boot/efi /mnt/oldboot -type f -iname '*init*' 2>/dev/null | while read -r f; do
  lsinitrd "${f}" 2>/dev/null | grep -q 'cryptsetup-keys\.d' && echo "KEY FILE IN ${f}"
done
```

Both print nothing. (Later, after [Building the image](#building-the-image), the first lists ZFSBootMenu's own images
and nothing else.) If either prints a kernel, initramfs or `.efi` file, delete that file (or rebuild it without the
key), remove the key slot with `cryptsetup luksRemoveKey "${LUKS_PART}" "${KEYFILE}"`, and start this section again
with a new key file.

At this point the old boot loader still works: it boots the old kernel and initramfs, which ask for the passphrase as
before. You can reboot through it now to make sure, before ZFSBootMenu is installed.

## Installing ZFSBootMenu

### Dependencies

| Package | Why |
|---|---|
| `dev-vcs/git` | to clone this repository |
| `dev-lang/rust-bin` (or `dev-lang/rust`) | `cargo` and `rustc` 1.88 or later, to build the Rust programs |
| `sys-devel/binutils` | `strip` for `make install`, `strings` for `generate-zbm`; always installed |
| `sys-kernel/dracut` | builds the ZFSBootMenu image |
| `sys-apps/kexec-tools` | `kexec`, required in every image |
| `app-shells/fzf` | the menu of the default (legacy) runtime; the Rust runtime does not need it |
| `sys-fs/cryptsetup` | LUKS support: without it on the build host, the image silently has none |
| `sys-boot/efibootmgr` | the firmware's boot entries |
| `sys-block/mbuffer`, `sys-apps/less` | optional, like `column` (util-linux); built without them, with a warning |
| `sys-apps/systemd[boot]` (systemd profile) | the EFI stub `/usr/lib/systemd/boot/efi/linuxx64.efi.stub` |
| `sys-apps/systemd-utils[boot]` (OpenRC) | the same EFI stub on OpenRC |

`docs/general/building.rst` lists the programs an image needs; ZFS, util-linux, kmod, procps and ncurses are already
installed on a ZFS root system. The Perl modules named by older guides are not needed: `generate-zbm` is a Rust
program on this branch. Only the optional Secure Boot signing hook, `contrib/zbm-sign.pl`, is a Perl script (see
[Secure Boot](#secure-boot)).

On the **systemd** profile, the crypttab way and the `luks.*` parameters need `systemd-cryptsetup` in the initramfs,
which `sys-apps/systemd` builds only with USE `cryptsetup`. Check with `emerge --pretend --verbose sys-apps/systemd`.

After `sys-apps/systemd[boot]` or `sys-apps/systemd-utils[boot]` is installed, `sys-kernel/installkernel` may print
that its `systemd-boot` USE flag should be enabled. Ignore it; see
[Turning off the old loader's kernel installation](#turning-off-the-old-loaders-kernel-installation).

**FILE** `/etc/portage/package.use/zfsbootmenu`

```
# The EFI stub for the ZFSBootMenu image: keep the line for your profile
sys-apps/systemd boot
#sys-apps/systemd-utils boot
```

```
root # emerge --ask --oneshot --changed-use sys-apps/systemd     # or sys-apps/systemd-utils on OpenRC
root # emerge --ask --noreplace dev-vcs/git dev-lang/rust-bin sys-kernel/dracut sys-apps/kexec-tools \
  app-shells/fzf sys-fs/cryptsetup sys-boot/efibootmgr sys-block/mbuffer
```

For the Rust runtime, the keyboard layouts are compiled from `x11-misc/xkeyboard-config` and the compose rules of
`x11-libs/libX11`; without them the build only warns. For hardware keys, see
[Optional next steps](#optional-next-steps).

**Check:**

```
root # cargo --version
root # rustc --version
root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
root # command -v kexec fzf cryptsetup dracut efibootmgr strings strip
```

`cargo` and `rustc` are 1.88 or later. If `cargo` is not found, `dev-lang/rust-bin` is installed in a slot that is
not selected: `eselect rust list` and `eselect rust set <number>`. `rust-toolchain.toml` in the repository names
Rust 1.97.0, but only `rustup` reads that file; Gentoo's `cargo` builds with its own version.

### Building and installing

Clone the branch with the LUKS support and build as root. The Rust dependencies are vendored in the repository, so
the build needs no network after the clone:

```
root # git clone --branch claude/zbm-refactor \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2.git /usr/src/zbm
root # make -C /usr/src/zbm
root # make -C /usr/src/zbm install
```

`make` runs `cargo build --release --locked`. `make install` writes the version into the tree and builds the programs
again, as the user who runs it, so root needs `cargo` on its `PATH`; on Gentoo `/usr/bin/cargo` is. Use `make install`,
not `make core dracut`: the latter installs neither `zbm` nor `generate-zbm`.

`make install` puts:

- `generate-zbm` and `zbm-kcl` in `/usr/bin`;
- `zbm` and `brush` in `/usr/libexec/zfsbootmenu`;
- the legacy runtime in `/usr/share/zfsbootmenu`;
- the dracut modules `90zfsbootmenu` and `90zfsbootmenu-measured` in `/usr/lib/dracut/modules.d`, and mkinitcpio hooks
  in `/usr/lib/initcpio`;
- `config.yaml`, `mkinitcpio.conf` and `dracut.conf.d/zfsbootmenu.conf` and `dracut.conf.d/omit-drivers.conf` in
  `/etc/zfsbootmenu`;
- the manual pages in `/usr/share/man/man5`, `man7` and `man8`;
- the `contrib/` hooks in `/usr/share/examples/zfsbootmenu/hooks`, and `examples/` and `splash.bmp` in
  `/usr/share/examples/zfsbootmenu`.

> **Warning**
> Every `make install` **overwrites** `/etc/zfsbootmenu/config.yaml`, `/etc/zfsbootmenu/mkinitcpio.conf` and the two
> files it installs in `/etc/zfsbootmenu/dracut.conf.d/`. Files of your own in that directory, such as `runtime.conf`
> below, are kept. Copy `/etc/zfsbootmenu` away before every later install; see
> [Updating ZFSBootMenu](#updating-zfsbootmenu).

The dracut module `zfsbootmenu-measured` is also picked up by the installed system's initramfs from its next rebuild.
It only copies the TPM event log that images with the Rust runtime pass on, and does nothing otherwise.

**Check:**

```
root # generate-zbm --showver
root # ls -l /usr/bin/generate-zbm /usr/bin/zbm-kcl /usr/libexec/zfsbootmenu/
root # ls /usr/lib/dracut/modules.d/ | grep zfsbootmenu
```

## Configuring and building the image

### config.yaml

Write the configuration. Replace the UUID with `${LUKS_UUID}`, or let the shell do it:

```
root # cat > /etc/zfsbootmenu/config.yaml <<EOF
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
  CommandLine: "ro quiet loglevel=0 zbm.prefer=${POOL} zbm.luks=UUID=${LUKS_UUID}"
EOF
root # cp /etc/zfsbootmenu/config.yaml /root/zfsbootmenu-config.yaml
```

The last command keeps a copy that `make install` cannot overwrite. Every key is described in
`docs/man/generate-zbm.5.rst`:

| Key | Meaning |
|---|---|
| `Global.ManageImages` | `generate-zbm` does nothing while this is `false`, the installed default |
| `Global.BootMountPoint` | the ESP; `generate-zbm` mounts it (through `/etc/fstab`) if needed, and unmounts it again |
| `Global.DracutConfDir` | dracut configuration of the **image**; must not be `/etc/dracut.conf.d` |
| `Global.PreHooksDir`, `PostHooksDir` | executables run before and after each build; none are needed here |
| `Global.InitCPIOConfig` | used only with mkinitcpio; harmless here |
| `Components.Enabled: false` | no separate kernel and initramfs files, only the EFI image |
| `EFI.ImageDir` | where the EFI image is written: `\EFI\zbm` on the ESP |
| `EFI.Versions: false` | one image, `vmlinuz.EFI`, and one backup, `vmlinuz-backup.EFI`; the entries never change |
| `EFI.Enabled: true` | build the EFI image |
| `Kernel.Prefix` | fixes the image name; see below |
| `Kernel.CommandLine` | ZFSBootMenu's own command line, embedded in the image |

**`Kernel.Prefix`.** Without it, `generate-zbm` names the image after the kernel file it uses. On the systemd
profile, `installkernel` names kernels `kernel-<version>`, which would give `kernel.EFI` instead of the `vmlinuz.EFI`
the firmware entries below point to. On OpenRC the kernels are `vmlinuz-<version>` already; the setting is harmless
there and keeps the name fixed if the backend ever changes.

**Which kernel the image uses.** `generate-zbm` builds the image around a kernel from the host's `/boot`: the highest
version among `vmlinuz*`, and only if there is none, among `vmlinux*`, `linux*` and then `kernel*`. On the systemd
profile, a leftover `vmlinuz-*` file (or a version-less `vmlinuz` copy) in `/boot` therefore wins over every newer
`kernel-*`; move such files away. A kernel whose file name and embedded version disagree, such as
`vmlinuz-<version>.old`, is skipped with an *inconsistent versions* warning. The ZFS modules for the chosen kernel
must be installed. To pin the running kernel instead, add `Version: "%{current}"` to the `Kernel` section, or run
`generate-zbm --kver "$(uname -r)"`. The pin needs the running kernel's modules to stay installed until the next
reboot, which Portage does until `emerge --depclean`.

**`Kernel.CommandLine`** holds options for ZFSBootMenu, not for the installed system
(`docs/man/zfsbootmenu.7.rst`):

- `zbm.luks=UUID=<LUKS UUID>` unlocks exactly this container, as `luks-<UUID>`, asking for the passphrase. Use the UUID
  of the **LUKS container** (`cryptsetup luksUUID`), not the pool's GUID and not the UUID of the file system inside.
  For several containers, separate them with commas: `zbm.luks=UUID=<a>,UUID=<b>`. The other forms are `PARTUUID=`,
  `LABEL=`, `PARTLABEL=`, `ID=` (`/dev/disk/by-id`), `PATH=` (`/dev/disk/by-path`), a path below `/dev`, a bare LUKS
  UUID and `luks-<UUID>` (`docs/general/luks.rst`, *Choosing which volumes to unlock*).
- Without `zbm.luks=` and without a ZFSBootMenu crypttab, ZFSBootMenu discovers **every** LUKS volume on the machine,
  asks for each (an empty passphrase skips one), and closes again those that hold no ZFS vdev. Listing the containers
  keeps it from asking about data disks and USB drives.
- `zbm.prefer=<pool>` imports this pool first and takes the default boot environment from its `bootfs`. Without it,
  the `bootfs` of the first pool in alphabetical order is used, which matters when other, unencrypted pools are
  attached.
- `zbm.timeout=<seconds>`, `zbm.show` and `zbm.skip` control the countdown.

**Keyboard layout.** Test the passphrase at the first prompt. If it fails and it contains characters that sit
elsewhere on your layout than on a US layout, type it as on a US layout; this article does not confirm which layout
the legacy runtime uses. In the Rust runtime, the image's layouts apply on the console ZFSBootMenu draws itself:
add `zbm.console=drm` to `Kernel.CommandLine` and `add_drivers+=" evdev <your display's DRM driver> "` to a file in
`/etc/zfsbootmenu/dracut.conf.d/`, and choose the layout with `zfsbootmenu_keyboard_layouts` or
`zbm.keyboard_layout=` (`docs/man/zfsbootmenu.7.rst`). The default, `zbm.console=kernel`, keeps the kernel console.

For the passphrase setup of this article, do **not** write a ZFSBootMenu crypttab (`/etc/zfsbootmenu/crypttab`):
`zbm.luks=` overrides it anyway. Never name the plain key file in `zbm.luks_key=` or in `install_items` of
`/etc/zfsbootmenu/dracut.conf.d/`: the image sits on the unencrypted ESP, and the build does not check files named
that way.

### Choosing the runtime

Images run one of two runtimes (`docs/general/building.rst`):

- the **legacy runtime** (Bash, with the `fzf` menu), the default;
- the **Rust runtime** (`zbm`, with a new menu), opt-in. Both unlock LUKS with passphrases, key files, TPM2, FIDO2,
  GnuPG cards and YubiKey challenge-response.

The legacy runtime is enough for a passphrase. Choose the Rust runtime when you need what only it has:

- TPM2 PIN tokens enrolled with systemd 262 or later and `--tpm2-with-pin=yes`, which are hardened with Argon2id: the
  legacy runtime cannot read them (or enroll with `--tpm2-with-pin=direct`; see
  [Optional next steps](#optional-next-steps));
- YubiKey challenge-response without `ykchalresp` (over `hidraw`), measured boot into PCRs 8 and 9, the locked menu,
  keyboard layouts and fonts.

```
root # echo 'zfsbootmenu_runtime=rust' > /etc/zfsbootmenu/dracut.conf.d/runtime.conf
```

With the Rust runtime, also add `init_on_free=1` to `Kernel.CommandLine`, so that the kernel zeroes the memory it
frees. `zbm.menu=classic` keeps the classic menu for this release.

### Building the image

Look at what would happen, then build twice: the second run keeps the first image as `vmlinuz-backup.EFI`, so that
the backup entry below has something to boot from the start:

```
root # generate-zbm --dry-run --debug
root # generate-zbm
root # generate-zbm
```

**Check** the image:

```
root # ls -l /boot/efi/EFI/zbm/
root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
root # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
root # lsinitrd /run/zbm-initrd.img | grep -E 'bin/cryptsetup$|etc/hostid|zfs\.ko|nvme'
root # lsinitrd /run/zbm-initrd.img | grep -E 'cryptsetup-keys\.d|\.key$'
root # lsinitrd -f etc/crypttab /run/zbm-initrd.img
root # lsinitrd -f etc/zfsbootmenu/crypttab /run/zbm-initrd.img
root # rm /run/zbm-initrd.img
```

- `vmlinuz.EFI` and `vmlinuz-backup.EFI` exist.
- The embedded command line contains `zbm.luks=UUID=` and your LUKS UUID.
- The image contains `cryptsetup`, `etc/hostid`, `zfs.ko` and NVMe support (`nvme.ko`, or nothing if the kernel has
  the driver built in).
- The second `grep` and both `lsinitrd -f` commands print **nothing**: no key file and no crypttab are in the image.

If `generate-zbm` fails, see the messages listed in `docs/general/building.rst`, *Troubleshooting*. The usual ones on
Gentoo are *Unable to find UEFI stub loader* (USE `boot` missing) and *failed to install essential executable*
(`kexec` or `fzf` missing).

## Firmware entries and the first boot

### Creating the entries without changing the boot order

`efibootmgr -c` creates an entry **and puts it first** in `BootOrder`. Save the order first, create the two entries,
then put the old order back with ZFSBootMenu appended at the end, so that nothing changes until you test it:

```
root # export ORDER="$(efibootmgr | sed -n 's/^BootOrder: //p')"
root # echo "export ORDER=\"${ORDER}\"" >> /root/zbm-migration.env
root # efibootmgr -c -d "${ESP_DISK}" -p "${ESP_PARTNUM}" -L "ZFSBootMenu (Backup)" -l '\EFI\ZBM\VMLINUZ-BACKUP.EFI'
root # efibootmgr -c -d "${ESP_DISK}" -p "${ESP_PARTNUM}" -L "ZFSBootMenu" -l '\EFI\ZBM\VMLINUZ.EFI'
root # efibootmgr
```

Read the numbers of the two new entries from the output (`Boot0004* ZFSBootMenu`, for example), and set them:

```
root # export ZBM_ENTRY=0004 ZBM_BACKUP_ENTRY=0003
root # echo "export ZBM_ENTRY=${ZBM_ENTRY} ZBM_BACKUP_ENTRY=${ZBM_BACKUP_ENTRY}" >> /root/zbm-migration.env
root # efibootmgr -o "${ORDER},${ZBM_ENTRY},${ZBM_BACKUP_ENTRY}"
```

Some firmware drops entries that are missing from `BootOrder`; appending them keeps them listed. The ESP is FAT, so
the case of the path does not matter.

**Check:**

```
root # efibootmgr -v
```

`BootOrder` starts with the same entries as in `/mnt/usb/efibootmgr-before-zfsbootmenu.txt`, and the two ZFSBootMenu
entries point to `\EFI\ZBM\VMLINUZ.EFI` and `\EFI\ZBM\VMLINUZ-BACKUP.EFI` on the ESP's partition.

### The test boot

`BootNext` makes the firmware boot an entry **once**; the boot after that uses `BootOrder` again, so a failed test
falls back to the old loader by itself after a power cycle:

```
root # efibootmgr -n "${ZBM_ENTRY}"
root # reboot
```

What to expect:

1. ZFSBootMenu starts and asks for the LUKS passphrase (`Unlock LUKS volume ...` in the legacy runtime).
2. It imports the pool and shows `${POOL}/ROOT/${BE}` with a countdown. Press **Enter**, or wait.
3. The screen goes blank briefly while `kexec` starts Gentoo's kernel. Gentoo's initramfs unlocks the container with
   the key file **without asking**, and the system starts as usual.

If a step fails, see [Troubleshooting](#troubleshooting). To get back to the old loader, power the machine off and on,
or choose the old entry in the firmware's boot menu.

**Check** after logging in as root:

```
root # cat /proc/cmdline
root # findmnt /
root # zpool status "${POOL}"
root # dmsetup ls --target crypt
root # efibootmgr | grep -E '^Boot(Current|Order)'
```

- `/proc/cmdline` contains `root=ZFS=${POOL}/ROOT/${BE}` (or your root prefix), `spl.spl_hostid=` (or
  `spl_hostid=00000000` when the host ID is zero) and the parameters of `org.zfsbootmenu:commandline`, among them
  `rd.luks.uuid=` (crypttab way) or `luks.uuid=` (kernel-parameter way).
- `/` is `${POOL}/ROOT/${BE}`, and the pool is `ONLINE`.
- The container is open under its usual name.
- `BootCurrent` is `${ZBM_ENTRY}`, and `BootOrder` is unchanged.

Boot the backup entry once in the same way (`efibootmgr -n "${ZBM_BACKUP_ENTRY}"`).

### Making ZFSBootMenu the default

After a few good test boots, put the ZFSBootMenu entries first. The old loader stays in the order as the third
choice:

```
root # efibootmgr -o "${ZBM_ENTRY},${ZBM_BACKUP_ENTRY},${ORDER}"
```

Keep the old loader and its files until you have updated the kernel and ZFS at least once through ZFSBootMenu.
Afterwards you can remove its entry (`efibootmgr -b <number> -B`) and its files from the ESP; the old `/boot`
partition of case B can then be reused.

**Check:** `efibootmgr` shows `BootOrder` starting with `${ZBM_ENTRY}`, and a normal reboot (without `BootNext`) shows
ZFSBootMenu.

### Optional: the removable-media fallback path

Some firmware ignores boot entries and starts `\EFI\BOOT\BOOTX64.EFI` instead. Copying the image there helps on such
machines, but that path often holds **another loader's copy** already (GRUB, systemd-boot and shim install one).
Overwriting it removes that fallback. Look first:

```
root # ls -l /boot/efi/EFI/BOOT/ 2>/dev/null
```

Only if the directory is empty or missing, or you know the file there is not needed:

```
root # install -D -m 0644 /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI
```

`generate-zbm` does not update this copy; repeat the command after rebuilding the image.

### Secure Boot

This article does not cover Secure Boot. The image built here is **not signed**, and a firmware with Secure Boot
enabled refuses to start it. Check the state:

```
root # od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c
```

The last number is `1` when Secure Boot is enabled and `0` when it is disabled (`bootctl status` shows the same, where
it is installed). If it is enabled, either disable it in the firmware setup, or sign the image: `contrib/zbm-sign.pl`,
a `generate-zbm` post-hook, signs with `sbctl` or `sbsigntools` (`docs/general/building.rst`, *Secure Boot signing*).
It is a Perl script: it needs Perl with the `YAML::PP` module (the Gentoo package name was not checked for this
article; `emerge --search YAML-PP` finds it) and a `SecureBoot` section in `config.yaml`.
Changing Secure Boot changes TPM PCR 7, which matters if anything on the machine (another operating system, or a TPM2
token you enroll later) is bound to it.

## Keeping it working

### After ZFS and kernel updates

The ZFSBootMenu image is built from the host's kernel, ZFS modules and tools, so it has the same ZFS version as the
system when it is built. Rebuild it:

- **after every ZFS update** (`sys-fs/zfs`, and `sys-fs/zfs-kmod` with ZFS 2.3 or older): required before
  `zpool upgrade` or before enabling new pool features, because an older image may refuse to import the pool;
- after `sys-fs/cryptsetup` updates and after changes to `/etc/zfsbootmenu/`;
- after kernel updates if you want the newer kernel's drivers in the image. The old image keeps working with a new
  installed kernel: it boots whatever kernel is in `/boot` of the boot environment.

The installed system's initramfs is rebuilt by Portage itself: a new distribution kernel runs `installkernel`, and
with USE `dist-kernel`, the ZFS module package (`sys-fs/zfs` from 2.4 on, `sys-fs/zfs-kmod` before) is rebuilt for it
and reinstalls the initramfs again. The plugin of
[Keeping the initramfs readable by root only](#keeping-the-initramfs-readable-by-root-only) runs each time.

To rebuild the ZFSBootMenu image automatically, a Portage phase hook can run `generate-zbm` after the packages above
are merged:

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

If `/etc/portage/bashrc` already defines `post_pkg_postinst`, add the `case` block to it instead of defining it twice.

Be aware of what this does:

- With ZFS 2.3 or older, a ZFS update merges `sys-fs/zfs-kmod` and `sys-fs/zfs` in one run, so the hook builds twice.
  Each build moves the previous image to `vmlinuz-backup.EFI`, so after such an update **both** images are new, and
  the backup no longer is the image that last booted. From ZFS 2.4 on, there is one package and one build. Keep a
  [known-good image](#keeping-a-known-good-image) either way.
- The hook can run while the newest kernel in `/boot` has no ZFS module yet, for example when `sys-fs/cryptsetup` is
  merged after a new kernel but before ZFS is rebuilt for it. `generate-zbm` then fails on that kernel and the hook
  only warns. Run `generate-zbm` again after `emerge @module-rebuild` (or after the ZFS package is rebuilt for the new
  kernel), or pin the image to the running kernel with `Version: "%{current}"` in the `Kernel` section of
  [config.yaml](#configyaml).
- With kernels built by hand, the ZFS modules are rebuilt only when you run `emerge @module-rebuild` after building
  the kernel; the hook runs then.
- The hook has not been tested by this repository. Read the output of `emerge` and, before the next reboot, look at
  `ls -l /boot/efi/EFI/zbm/`.

Without the hook, run `generate-zbm` by hand after these updates.

**Check** after the next update:

```
root # ls -l --time-style=long-iso /boot/efi/EFI/zbm/ /boot/initramfs*
root # find /boot -maxdepth 1 -name 'initramfs*' -type f -perm /077
root # zfs version
```

The images are newer than the update, `find` prints nothing, and `zfs version` shows the same version for the tools
and the kernel module after a reboot.

### Keeping a known-good image

A ZFSBootMenu image holds no secrets, so copies of it are harmless. After a boot through a new image works, keep a
copy outside `EFI.ImageDir`, where `generate-zbm` never touches it, with an entry of its own:

```
root # install -D -m 0644 /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/zbm-good/vmlinuz.EFI
root # efibootmgr -c -d "${ESP_DISK}" -p "${ESP_PARTNUM}" -L "ZFSBootMenu (known good)" \
  -l '\EFI\ZBM-GOOD\VMLINUZ.EFI'
root # efibootmgr -o "${ZBM_ENTRY},${ZBM_BACKUP_ENTRY},<known-good number>,${ORDER}"
```

Update the copy (the `install` command alone) whenever a new image has booted well. An old image can import the pool
only as long as the pool has no features newer than its ZFS; do not run `zpool upgrade` while you rely on an old copy.

### Changing the passphrase

```
root # cryptsetup luksChangeKey "${LUKS_PART}"
```

Run it for every container, with the same new passphrase. The key file is not affected, nothing needs to be rebuilt,
and ZFSBootMenu asks for the new passphrase at the next boot. Make a new header backup afterwards. Header backups
made before the change still hold the old passphrase's slot and open the container with the old passphrase; destroy
them if the old passphrase may be known to someone. Only `cryptsetup reencrypt` changes the volume key itself.

### Replacing the key file

Create a new key file, add it, put it into every initramfs, boot with it, and only then remove the old one. The order
matters: removing the old slot first would leave an initramfs whose key no longer opens the container.

1. Create the new key file and add it, authorized with the old key file instead of the passphrase:

   ```
   root # install -m 0400 /dev/null "${KEYFILE}.new"
   root # dd if=/dev/urandom of="${KEYFILE}.new" bs=64 count=1
   root # cryptsetup luksAddKey --pbkdf pbkdf2 --pbkdf-force-iterations 1000 --key-file "${KEYFILE}" \
     "${LUKS_PART}" "${KEYFILE}.new"
   root # cryptsetup open --test-passphrase --key-file "${KEYFILE}.new" "${LUKS_PART}"
   ```

2. Swap the files and rebuild every initramfs:

   ```
   root # mv "${KEYFILE}" "${KEYFILE}.old"
   root # mv "${KEYFILE}.new" "${KEYFILE}"
   root # for p in $(portageq match / sys-kernel/gentoo-kernel-bin); do emerge --config "=${p}"; done
   ```

   For kernels built by hand, run `make -C /usr/src/linux install` for each instead.

3. Check that the initramfs holds the new key: both lines print the same checksum.

   ```
   root # lsinitrd -f "${KEYFILE#/}" "/boot/initramfs-$(uname -r).img" | sha256sum
   root # sha256sum < "${KEYFILE}"
   ```

4. Boot once through ZFSBootMenu; Gentoo must start without asking.
5. Only then remove the old slot and the old key file, and the `.old` initramfs files that may still hold it:

   ```
   root # cryptsetup luksRemoveKey "${LUKS_PART}" "${KEYFILE}.old"
   root # shred -u "${KEYFILE}.old"
   root # rm -f /boot/initramfs-*.img.old
   ```

Run the `luksAddKey`, test and `luksRemoveKey` commands for every container. Make a new header backup afterwards.

Copies of the old key file remain in snapshots, backups and `zfs send` streams. They no longer open the container
from its own header, but a **header backup made while the old slot existed** still accepts the old key file, and every
slot wraps the same volume key. Destroy those header backups (the first backup, made before
[Creating the key file](#creating-the-key-file), never held a key-file slot). Only `cryptsetup reencrypt` changes the
volume key and makes every old header backup useless.

### Updating ZFSBootMenu

```
root # cp -a /etc/zfsbootmenu "/root/zfsbootmenu-etc.$(date +%Y%m%d)"
root # git -C /usr/src/zbm pull
root # make -C /usr/src/zbm
root # make -C /usr/src/zbm install
root # cp /root/zfsbootmenu-config.yaml /etc/zfsbootmenu/config.yaml
root # diff -r "/root/zfsbootmenu-etc.$(date +%Y%m%d)" /etc/zfsbootmenu
root # generate-zbm
```

`diff` shows what `make install` changed besides `config.yaml`; carry over your own changes. The branch is a
development branch and may be rewritten; if `git pull` refuses, use `git -C /usr/src/zbm fetch` followed by
`git -C /usr/src/zbm reset --hard origin/claude/zbm-refactor`, which discards local changes in the clone.

The next boot uses the new `vmlinuz.EFI`. If it fails, boot the previous image through the backup entry:
`efibootmgr -n "${ZBM_BACKUP_ENTRY}"`, or choose it in the firmware's boot menu.

## Optional next steps

These replace typing the passphrase in ZFSBootMenu. The second unlock in the installed system's initramfs stays the
key file, so the installed system's initramfs never needs TPM, FIDO2 or smart card support. Keep the passphrase in
its key slot as the fallback for every method. `docs/general/luks.rst` (*Hardware keys and encrypted key files*) and
`docs/luks/administration.rst` describe each one. Rebuild the image (`generate-zbm`) after installing a method's tools
on this machine: they are added to the image only when they are installed here. Tokens themselves are read from the
LUKS2 header at boot, so enrolling one needs no rebuild.

| Method | Gentoo packages (check the names with `emerge --search`) | Section |
|---|---|---|
| TPM2 token | `app-crypt/tpm2-tools`, `dev-libs/openssl`, `sys-apps/systemd[cryptsetup,tpm]` | *TPM2* |
| FIDO2 | `dev-libs/libfido2`, `sys-apps/systemd[cryptsetup,fido2]` | *FIDO2 security keys (YubiKey, Nitrokey, ...)* |
| clevis | `app-crypt/clevis`, in the GURU repository | administration.rst, *Binding with clevis instead* |
| GnuPG card | `app-crypt/gnupg`, `sys-apps/pcsc-lite` | *GnuPG and OpenPGP smart cards (YubiKey, Nitrokey, ...)* |
| YubiKey challenge-response | `sys-auth/ykpers` (to program the key) | *YubiKey challenge-response* |
| key file on a USB stick | none | administration.rst, *Using a key file on a USB stick* |

Sections are in `docs/general/luks.rst` unless the table names `docs/luks/administration.rst`. `systemd-cryptenroll`
enrolls TPM2 and FIDO2 tokens; it comes with the USE flags shown (PKCS#11 tokens need `pkcs11`). `sys-apps/pcsc-lite`
is needed only for card readers that GnuPG's built-in driver does not handle, together with `zfsbootmenu_luks_pcscd=yes`
in `/etc/zfsbootmenu/dracut.conf.d/`. `dev-libs/openssl` is normally installed already; images of the legacy runtime
need it for ZFSBootMenu's own TPM2 code.

Notes:

- **`systemd-cryptenroll` on OpenRC.** It comes with `sys-apps/systemd[cryptsetup]`; `sys-apps/systemd-utils`, which
  OpenRC systems have, has no `cryptsetup` USE flag and does not install it. Enroll TPM2 and FIDO2 tokens from a live
  system that has it, or use clevis, a GnuPG card or challenge-response. Check with `command -v systemd-cryptenroll`.
- **GnuPG key files and challenge-response** are set in the ZFSBootMenu crypttab, `/etc/zfsbootmenu/crypttab`, which
  `zbm.luks=` overrides. To use them, remove `zbm.luks=` from `Kernel.CommandLine` and list every container in that
  crypttab, named `luks-<UUID>`. A GnuPG-encrypted key file can instead be named with `zbm.luks_key=` and added with
  `install_items` in `/etc/zfsbootmenu/dracut.conf.d/` (`docs/general/luks.rst`, *Unlocking without a crypttab*).
  Never do either with the plain key file of this article.
- **TPM2 and PCRs.** Bind to PCR 7 (the Secure Boot state), which survives ZFSBootMenu, kernel and initramfs updates:
  `systemd-cryptenroll --tpm2-device=auto --tpm2-pcrs=7 "${LUKS_PART}"`. PCRs 4, 8, 9 and 11 change with every image or
  kernel update. With the Rust runtime, PCR 9 can be bound together with PCR 7 using the value
  `/usr/libexec/zfsbootmenu/zbm measure unseal` prints; re-enroll after every ZFSBootMenu update (luks.rst, *Measured
  boot*). A TPM2 token **without a PIN** unlocks the pool for anyone who can switch the computer on, and ZFSBootMenu
  offers a recovery shell: use a PIN, or Secure Boot with PCR 7.
- **systemd 262 and later** harden a TPM2 PIN (`--tpm2-with-pin=yes`) with Argon2id, sized on the enrolling machine.
  ZFSBootMenu derives it at every boot on one core, and refuses a token above its bounds. Fix the costs with
  `--tpm2-argon2id-memory=1G --tpm2-argon2id-iterations=3`, or use `--tpm2-with-pin=direct` (PBKDF2, as systemd 255 to
  261) when the legacy runtime must open the token. The Rust runtime opens Argon2id tokens itself.
- **USB key stick.** Add `zbm.luks_key=/zroot.key:LABEL=ZROOTKEY` to `Kernel.CommandLine`, and the USB storage drivers
  to the image: `echo 'add_drivers+=" usb_storage uas "' > /etc/zfsbootmenu/dracut.conf.d/usb.conf`. Keep the stick
  apart from the computer.
- **TRIM.** To pass TRIM through without crypttab options, store the flag in the LUKS2 header of the open container:
  `cryptsetup refresh --allow-discards --persistent "${LUKS_NAME}"` (add `--perf-no_read_workqueue
  --perf-no_write_workqueue` on NVMe if wanted). ZFSBootMenu and the initramfs apply stored flags at every
  activation; `cryptsetup luksDump "${LUKS_PART}" | grep Flags` shows them. TRIM reveals which blocks are unused, not
  their content. Enable `autotrim=on` on the pool or run `zpool trim` periodically.
- **ZFS native encryption** on top of LUKS is described in `docs/general/native-encryption.rst`
  (`org.zfsbootmenu:keysource`); this article does not cover it.

## Troubleshooting

Open a recovery shell in ZFSBootMenu with **Escape** while it waits for a pool, or with **Ctrl+R** (or **Alt+R**) in
the boot environment list. The commands below marked `zbm #` run there; the shell variables of this article do not
exist there, so type your pool name and UUID.

For more messages, add `loglevel=7` to the image's command line from the installed system:

```
root # zbm-kcl -r loglevel -a loglevel=7 /boot/efi/EFI/zbm/vmlinuz.EFI
```

It stays until the next `generate-zbm`, or until `zbm-kcl -r loglevel -a loglevel=0` on the same file. Read the log in
the recovery shell with `zlogtail`. (The editor of the classic menu, Alt+E, changes the boot environment's command
line, not ZFSBootMenu's own.)

### ZFSBootMenu does not ask for the passphrase, or shows no pool

```
zbm # cat /proc/cmdline
zbm # zluks list
zbm # zluks
```

- `zbm.luks=` names the wrong UUID, often the pool's GUID or a file system UUID instead of `cryptsetup luksUUID`. Fix
  `Kernel.CommandLine` and run `generate-zbm`, or for one image `zbm-kcl -r zbm.luks -a zbm.luks=UUID=<uuid>
  /boot/efi/EFI/zbm/vmlinuz.EFI` on the installed system (or from the rescue medium).
- `zluks list` does not show the container: the disk's driver is missing from the image. For NVMe, add
  `add_drivers+=" nvme "` to a file in `/etc/zfsbootmenu/dracut.conf.d/` and rebuild. If `/etc/dracut.conf` sets
  `hostonly="yes"`, the image is built host-only as well.
- `zluks` unlocks by hand; `exit` returns to the import.

### The image is missing cryptsetup

`lsinitrd` of the extracted image (see [Building the image](#building-the-image)) shows no `cryptsetup`, and
ZFSBootMenu never offers to unlock anything. dracut leaves LUKS support out silently when `cryptsetup` is missing on
the build host, or when `zfsbootmenu_luks=no` is set in `/etc/zfsbootmenu/dracut.conf.d/`. Install
`sys-fs/cryptsetup`, remove the option, and run `generate-zbm`.

### Gentoo asks for the passphrase a second time

The installed system's initramfs could not use the key file. Boot, then:

```
root # cat /proc/cmdline
root # lsinitrd "/boot/initramfs-$(uname -r).img" | grep -E 'cryptsetup-keys\.d|systemd-cryptsetup$|bin/cryptsetup$'
root # lsinitrd -f etc/crypttab "/boot/initramfs-$(uname -r).img"
```

- The key file is missing: the `install_items` path in `/etc/dracut.conf.d/10-zfs-luks.conf` is wrong, or the
  initramfs was not rebuilt (see [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs)).
- The crypttab is missing, names another UUID or key path, or writes the device in another form than
  `UUID=<LUKS UUID>` (crypttab way).
- `luks.uuid=` or `luks.key=` is missing from `/proc/cmdline` or has the wrong UUID (kernel-parameter way); look at
  `zfs get org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"`. On OpenRC, `luks.*` does nothing; use the crypttab.
- On the systemd profile, `systemd-cryptsetup` is missing from the initramfs: `sys-apps/systemd` lacks USE
  `cryptsetup`.
- A boot through the old loader asks for the passphrase as before; that is expected.

### No kernels are listed

ZFSBootMenu shows the boot environment but no kernel, or refuses to boot it:

- `/boot` is still a separate partition or dataset, so the root dataset's `/boot` is empty. See
  [Moving /boot into the root dataset](#moving-boot-into-the-root-dataset).
- `/boot` holds a kernel without a matching initramfs (`initramfs-<version>.img`), for example after a failed dracut
  run. ZFSBootMenu leaves such kernels out. Rebuild the initramfs.
- Kernels went to the ESP: `installkernel` still has `systemd-boot`, `uki` or `efistub` set, or
  `/etc/kernel/install.conf` names another layout than `compat`. `tail /var/log/installkernel.log` shows where the
  last kernels went.

### The pool is not imported: wrong host ID, or in use elsewhere

ZFSBootMenu by default adopts the host ID of a pool it cannot import otherwise (`zbm.import_policy=hostid`), and then
passes it to the booted kernel. If the installed system's initramfs still refuses the pool (*pool was previously in
use from another system*), it drops to dracut's emergency shell. Import the pool there by its name (here `zroot`) and
continue the boot:

```
initramfs # zpool import -f -N zroot
initramfs # exit
```

Then, on the running system, compare `hostid` with the pool label (see [The host ID](#the-host-id)), fix
`/etc/hostid`, rebuild the initramfs and run `generate-zbm`. A pool imported on the rescue medium and not exported
causes the same message once.

### Dropping into dracut's emergency shell

The initramfs could not unlock the container or mount the root. In the emergency shell, with your container's UUID
and your pool's name:

```
initramfs # cryptsetup open /dev/disk/by-uuid/YOUR-LUKS-UUID luks-YOUR-LUKS-UUID
initramfs # zpool import -N zroot
initramfs # exit
```

Common causes:

- On **OpenRC**, `rd.luks.uuid=` is missing from the command line: without it dracut's script never starts for the
  container, asks nothing and waits for the root until it gives up.
- The `zfs` or `crypt` dracut module is missing (`lsinitrd | grep -E 'zfs|crypt'`), or `zfs.ko` is (see the USE
  flag `initramfs` in [Reinstalling the kernels](#reinstalling-the-kernels)).
- The root prefix does not suit the initramfs (`org.zfsbootmenu:rootprefix`).

`journalctl -b` (systemd profile) or the report that dracut's `rdsosreport` command writes (its path is printed in
the emergency shell) shows what failed.

### Getting back to the old loader

- Power off and on: after a `BootNext` test boot, the firmware uses `BootOrder` again.
- Choose the old entry in the firmware's boot menu (the key differs by vendor: often F8, F11, F12 or Esc).
- `efibootmgr -o <old order>` from the running system; the old order is in
  `/mnt/usb/efibootmgr-before-zfsbootmenu.txt`.

The old loader boots the old kernel and initramfs, which ask for the passphrase as before, as long as that kernel's
modules are still installed.

## Removing ZFSBootMenu

1. Put the old order back and delete the entries:

   ```
   root # efibootmgr -o "${ORDER}"
   root # efibootmgr -b "${ZBM_ENTRY}" -B
   root # efibootmgr -b "${ZBM_BACKUP_ENTRY}" -B
   ```

2. Delete the images (and a known-good or `\EFI\BOOT` copy you made) and the programs:

   ```
   root # rm -r /boot/efi/EFI/zbm /boot/efi/EFI/zbm-good
   root # rm /usr/bin/generate-zbm /usr/bin/zbm-kcl
   root # rm -r /usr/libexec/zfsbootmenu /usr/share/zfsbootmenu /usr/share/examples/zfsbootmenu
   root # rm -r /usr/lib/dracut/modules.d/90zfsbootmenu /usr/lib/dracut/modules.d/90zfsbootmenu-measured
   root # rm /usr/lib/initcpio/hooks/zfsbootmenu* /usr/lib/initcpio/install/zfsbootmenu* \
     /usr/lib/initcpio/zfsbootmenu-measured.service
   root # rm /usr/share/man/man5/generate-zbm.5 /usr/share/man/man5/zfsbootmenu-crypttab.5 \
     /usr/share/man/man7/zfsbootmenu.7 /usr/share/man/man7/zfsbootmenu-luks.7 \
     /usr/share/man/man8/generate-zbm.8 /usr/share/man/man8/zbm-debug.8 /usr/share/man/man8/zbm-kcl.8 \
     /usr/share/man/man8/zluks.8
   root # rm -r /etc/zfsbootmenu
   ```

   Remove the `post_pkg_postinst` block from `/etc/portage/bashrc` and the ZFSBootMenu properties
   (`zfs inherit org.zfsbootmenu:commandline "${POOL}/ROOT"`). Rebuild the installed system's initramfs, so that the
   `zfsbootmenu-measured` module leaves it.

3. **Before** the old loader takes over permanently, decide where kernels go. If they go back to an **unencrypted**
   `/boot` (the ESP or a separate partition), the key file must leave the initramfs first, and its key slot must go:

   ```
   root # nano -w /etc/dracut.conf.d/10-zfs-luks.conf     # remove the key file from install_items
   root # cryptsetup luksRemoveKey "${LUKS_PART}" "${KEYFILE}"
   root # shred -u "${KEYFILE}"
   ```

   Keep the `zfs` and `crypt` modules in that file. Remove the key file from `/etc/crypttab` (or the line, if you
   added it), restore `/etc/fstab.before-zfsbootmenu` and the old `/etc/kernel/install.conf` (or delete the latter),
   re-enable the old loader's USE flags on `sys-kernel/installkernel`, mount the old `/boot`, and reinstall the
   kernels. Delete `/boot/initramfs*` files left in the root dataset, `.old` ones included, then check the old `/boot`
   as in [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs): no file may hold the key.
   Make a new header backup, and destroy the header backups made while the key file's slot existed: they still accept
   the key file, whose copies remain in snapshots and backups. Only `cryptsetup reencrypt` changes the volume key.

## Security notes

- **The key file in the initramfs.** Anyone who can read the installed system's initramfs, or `/etc/cryptsetup-keys.d`,
  can unlock the container without the passphrase. Both are inside the encrypted pool and readable by root only. But
  every copy of the root dataset carries them: `zfs send` streams, backups of `/` and `/boot`, and snapshots. Treat
  such backups like the passphrase, or exclude `/etc/cryptsetup-keys.d` and `/boot/initramfs*` from file backups.
  The traditional installkernel (OpenRC) also keeps the previous build of a version as `initramfs-<version>.img.old`,
  which holds the key file that was current then.
- **Old key files and header backups.** [Replacing the key file](#replacing-the-key-file) makes old copies useless
  against the container's own header, but not against a header backup made while their slot existed: every slot
  wraps the same volume key. Destroy such header backups when you retire a key file, or run `cryptsetup reencrypt`,
  which changes the volume key.
- **The ESP** is unencrypted and writable by anyone with physical access or root. It holds ZFSBootMenu's images, which
  contain no key, and the old loader's files, whose initramfs predates the key file (the checks in
  [Rebuilding and checking the initramfs](#rebuilding-and-checking-the-initramfs) confirm that). Someone who can change
  the image can, however, capture the passphrase at the next boot;
  that is the reason for Secure Boot with signed images. Without Secure Boot, this setup protects data at rest
  (a stolen or powered-off machine), not a machine that someone else had access to and you boot again afterwards.
- **Hooks.** Executables in `Global.PreHooksDir`, `Global.PostHooksDir` and the image's hook directories run as root
  during the build or before the menu; only root should be able to write them.
- **The recovery shell** of ZFSBootMenu gives root access to the unlocked pool to whoever types the passphrase. With
  TPM2 unlocking without a PIN, that is anyone who can switch the computer on.
- The threat model of ZFSBootMenu on this branch, with its assumptions and the risks that remain, is in
  `docs/dev/threat-model.md`.
- `claude/zbm-refactor` is a **development branch**. Read its changes before updating, and keep the passphrase, the
  rescue medium and the header backups.

## See also

- This repository: `docs/general/luks.rst` (how unlocking works), `docs/general/building.rst` (building on the host),
  `docs/luks/administration.rst` (key management, recovery), `docs/general/uefi-booting.rst`,
  `docs/man/zfsbootmenu.7.rst`, `docs/man/generate-zbm.5.rst`, `docs/man/zbm-kcl.8.rst` and `docs/man/zluks.8.rst`
- `docs/guides/gentoo/zfs-luks-v4.md` — a new Gentoo installation with the same layout
- [ZFS](https://wiki.gentoo.org/wiki/ZFS) and [ZFS/rootfs](https://wiki.gentoo.org/wiki/ZFS/rootfs) — ZFS on Gentoo
- [Dracut](https://wiki.gentoo.org/wiki/Dracut) and [Installkernel](https://wiki.gentoo.org/wiki/Installkernel)
- [Dm-crypt](https://wiki.gentoo.org/wiki/Dm-crypt) and
  [Full Disk Encryption From Scratch](https://wiki.gentoo.org/wiki/Full_Disk_Encryption_From_Scratch)
- [systemd-cryptsetup-generator(8)][cryptsetup-generator] — the `luks.*` and `rd.luks.*` parameters
- [dracut.cmdline(7)](https://man7.org/linux/man-pages/man7/dracut.cmdline.7.html) — dracut's `rd.luks.*` parameters
- [efibootmgr(8)](https://github.com/rhboot/efibootmgr) — boot entries, `BootOrder` and `BootNext`

[cryptsetup-generator]: https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html
