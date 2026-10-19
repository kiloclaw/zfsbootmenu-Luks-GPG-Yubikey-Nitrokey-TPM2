# Gentoo Linux on a LUKS-encrypted ZFS root with a GnuPG key file (systemd)

This article describes how to install Gentoo Linux with the **systemd** profile on a **single disk** whose ZFS root
pool lives inside a **LUKS2** container. The container is unlocked at boot by a **GnuPG-encrypted key file**: a random
key, encrypted either to an OpenPGP key on a smart card (YubiKey, Nitrokey, ...) or with a GnuPG passphrase.

The system boots with the ZFSBootMenu build from this repository, which can unlock LUKS volumes and decrypt GnuPG key
files. **No crypttab is used anywhere**: ZFSBootMenu learns what to unlock from its own kernel command line, and
Gentoo's initramfs from systemd's `luks.*` kernel parameters.

> **Important**
> This guide erases the target disk completely. Back up all data on it before starting.

## Contents

- [Overview](#overview)
  - [Boot process](#boot-process)
  - [Unlocking without crypttab](#unlocking-without-crypttab)
  - [Key slots](#key-slots)
  - [Choosing how the key file is protected](#choosing-how-the-key-file-is-protected)
- [Requirements](#requirements)
  - [Hardware](#hardware)
  - [Installation media](#installation-media)
  - [Conventions](#conventions)
- [Preparing the disk](#preparing-the-disk)
  - [Setting variables](#setting-variables)
  - [Partitioning](#partitioning)
  - [Creating the LUKS container](#creating-the-luks-container)
  - [Creating the ZFS pool](#creating-the-zfs-pool)
- [Installing the stage file](#installing-the-stage-file)
- [Installing the base system](#installing-the-base-system)
  - [Chrooting](#chrooting)
  - [Configuring Portage](#configuring-portage)
  - [Localization and host name](#localization-and-host-name)
- [Kernel, ZFS and GnuPG](#kernel-zfs-and-gnupg)
- [Creating the GnuPG key file](#creating-the-gnupg-key-file)
  - [Option A: OpenPGP smart card](#option-a-openpgp-smart-card)
  - [Option B: GnuPG passphrase](#option-b-gnupg-passphrase)
  - [Adding the key file to LUKS](#adding-the-key-file-to-luks)
- [Configuring the initramfs](#configuring-the-initramfs)
- [Configuring the system](#configuring-the-system)
- [Installing ZFSBootMenu](#installing-zfsbootmenu)
- [Finalizing the installation](#finalizing-the-installation)
- [Usage](#usage)
- [Troubleshooting](#troubleshooting)
- [See also](#see-also)
- [External resources](#external-resources)

## Overview

### Boot process

1. The UEFI firmware starts **ZFSBootMenu** from the EFI system partition (ESP).
2. ZFSBootMenu reads `zbm.luks=` and `zbm.luks_key=` from its kernel command line, decrypts the GnuPG key file
   (asking for the smart card and its PIN, or for the GnuPG passphrase), unlocks the LUKS container and imports the
   pool.
3. The user selects a boot environment, or the countdown expires. ZFSBootMenu closes the LUKS container, which wipes
   the volume key from memory, and starts Gentoo's kernel with `kexec`.
4. Gentoo's initramfs reads `luks.uuid=` and `luks.key=` from the kernel command line and reopens the container with a
   second key file that is stored **inside** the encrypted pool. No second prompt appears.
5. The initramfs imports the pool, mounts the root file system and starts systemd.

### Unlocking without crypttab

| Stage | What to unlock | Which key | Configured in |
|---|---|---|---|
| ZFSBootMenu | `zbm.luks=UUID=<LUKS UUID>` | `zbm.luks_key=/etc/zfsbootmenu/keys/zroot.key.gpg` | `Kernel.CommandLine` in `/etc/zfsbootmenu/config.yaml` |
| Gentoo initramfs | `luks.uuid=<LUKS UUID>` | `luks.key=<LUKS UUID>=/etc/cryptsetup-keys.d/zroot-initrd.key` | ZFS property `org.zfsbootmenu:commandline` on `zroot/ROOT` |

`luks.crypttab=no` tells systemd to ignore `/etc/crypttab` completely, and `/etc/zfsbootmenu/crypttab` is never
created. The kernel parameters are documented in
[systemd-cryptsetup-generator(8)](https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html)
and `man 7 zfsbootmenu-luks`.

### Key slots

The LUKS container ends up with three key slots:

| Key | Stored where | Used by | Purpose |
|---|---|---|---|
| Passphrase | your memory | anyone, by hand | fallback when the card or GnuPG key is lost |
| GnuPG key file | `/etc/zfsbootmenu/keys/zroot.key.gpg`, embedded **encrypted** in the ZFSBootMenu image | ZFSBootMenu | normal unlocking at power-on |
| Initramfs key file | `/etc/cryptsetup-keys.d/zroot-initrd.key`, inside the encrypted pool and in Gentoo's initramfs | Gentoo's initramfs | second unlock after `kexec`, without a prompt |

The initramfs key file is plain, but it is only readable after the pool has been unlocked: both the file and the
initramfs live in the encrypted pool, under `/etc` and `/boot`. The only unencrypted data on the disk is the
ZFSBootMenu image on the ESP, which contains the key file in **encrypted** form only.

> **Note**
> The initramfs key cannot be GnuPG-encrypted as well. After `kexec`, Gentoo's kernel starts without any secrets, so
> a GnuPG-protected key at that stage would mean entering the PIN or passphrase a second time.

### Choosing how the key file is protected

| | Option A: OpenPGP smart card | Option B: GnuPG passphrase |
|---|---|---|
| Hardware | YubiKey 5, Nitrokey 3/Pro/Start, or any OpenPGP card | none |
| At boot | insert card, enter card PIN, touch (if configured) | enter GnuPG passphrase |
| Offline attack on a stolen ESP | not possible: the private key never leaves the card | possible: choose a strong passphrase |
| Wrong guesses | card locks after 3 wrong PINs | unlimited |

Option A is recommended. It requires an OpenPGP key with an **encryption subkey already on the card**. Creating one is
outside the scope of this article; see [GnuPG](https://wiki.gentoo.org/wiki/GnuPG) and
[YubiKey](https://wiki.gentoo.org/wiki/YubiKey).

## Requirements

### Hardware

- An amd64 computer that boots in **UEFI** mode, with **Secure Boot disabled** (the ZFSBootMenu image built here is
  unsigned).
- One disk for the installation. All data on it will be lost.
- For option A: the OpenPGP smart card, and its **public key** as a file (`pubkey.asc`) on a USB stick, or published
  on a key server or at the URL stored on the card.
- An internet connection.

### Installation media

The live system must provide **ZFS** (2.2 or later) and **cryptsetup**. The Gentoo minimal installation CD does not
include ZFS. Suitable choices:

- the Ubuntu Desktop live ISO, 24.04 or later (*Try Ubuntu*, then open a terminal and run `sudo -i`);
- a Gentoo LiveGUI image, if `zfs version` works on it;
- any other UEFI live system with ZFS and cryptsetup.

GnuPG is **not** needed on the live system. All GnuPG work is done inside the new Gentoo system.

### Conventions

| Notation | Meaning |
|---|---|
| `root #` | run the command as root on the **live system**; do not type the prompt |
| `(chroot) root #` | run the command as root **inside the new Gentoo system** |
| **FILE** `/path` | create or edit this file with the content shown, for example with `nano -w /path` |
| `${VARIABLE}` | a shell variable set in [Setting variables](#setting-variables) |
| `CAPITALS` | a value you must replace |

Each section ends with a **Check**. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Preparing the disk

Boot the live system in UEFI mode, connect to the network and open a root shell.

**Check:**

```
root # ls /sys/firmware/efi
root # modprobe zfs && zfs version
root # cryptsetup --version
root # date
```

The first command must list files (otherwise the system booted in BIOS mode), ZFS and cryptsetup must print their
versions, and the date must be correct.

### Setting variables

List the disks by their persistent names, and pick the target disk:

```
root # ls -l /dev/disk/by-id/ | grep -v -- -part
```

```
root # export DISK=/dev/disk/by-id/YOUR-DISK
root # export ESP_PART="${DISK}-part1"
root # export LUKS_PART="${DISK}-part2"
root # export POOL=zroot
root # export BE=gentoo
```

> **Tip**
> If the live system is rebooted or the terminal closed, set these variables again before continuing.

**Check:**

```
root # lsblk -o NAME,SIZE,MODEL "${DISK}"
```

### Partitioning

| Partition | Size | Type | Use |
|---|---|---|---|
| 1 | 1 GiB | EFI System | ESP, mounted at `/boot/efi`, holds ZFSBootMenu |
| 2 | remainder | Linux LUKS | LUKS2 container holding the ZFS pool |

```
root # wipefs -a "${DISK}"
root # sfdisk "${DISK}" <<'EOF'
label: gpt
size=1GiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI"
type=CA7D7CCB-63ED-4C53-861C-1742536059CC, name="zroot-luks"
EOF
root # udevadm settle
root # mkfs.vfat -F 32 -n EFI "${ESP_PART}"
```

**Check:**

```
root # lsblk -o NAME,SIZE,FSTYPE,PARTLABEL "${DISK}"
```

Two partitions are listed: `EFI` (vfat) and `zroot-luks`.

### Creating the LUKS container

Create the container. Type `YES` when asked, then choose the **fallback passphrase**:

```
root # cryptsetup luksFormat --type luks2 "${LUKS_PART}"
```

> **Important**
> Keep this passphrase. It is the only way in if the smart card is lost or the GnuPG key file is damaged. Use
> characters that are on a US keyboard layout: ZFSBootMenu prompts before any keyboard layout is loaded.

Open the container as `luks-<UUID>`. `--allow-discards --persistent` stores the TRIM setting in the LUKS header, so it
applies at every boot without a crypttab option:

```
root # export LUKS_UUID="$(cryptsetup luksUUID "${LUKS_PART}")"
root # cryptsetup open --allow-discards --persistent "${LUKS_PART}" "luks-${LUKS_UUID}"
```

**Check:**

```
root # cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|flags'
```

The output shows `type: LUKS2` and `flags: discards`.

### Creating the ZFS pool

Set a fixed host ID. The same `/etc/hostid` is copied into Gentoo and from there into both initramfs images:

```
root # zgenhostid -f 0x00bab10c
```

Create the pool on the opened container:

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
  "${POOL}" "/dev/mapper/luks-${LUKS_UUID}"
```

> **Note**
> `compatibility=openzfs-2.2-linux` keeps the pool importable by live systems with ZFS 2.2. It can be relaxed later;
> see [Usage](#relaxing-the-pool-compatibility).

Create the datasets and set the default boot environment:

```
root # zfs create -o mountpoint=none "${POOL}/ROOT"
root # zfs create -o mountpoint=/ -o canmount=noauto "${POOL}/ROOT/${BE}"
root # zfs create -o mountpoint=/home "${POOL}/home"
root # zpool set bootfs="${POOL}/ROOT/${BE}" "${POOL}"
```

Set the kernel command line that ZFSBootMenu passes to Gentoo. These are the `luks.*` parameters that replace the
crypttab in Gentoo's initramfs (see [Unlocking without crypttab](#unlocking-without-crypttab)). The key file named
here is created later, in [Configuring the initramfs](#configuring-the-initramfs):

```
root # zfs set org.zfsbootmenu:commandline="quiet luks.crypttab=no luks.uuid=${LUKS_UUID} luks.key=${LUKS_UUID}=/etc/cryptsetup-keys.d/zroot-initrd.key luks.options=${LUKS_UUID}=discard" "${POOL}/ROOT"
```

| Parameter | Effect |
|---|---|
| `luks.crypttab=no` | ignore `/etc/crypttab`, in the initramfs and on the running system |
| `luks.uuid=<UUID>` | unlock the container with this LUKS UUID, as `luks-<UUID>` |
| `luks.key=<UUID>=<path>` | use this key file for it |
| `luks.options=<UUID>=discard` | pass TRIM through |

ZFSBootMenu adds `root=zfs:zroot/ROOT/gentoo` by itself. The property is set on `zroot/ROOT`, so future boot
environments inherit it.

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
root # findmnt -R /mnt/gentoo
root # zfs get -H -o value org.zfsbootmenu:commandline "${POOL}/ROOT/${BE}"
```

The pool is `ONLINE`; `zroot/ROOT/gentoo` and `zroot/home` are mounted; the command line shows your LUKS UUID three
times.

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

Verify it, then stop the live system's GnuPG agent so it cannot interfere with GnuPG inside the chroot later:

```
root # wget -O - https://qa-reports.gentoo.org/output/service-keys.gpg | gpg --import
root # gpg --verify stage3-amd64-systemd-*.tar.xz.asc
root # gpgconf --kill all
```

The output must include `Good signature from "Gentoo Linux Release Engineering (Automated Weekly Release Key) <releng@gentoo.org>"`.

Extract it and mount the ESP:

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
(chroot) root # export GPG_TTY="$(tty)"
```

`GPG_TTY` lets GnuPG ask for PINs and passphrases in this terminal.

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

**FILE** `/etc/portage/package.use/zfs-luks-gpg`

```
# ZFS: dracut module and root-on-ZFS support
sys-fs/zfs dracut rootfs
# Do not install kernels newer than ZFS supports
sys-fs/zfs-kmod dist-kernel-cap
# Build an initramfs with dracut for every installed kernel
sys-kernel/installkernel dracut
# systemd-cryptsetup and its generator, which read the luks.* kernel parameters
# boot: the UEFI stub (linuxx64.efi.stub) that generate-zbm needs to build the ZFSBootMenu EFI image
sys-apps/systemd cryptsetup boot
# GnuPG with scdaemon and its built-in USB smart card driver
app-crypt/gnupg smartcard usb
```

| Package | USE flag | Why |
|---|---|---|
| `sys-fs/zfs` | `dracut`, `rootfs` | dracut module for a ZFS root in Gentoo's initramfs |
| `sys-fs/zfs-kmod` | `dist-kernel-cap` | keeps the kernel within the range ZFS supports |
| `sys-kernel/installkernel` | `dracut` | creates the initramfs when a kernel is installed |
| `sys-apps/systemd` | `cryptsetup`, `boot` | unlocks the container from `luks.*` parameters; `boot` installs the UEFI stub for the ZFSBootMenu image |
| `app-crypt/gnupg` | `smartcard`, `usb` | `scdaemon` with the built-in CCID driver for YubiKeys and Nitrokeys; also copied into the ZFSBootMenu image |

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

## Kernel, ZFS and GnuPG

```
(chroot) root # emerge --ask \
  sys-kernel/linux-firmware \
  sys-kernel/gentoo-kernel-bin \
  sys-fs/zfs \
  sys-fs/cryptsetup \
  sys-fs/dosfstools \
  sys-boot/efibootmgr \
  app-crypt/gnupg \
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
| `sys-kernel/gentoo-kernel-bin` | prebuilt distribution kernel (`sys-kernel/gentoo-kernel` compiles it instead) |
| `sys-fs/zfs` | ZFS tools; pulls in the `sys-fs/zfs-kmod` kernel module |
| `sys-fs/cryptsetup` | LUKS tools, for Gentoo and for the ZFSBootMenu image |
| `app-crypt/gnupg` | creates the key file; copied into the ZFSBootMenu image to decrypt it |
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
uki_generator=none
# Never install kernels onto the EFI system partition: it is not encrypted, and the initramfs holds the key file
BOOT_ROOT=/boot
```

This file replaces Gentoo's default `/usr/lib/kernel/install.conf` completely, so it must repeat
`uki_generator=none`; without it, `sys-kernel/installkernel` refuses to install kernels. `BOOT_ROOT=/boot` matters
as soon as the ESP is mounted at `/boot/efi`: systemd's `kernel-install` would otherwise detect the ESP and put kernels
and initramfs images there, unencrypted, together with the key file inside the initramfs.

**Check:**

```
(chroot) root # ls /lib/modules/
(chroot) root # find /lib/modules -name 'zfs.ko*'
(chroot) root # ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub
(chroot) root # ls /boot
(chroot) root # gpg --version | head -n 1
```

One kernel version is listed, `zfs.ko` exists for it, the UEFI stub exists, `/boot` contains `kernel-<version>` (or `vmlinuz-<version>`) and `initramfs-<version>.img`, and GnuPG prints its version. The initramfs does not unlock the disk yet; that is done in
[Configuring the initramfs](#configuring-the-initramfs).

## Creating the GnuPG key file

A random 64-byte key is created in RAM (`/run` is a tmpfs), added to the LUKS container, encrypted with GnuPG, and the
plain copy is destroyed. Follow **either** option A or option B to encrypt it, then continue with
[Adding the key file to LUKS](#adding-the-key-file-to-luks).

Create the key and the directory for the encrypted copy:

```
(chroot) root # install -m 0600 /dev/null /run/zroot.key
(chroot) root # dd if=/dev/urandom of=/run/zroot.key bs=64 count=1
(chroot) root # install -d -m 0700 /etc/zfsbootmenu/keys
```

### Option A: OpenPGP smart card

Insert the card. Import its public key using **one** of these methods:

```
(chroot) root # gpg --import /PATH/TO/pubkey.asc
```

```
(chroot) root # gpg --card-edit
gpg/card> fetch
gpg/card> quit
```

```
(chroot) root # gpg --keyserver hkps://keys.openpgp.org --recv-keys FINGERPRINT
```

Link the card to the key and note the fingerprint:

```
(chroot) root # gpg --card-status
(chroot) root # gpg --list-keys --with-subkey-fingerprints
(chroot) root # export GPG_KEY=FINGERPRINT
```

`gpg --card-status` must show the card's serial number and an *Encryption key* fingerprint. Use the fingerprint of the
**primary key** for `GPG_KEY`. GnuPG picks the encryption subkey automatically.

> **Note**
> If `gpg --card-status` reports *No such device* while the card is plugged in, the live system's `pcscd` probably
> holds the card. Stop it **on the live system** in a second terminal (on Ubuntu:
> `systemctl stop pcscd.socket pcscd.service`), then run `gpgconf --kill all` in the chroot and try again.

Encrypt the key to the card's key. To allow a backup card as well, add a second `--recipient` option:

```
(chroot) root # gpg --trust-model always --encrypt --recipient "${GPG_KEY}" \
  --output /etc/zfsbootmenu/keys/zroot.key.gpg /run/zroot.key
```

Tell ZFSBootMenu to copy the public key into its image. Without the public key, GnuPG in ZFSBootMenu cannot match the
card to the key file:

**FILE** `/etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf`

```
# GnuPG-encrypted LUKS key file, unlocked with zbm.luks_key=
install_items+=" /etc/zfsbootmenu/keys/zroot.key.gpg "

# Public key(s) the key file is encrypted to (option A only); replace FINGERPRINT
zfsbootmenu_luks_gpg_keys="FINGERPRINT"
```

### Option B: GnuPG passphrase

Encrypt the key with a passphrase. GnuPG asks for it twice. Because the encrypted file is stored on the unencrypted
ESP, **anyone who copies it can try passphrases offline**: use a long passphrase, and different from the LUKS fallback
passphrase. The options below make each guess slower:

```
(chroot) root # gpg --symmetric --cipher-algo AES256 \
  --s2k-mode 3 --s2k-digest-algo SHA512 --s2k-count 65011712 \
  --output /etc/zfsbootmenu/keys/zroot.key.gpg /run/zroot.key
```

**FILE** `/etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf`

```
# GnuPG-encrypted LUKS key file, unlocked with zbm.luks_key=
install_items+=" /etc/zfsbootmenu/keys/zroot.key.gpg "
```

Clear GnuPG's passphrase cache, so the test below really asks for the passphrase:

```
(chroot) root # gpgconf --reload gpg-agent
```

### Adding the key file to LUKS

Add the plain key to a new key slot. cryptsetup asks for the fallback passphrase:

```
(chroot) root # cryptsetup luksAddKey "${LUKS_PART}" /run/zroot.key
```

Test the **encrypted** file end to end: GnuPG decrypts it (asking for the card PIN or the passphrase), and cryptsetup
checks the result against the container without opening it:

```
(chroot) root # gpg --decrypt /etc/zfsbootmenu/keys/zroot.key.gpg \
  | cryptsetup open --test-passphrase --verbose --key-file=- "${LUKS_PART}"
```

The output ends with `Key slot 1 unlocked.` and `Command successful.`. Only then destroy the plain key:

```
(chroot) root # shred -u /run/zroot.key
(chroot) root # chmod 0600 /etc/zfsbootmenu/keys/zroot.key.gpg
```

**Check:**

```
(chroot) root # gpg --batch --list-only --list-packets /etc/zfsbootmenu/keys/zroot.key.gpg | grep 'enc packet'
(chroot) root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^\s+[0-9]+: luks2'
```

The first command prints `:pubkey enc packet:` (option A) or `:symkey enc packet:` (option B). If it prints nothing,
the file is **not** encrypted: do not continue. The second command lists two key slots, `0` and `1`.

> **Warning**
> Never copy `/run/zroot.key` anywhere else, and never put a plain key file into `/etc/zfsbootmenu`. Everything listed
> in `install_items` is copied unencrypted onto the ESP.

## Configuring the initramfs

Gentoo's initramfs unlocks the container from the `luks.*` kernel parameters set in
[Creating the ZFS pool](#creating-the-zfs-pool). It needs the key file those parameters name.

Create the initramfs key file and add it to a third key slot (cryptsetup asks for the fallback passphrase):

```
(chroot) root # install -d -m 0700 /etc/cryptsetup-keys.d
(chroot) root # install -m 0400 /dev/null /etc/cryptsetup-keys.d/zroot-initrd.key
(chroot) root # dd if=/dev/urandom of=/etc/cryptsetup-keys.d/zroot-initrd.key bs=64 count=1
(chroot) root # cryptsetup luksAddKey "${LUKS_PART}" /etc/cryptsetup-keys.d/zroot-initrd.key
```

Configure dracut. The stage3 has no `/etc/dracut.conf.d` directory yet, so create it first:

```
(chroot) root # mkdir -p /etc/dracut.conf.d
```

**FILE** `/etc/dracut.conf.d/10-zfs-luks.conf`

```
# Only include what this computer needs
hostonly="yes"
# Do not store detected rd.luks.* options; the luks.* kernel parameters are used instead
hostonly_cmdline="no"
nofsck="yes"
# ZFS root, and systemd-cryptsetup for the luks.* kernel parameters
add_dracutmodules+=" zfs crypt "
# The key file named by luks.key=
install_items+=" /etc/cryptsetup-keys.d/zroot-initrd.key "
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
(chroot) root # lsinitrd "${INITRD}" | grep -E 'zfs\.ko|zroot-initrd\.key|etc/hostid|zpool\.cache|systemd-cryptsetup$'
(chroot) root # lsinitrd -f etc/crypttab "${INITRD}"
(chroot) root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^\s+[0-9]+: luks2'
```

- The initramfs is readable only by root (`-rw-------`). If not, run `chmod 600 /boot/initramfs-*`.
- The `grep` lists all five names.
- `etc/crypttab` prints nothing.
- Three key slots are listed: `0`, `1` and `2`.

## Configuring the system

Enable systemd's presets and the ZFS services:

```
(chroot) root # systemd-machine-id-setup
(chroot) root # systemctl preset-all --preset-mode=enable-only
(chroot) root # systemctl enable zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target zfs-zed.service
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
(chroot) root # cat /etc/fstab
```

All four services are `enabled`, and fstab contains one `vfat` line.

## Installing ZFSBootMenu

This repository's build is required: upstream ZFSBootMenu cannot unlock LUKS volumes or decrypt GnuPG key files.

```
(chroot) root # ZBM_BRANCH=claude/modest-volta-qv7ugi
(chroot) root # git clone --branch "${ZBM_BRANCH}" \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2.git \
  /usr/local/src/zfsbootmenu
(chroot) root # make -C /usr/local/src/zfsbootmenu core dracut
```

> **Note**
> `ZBM_BRANCH` names the branch that contains the LUKS and GnuPG support. Use `master` once that work has been merged.
> If one of the Perl modules could not be installed with emerge, run
> `emerge --ask dev-perl/App-cpanminus && cpanm --installdeps /usr/local/src/zfsbootmenu`.

Write the generate-zbm configuration. `Kernel.CommandLine` carries the two parameters that replace the ZFSBootMenu
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
  CommandLine: "quiet loglevel=0 zbm.luks=UUID=${LUKS_UUID} zbm.luks_key=/etc/zfsbootmenu/keys/zroot.key.gpg"
EOF
```

| Parameter | Effect |
|---|---|
| `zbm.luks=UUID=<UUID>` | unlock only this container, as `luks-<UUID>` |
| `zbm.luks_key=<path>` | key file for it; files ending in `.gpg` are decrypted with GnuPG first |

If the key file cannot be decrypted, ZFSBootMenu falls back to asking for the LUKS passphrase.

Build the image twice; the second run keeps the first image as a backup:

```
(chroot) root # generate-zbm
(chroot) root # generate-zbm
```

**Check** that the image contains the encrypted key file, GnuPG, and (option A) the public key:

```
(chroot) root # ls -l /boot/efi/EFI/zbm/
(chroot) root # objcopy -O binary --only-section=.initrd /boot/efi/EFI/zbm/vmlinuz.EFI /run/zbm-initrd.img
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'zroot\.key\.gpg|bin/gpg$|scdaemon|zfsbootmenu/gnupg/pubring'
(chroot) root # lsinitrd /run/zbm-initrd.img | grep -E 'zroot-initrd\.key|etc/zfsbootmenu/crypttab'
(chroot) root # rm /run/zbm-initrd.img
```

- `vmlinuz.EFI` and `vmlinuz-backup.EFI` exist.
- The first `grep` lists `zroot.key.gpg`, `gpg` and `scdaemon`, and for option A also the public keyring.
- The second `grep` prints **nothing**: the plain initramfs key and a crypttab must never be in this image.

Create the firmware boot entries:

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

## Finalizing the installation

Switch DNS to systemd-resolved (this breaks name resolution inside the chroot, so it comes last), stop GnuPG and leave
the chroot:

```
(chroot) root # ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
(chroot) root # gpgconf --kill all
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

**Option A (smart card):**

1. ZFSBootMenu asks for the OpenPGP card. Insert it.
2. ZFSBootMenu checks that the card holds the key the file is encrypted to, then asks for the **card PIN** and shows
   how many attempts remain.
3. Touch the card if it is configured to require touch for decryption.

**Option B (passphrase):** ZFSBootMenu asks for the **GnuPG passphrase**.

Then:

1. ZFSBootMenu lists `zroot/ROOT/gentoo`. Press **Enter**, or wait for the countdown.
2. Gentoo starts **without** asking for a passphrase, and shows the login prompt.

**Check** after logging in as root:

```
root # cat /proc/cmdline
root # zpool status
root # findmnt /
root # cryptsetup status "luks-$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)" | grep flags
root # systemctl --failed
```

The command line contains `luks.crypttab=no`, `luks.uuid=` and `luks.key=`. The pool is `ONLINE`, `/` is
`zroot/ROOT/gentoo`, the container shows `flags: discards`, and no units have failed.

### Booting without the card or GnuPG passphrase

ZFSBootMenu falls back to the **LUKS fallback passphrase** when the key file cannot be used:

- **No card:** ZFSBootMenu shows *Insert the OpenPGP security key* and waits (10 seconds by default, set with
  `zbm.luks_token_wait=`). Press any key to stop waiting.
- **Wrong PIN or GnuPG passphrase:** it asks again, up to three times. Leave the prompt **empty** and press **Enter**
  to switch to the LUKS passphrase straight away. This saves card PIN attempts.

Gentoo still starts without a second prompt, because its initramfs uses its own key file.

### Updating the system

```
root # emerge --sync
root # emerge --ask --verbose --update --deep --newuse @world
root # emerge --ask --depclean
```

New kernels get a rebuilt ZFS module and a new initramfs, including the initramfs key file, automatically. Rebuild the
ZFSBootMenu image with `generate-zbm` after:

- `sys-fs/zfs`, `sys-fs/cryptsetup` or `app-crypt/gnupg` is updated;
- the key file, `/etc/zfsbootmenu/config.yaml` or `/etc/zfsbootmenu/dracut.conf.d/` changes.

Before large updates, take a snapshot: `zfs snapshot zroot/ROOT/gentoo@pre-update-$(date +%Y%m%d)`. To roll back,
select the boot environment in ZFSBootMenu and press **Alt+S** to list its snapshots.

### Adding a backup card

Decrypt the key file to RAM, re-encrypt it to both cards, test, and rebuild the image (option A):

```
root # export GPG_TTY="$(tty)"
root # gpg --import /PATH/TO/backup-pubkey.asc
root # install -m 0600 /dev/null /run/zroot.key
root # gpg --decrypt --output - /etc/zfsbootmenu/keys/zroot.key.gpg > /run/zroot.key
root # gpg --trust-model always --encrypt --recipient PRIMARY_FPR --recipient BACKUP_FPR \
  --output /etc/zfsbootmenu/keys/zroot.key.gpg.new /run/zroot.key
root # shred -u /run/zroot.key
root # mv /etc/zfsbootmenu/keys/zroot.key.gpg.new /etc/zfsbootmenu/keys/zroot.key.gpg
```

Add `BACKUP_FPR` to `zfsbootmenu_luks_gpg_keys` in `/etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf` (fingerprints
separated by spaces), then run `generate-zbm`.

### Replacing the key file

Do this if a card is lost, or to rotate the key regularly. It creates a new random key, removes the old key slot, and
needs only the fallback passphrase:

```
root # export GPG_TTY="$(tty)"
root # LUKS_PART=/dev/disk/by-partlabel/zroot-luks
root # cryptsetup luksDump "${LUKS_PART}" | grep -E '^\s+[0-9]+: luks2'
root # cryptsetup luksKillSlot "${LUKS_PART}" 1
root # install -m 0600 /dev/null /run/zroot.key
root # dd if=/dev/urandom of=/run/zroot.key bs=64 count=1
root # cryptsetup luksAddKey "${LUKS_PART}" /run/zroot.key
root # gpg --trust-model always --encrypt --recipient FINGERPRINT \
  --output /etc/zfsbootmenu/keys/zroot.key.gpg.new /run/zroot.key
root # gpg --decrypt /etc/zfsbootmenu/keys/zroot.key.gpg.new \
  | cryptsetup open --test-passphrase --verbose --key-file=- "${LUKS_PART}"
root # shred -u /run/zroot.key
root # mv /etc/zfsbootmenu/keys/zroot.key.gpg.new /etc/zfsbootmenu/keys/zroot.key.gpg
root # generate-zbm
```

Slot `1` is the GnuPG key slot if the slots were created in the order of this guide. Slot `0` (passphrase) and slot
`2` (initramfs key) must **not** be removed. If unsure which slot belongs to which key, test a key with
`cryptsetup open --test-passphrase --verbose`: the output names the slot it unlocked. For option B, replace the
`--encrypt` command with the `--symmetric` command from [Option B](#option-b-gnupg-passphrase).

### Changing the ZFSBootMenu command line

[zbm-kcl(8)](https://docs.zfsbootmenu.org/en/latest/man/zbm-kcl.8.html) shows and changes the command line embedded
in the image, for example to wait longer for the smart card:

```
root # zbm-kcl /boot/efi/EFI/zbm/vmlinuz.EFI
root # zbm-kcl -a zbm.luks_token_wait=30 /boot/efi/EFI/zbm/vmlinuz.EFI
```

It also shows the command line passed to Gentoo: `zbm-kcl zroot/ROOT/gentoo`.

Changes made this way are lost the next time `generate-zbm` runs; make permanent changes in `config.yaml`.

### Backing up the LUKS header

A damaged LUKS header makes the data unrecoverable, whatever key is used. Store a header backup **away from this
disk**, together with a copy of `zroot.key.gpg`:

```
root # cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks --header-backup-file /PATH/ON/USB/zroot-luks-header.img
root # cp /etc/zfsbootmenu/keys/zroot.key.gpg /PATH/ON/USB/
```

### Relaxing the pool compatibility

Once no older live system needs to import the pool, newer ZFS features can be enabled. Rebuild ZFSBootMenu first, so
that it can still read the pool:

```
root # generate-zbm
root # zpool set compatibility=off zroot
root # zpool upgrade zroot
```

## Troubleshooting

### ZFSBootMenu asks for the LUKS passphrase instead of the card or GnuPG passphrase

The key file was not found or could not be decrypted. Unlock with the fallback passphrase, then open the recovery shell
from the menu with **Alt+R** and check:

```
root # cat /proc/cmdline
root # ls -l /etc/zfsbootmenu/keys/
root # gpg --card-status
root # zlogtail
```

- `/proc/cmdline` must contain `zbm.luks=UUID=...` and `zbm.luks_key=/etc/zfsbootmenu/keys/zroot.key.gpg`. If not,
  fix `Kernel.CommandLine` in `/etc/zfsbootmenu/config.yaml` and run `generate-zbm`.
- If `zroot.key.gpg` is missing, check `/etc/zfsbootmenu/dracut.conf.d/luks-gpg.conf` and run `generate-zbm`.
- If `gpg --card-status` does not find the card, try another USB port, and check that GnuPG was built with
  `USE="smartcard usb"`. For card readers that need pcscd, add `zfsbootmenu_luks_pcscd=yes` to `luks-gpg.conf`,
  install `sys-apps/pcsc-lite` and `app-crypt/ccid`, and run `generate-zbm`.
- `zlogtail` shows each unlock attempt. Add `loglevel=7` to the ZFSBootMenu command line for more detail.

### GnuPG reports "No secret key"

The card does not hold the key the file is encrypted to, or the public key is missing from the image. Check the
recipients of the file against `gpg --card-status`:

```
root # gpg --batch --list-only --list-packets /etc/zfsbootmenu/keys/zroot.key.gpg | grep keyid
```

Make sure `zfsbootmenu_luks_gpg_keys` names the right fingerprint, then run `generate-zbm`.

### Gentoo asks for a passphrase after ZFSBootMenu

Gentoo's initramfs could not use its key file. On the running system:

```
root # cat /proc/cmdline
root # lsinitrd /boot/initramfs-$(uname -r).img | grep -E 'zroot-initrd\.key|systemd-cryptsetup$'
```

- The command line must contain `luks.uuid=` and `luks.key=` with the right UUID. Check the property with
  `zfs get org.zfsbootmenu:commandline zroot/ROOT`.
- If the key file is missing from the initramfs, check `/etc/dracut.conf.d/10-zfs-luks.conf`, then run
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
`/etc/portage/package.use/zfs-luks-gpg` and run `emerge --ask --update --deep --newuse @world`. The supported range
is in the ebuild:

```
root # grep -h MODULES_KERNEL_MAX /var/db/repos/gentoo/sys-fs/zfs-kmod/*.ebuild
```

### Repairing the system from the live medium

```
root # export DISK=/dev/disk/by-id/YOUR-DISK
root # export LUKS_PART="${DISK}-part2"
root # export LUKS_UUID="$(cryptsetup luksUUID "${LUKS_PART}")"
root # cryptsetup open "${LUKS_PART}" "luks-${LUKS_UUID}"
root # zpool import -f -N -R /mnt/gentoo zroot
root # zfs mount zroot/ROOT/gentoo
root # zfs mount -a
root # mount "${DISK}-part1" /mnt/gentoo/boot/efi
```

`cryptsetup open` asks for the fallback passphrase. Then chroot as in [Chrooting](#chrooting), and finish as in
[Finalizing the installation](#finalizing-the-installation).

## See also

- [Handbook:AMD64](https://wiki.gentoo.org/wiki/Handbook:AMD64) — the Gentoo installation handbook
- [ZFS](https://wiki.gentoo.org/wiki/ZFS) — ZFS on Gentoo
- [ZFS/rootfs](https://wiki.gentoo.org/wiki/ZFS/rootfs) — booting Gentoo from a ZFS root
- [Dm-crypt](https://wiki.gentoo.org/wiki/Dm-crypt) — disk encryption with dm-crypt and LUKS
- [Full Disk Encryption From Scratch](https://wiki.gentoo.org/wiki/Full_Disk_Encryption_From_Scratch)
- [GnuPG](https://wiki.gentoo.org/wiki/GnuPG) — creating and managing OpenPGP keys
- [YubiKey](https://wiki.gentoo.org/wiki/YubiKey) — using YubiKeys on Gentoo
- [Dracut](https://wiki.gentoo.org/wiki/Dracut) — the initramfs generator
- [Systemd](https://wiki.gentoo.org/wiki/Systemd)

## External resources

- This repository's documentation: `docs/general/luks.rst` (how unlocking works), `docs/luks/administration.rst` (key
  management) and the manual pages `zfsbootmenu-luks(7)` and `zluks(8)`
- [ZFSBootMenu documentation](https://docs.zfsbootmenu.org/)
- [systemd-cryptsetup-generator(8)](https://www.freedesktop.org/software/systemd/man/latest/systemd-cryptsetup-generator.html)
  — the `luks.*` kernel parameters
- [cryptsetup FAQ](https://gitlab.com/cryptsetup/cryptsetup/-/wikis/FrequentlyAskedQuestions)
- [Nitrokey documentation](https://docs.nitrokey.com/)
- [YubiKey and OpenPGP](https://support.yubico.com/hc/en-us/articles/360013790259-Using-Your-YubiKey-with-OpenPGP)
