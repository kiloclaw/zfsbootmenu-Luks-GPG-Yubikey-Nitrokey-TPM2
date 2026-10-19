# Gentoo Linux on a LUKS-Encrypted ZFS Root (systemd)

This guide installs Gentoo Linux with the **systemd** profile on **one disk**. The disk holds a ZFS root pool inside a
LUKS2 encrypted partition, and the system boots with this project's ZFSBootMenu build, which can unlock LUKS volumes.

When you are done, the computer boots like this:

1. The firmware starts **ZFSBootMenu** from the EFI system partition.
2. ZFSBootMenu asks for your **LUKS passphrase** once, unlocks the disk and imports the pool.
3. You pick a boot environment (or wait for the countdown), and ZFSBootMenu starts Gentoo.
4. Gentoo's initramfs unlocks the disk again with a key file stored *inside* the encrypted pool, so you are **not
   asked a second time**.

## Contents

- [What you get](#what-you-get)
- [Before you start](#before-you-start)
- [Part 1: Boot the live system](#part-1-boot-the-live-system)
- [Part 2: Set variables](#part-2-set-variables)
- [Part 3: Partition the disk](#part-3-partition-the-disk)
- [Part 4: Encrypt the ZFS partition](#part-4-encrypt-the-zfs-partition)
- [Part 5: Create the ZFS pool and datasets](#part-5-create-the-zfs-pool-and-datasets)
- [Part 6: Install the Gentoo stage3](#part-6-install-the-gentoo-stage3)
- [Part 7: Prepare files from the live system](#part-7-prepare-files-from-the-live-system)
- [Part 8: Enter the new system](#part-8-enter-the-new-system)
- [Part 9: Configure Portage and the base system](#part-9-configure-portage-and-the-base-system)
- [Part 10: Install the kernel, ZFS and tools](#part-10-install-the-kernel-zfs-and-tools)
- [Part 11: Build Gentoo's initramfs](#part-11-build-gentoos-initramfs)
- [Part 12: Configure systemd, network and users](#part-12-configure-systemd-network-and-users)
- [Part 13: Install and configure ZFSBootMenu](#part-13-install-and-configure-zfsbootmenu)
- [Part 14: Finish and reboot](#part-14-finish-and-reboot)
- [Part 15: First boot](#part-15-first-boot)
- [Keeping the system healthy](#keeping-the-system-healthy)
- [Troubleshooting](#troubleshooting)
- [Next steps](#next-steps)

## What you get

| Part of the disk | Size | Contents |
|---|---|---|
| Partition 1 | 1 GiB | EFI system partition (FAT32), mounted at `/boot/efi`, holds the ZFSBootMenu image |
| Partition 2 | rest of the disk | LUKS2 volume `luks-<UUID>`, containing the ZFS pool `zroot` |

| ZFS dataset | Mount point | Purpose |
|---|---|---|
| `zroot/ROOT` | none | container for boot environments |
| `zroot/ROOT/gentoo` | `/` | the Gentoo system, including `/boot` (kernels and initramfs) |
| `zroot/home` | `/home` | user data, kept separate from the system |

Because `/boot` lives inside the encrypted pool, the kernels, the initramfs and the key file inside it are protected.
Only the ZFSBootMenu image on the EFI system partition is unencrypted, and it contains no secrets.

## Before you start

You need:

- A 64-bit (amd64) computer that boots in **UEFI** mode.
- **Secure Boot turned off** in the firmware settings. The ZFSBootMenu image built in this guide is not signed.
- A wired or Wi-Fi internet connection.
- A disk you are willing to **erase completely**. Back up anything on it first.
- A few hours. Most of the time is spent downloading and compiling packages.

> **Warning:** Part 3 wipes the disk you choose. Double-check the disk name before running it.

Conventions in this guide:

- Every command is run as **root**.
- Lines starting with `#` inside a code block are comments; you do not need to type them.
- Values you must change are shown in `CAPITALS`, for example `/dev/disk/by-id/YOUR-DISK`.
- Each part ends with a **Check** step. Do not continue if a check fails; see [Troubleshooting](#troubleshooting).

## Part 1: Boot the live system

You need a live system that has **ZFS** and **cryptsetup**. Gentoo's minimal install CD does not include ZFS. Either
of these works:

- **Ubuntu Desktop live ISO (24.04 or later):** ships ZFS and cryptsetup. Choose *Try Ubuntu*, open a terminal and run
  `sudo -i`.
- **Any other live system with ZFS 2.2 or later**, including a Gentoo LiveGUI image if `zfs version` works on it.

Boot the live system **in UEFI mode**, connect to the internet, and open a root shell.

**Check:**

```sh
# Must list files. If the directory does not exist, you booted in legacy BIOS mode.
ls /sys/firmware/efi

# Load ZFS and show its version. On Ubuntu, if "zfs" is not found: apt install zfsutils-linux
modprobe zfs
zfs version

# cryptsetup must be present
cryptsetup --version

# Must show the correct date and time (needed for downloads and signature checks)
date
```

## Part 2: Set variables

Find your disk's stable name:

```sh
ls -l /dev/disk/by-id/ | grep -v -- -part
```

Pick the entry for the disk you want to install to, for example `nvme-Samsung_SSD_980_1TB_S64ANS0T123456X`. Use the
`by-id` name, not `/dev/sda` or `/dev/nvme0n1`, because those can change between boots.

Set the variables (change the `DISK` line):

```sh
export DISK=/dev/disk/by-id/YOUR-DISK
export ESP_PART="${DISK}-part1"
export LUKS_PART="${DISK}-part2"
export POOL=zroot
export BE=gentoo
```

**Check:** the disk must exist and be the right size.

```sh
lsblk -o NAME,SIZE,MODEL "$DISK"
```

> If you close the terminal or reboot the live system later, run the `export` lines again before continuing.

## Part 3: Partition the disk

This **erases the disk**. It removes old file system and RAID signatures, then creates a new GPT partition table with
two partitions.

```sh
# Remove old signatures and partition tables
wipefs -a "$DISK"

# Optional, for SSDs only: discard every block (fast, also erases the disk)
# blkdiscard -f "$DISK"

# Create the partitions:
#   1: 1 GiB EFI system partition
#   2: the rest of the disk, typed as "Linux LUKS"
sfdisk "$DISK" <<'EOF'
label: gpt
size=1GiB, type=C12A7328-F81F-11D2-BA4B-00A0C93EC93B, name="EFI"
type=CA7D7CCB-63ED-4C53-861C-1742536059CC, name="zroot-luks"
EOF

# Ask the kernel and udev to pick up the new partitions
partprobe "$DISK" 2>/dev/null || true
udevadm settle

# Format the EFI system partition
mkfs.vfat -F 32 -n EFI "$ESP_PART"
```

**Check:** two partitions are listed, and the first is `vfat`.

```sh
lsblk -o NAME,SIZE,FSTYPE,PARTLABEL "$DISK"
```

## Part 4: Encrypt the ZFS partition

Create a LUKS2 volume on partition 2. You will be asked to type `YES` in capital letters and then choose a passphrase.

```sh
cryptsetup luksFormat --type luks2 "$LUKS_PART"
```

Choose a strong passphrase you can type on a plain US keyboard layout. ZFSBootMenu asks for it before any keyboard
layout is loaded.

Open the volume. Its name is `luks-<UUID>`, which is also the name ZFSBootMenu and Gentoo will use.
`--allow-discards --persistent` enables TRIM for SSDs and stores that setting in the LUKS header.

```sh
export LUKS_UUID="$(cryptsetup luksUUID "$LUKS_PART")"
cryptsetup open --allow-discards --persistent "$LUKS_PART" "luks-${LUKS_UUID}"
```

**Check:**

```sh
echo "$LUKS_UUID"                    # a UUID such as 3f1c...-...
ls -l "/dev/mapper/luks-${LUKS_UUID}" # the unlocked device exists
cryptsetup status "luks-${LUKS_UUID}" | grep -E 'type|flags'
# type:    LUKS2
# flags:   discards
```

> **Tip:** Write the passphrase down and store it somewhere safe. Without it, the data cannot be recovered.

## Part 5: Create the ZFS pool and datasets

### 5.1 Set the host ID

ZFS records which computer last used a pool. Set a fixed host ID now; the same file is copied into Gentoo later, so the
live system, ZFSBootMenu and Gentoo all agree.

```sh
zgenhostid -f 0x00bab10c
```

### 5.2 Create the pool

```sh
zpool create -f \
  -o ashift=12 \
  -o autotrim=on \
  -o compatibility=openzfs-2.2-linux \
  -O compression=lz4 \
  -O acltype=posixacl \
  -O xattr=sa \
  -O relatime=on \
  -m none \
  "$POOL" "/dev/mapper/luks-${LUKS_UUID}"
```

What the options do:

| Option | Meaning |
|---|---|
| `ashift=12` | 4 KiB sectors; right for nearly all modern disks |
| `autotrim=on` | passes TRIM to SSDs (through LUKS, thanks to `--allow-discards`) |
| `compatibility=openzfs-2.2-linux` | only enables features ZFS 2.2 understands, so live systems with ZFS 2.2 can still import the pool. You can relax this later; see [Keeping the system healthy](#keeping-the-system-healthy) |
| `compression=lz4` | fast transparent compression |
| `acltype=posixacl`, `xattr=sa` | Linux-style ACLs and extended attributes (needed by systemd-journald and others) |
| `-m none` | the pool itself is not mounted |

ZFS native encryption is **not** used here: LUKS already encrypts the whole partition.

### 5.3 Create the datasets

```sh
zfs create -o mountpoint=none "$POOL/ROOT"
zfs create -o mountpoint=/ -o canmount=noauto "$POOL/ROOT/$BE"
zfs create -o mountpoint=/home "$POOL/home"

# Tell ZFSBootMenu which boot environment to start by default
zpool set bootfs="$POOL/ROOT/$BE" "$POOL"

# Kernel command line for Gentoo; ZFSBootMenu adds "root=zfs:..." itself
zfs set org.zfsbootmenu:commandline="quiet" "$POOL/ROOT"
```

`canmount=noauto` on the root dataset is important: it stops ZFS from mounting it automatically on top of another
system's `/`.

### 5.4 Re-import the pool under /mnt/gentoo

```sh
zpool export "$POOL"
mkdir -p /mnt/gentoo
zpool import -N -R /mnt/gentoo "$POOL"
zfs mount "$POOL/ROOT/$BE"
zfs mount -a

# Remember the pool in a cache file, so Gentoo imports it quickly at boot
zpool set cachefile=/etc/zfs/zpool.cache "$POOL"

udevadm trigger
```

**Check:**

```sh
zpool status "$POOL"         # state: ONLINE, one device: luks-<UUID>
findmnt -R /mnt/gentoo       # zroot/ROOT/gentoo on /mnt/gentoo, zroot/home on /mnt/gentoo/home
```

## Part 6: Install the Gentoo stage3

A *stage3* is a minimal Gentoo system that you build on. Use the **systemd** stage3.

### 6.1 Download it

```sh
cd /mnt/gentoo

BASE=https://distfiles.gentoo.org/releases/amd64/autobuilds
LATEST="$(wget -qO- "$BASE/latest-stage3-amd64-systemd.txt" \
  | awk '/^[0-9]+T[0-9]+Z\/stage3-amd64-systemd-.*\.tar\.xz/ { print $1 }')"
echo "$LATEST"

wget "$BASE/$LATEST" "$BASE/$LATEST.asc"
```

If `echo` prints nothing, open <https://distfiles.gentoo.org/releases/amd64/autobuilds/current-stage3-amd64-systemd/>
in a browser, and download the newest `stage3-amd64-systemd-*.tar.xz` and its `.asc` file into `/mnt/gentoo`.

### 6.2 Verify it

```sh
wget -O - https://qa-reports.gentoo.org/output/service-keys.gpg | gpg --import
gpg --verify stage3-amd64-systemd-*.tar.xz.asc
```

**Check:** the output says `Good signature from "Gentoo Linux Release Engineering (Automated Weekly Release Key) <releng@gentoo.org>"`.
A warning that the key is "not certified with a trusted signature" is normal.

### 6.3 Unpack it

```sh
tar xpf stage3-amd64-systemd-*.tar.xz --xattrs-include='*.*' --numeric-owner -C /mnt/gentoo
rm stage3-amd64-systemd-*.tar.xz*
```

### 6.4 Mount the EFI system partition

```sh
mkdir -p /mnt/gentoo/boot/efi
mount "$ESP_PART" /mnt/gentoo/boot/efi
```

**Check:**

```sh
ls /mnt/gentoo                 # bin boot dev etc home lib ... usr var
findmnt /mnt/gentoo/boot/efi   # vfat
```

## Part 7: Prepare files from the live system

These steps copy ZFS settings into the new system and create the key file that lets Gentoo unlock the disk without a
second passphrase prompt.

### 7.1 Host ID and pool cache

```sh
cp /etc/hostid /mnt/gentoo/etc/hostid
mkdir -p /mnt/gentoo/etc/zfs
cp /etc/zfs/zpool.cache /mnt/gentoo/etc/zfs/zpool.cache
```

### 7.2 Key file for Gentoo's initramfs

ZFSBootMenu closes the LUKS volume before it starts Gentoo, which wipes the volume key from memory. Gentoo's initramfs
must therefore unlock the volume again. A random key file, stored inside the encrypted pool and copied into Gentoo's
initramfs, does that without asking you.

This is safe because the key file and the initramfs both live on the encrypted pool: nobody can read them without
first unlocking the disk with your passphrase.

```sh
install -d -m 0700 /mnt/gentoo/etc/cryptsetup-keys.d
dd if=/dev/urandom of=/mnt/gentoo/etc/cryptsetup-keys.d/zroot.key bs=64 count=1
chmod 0400 /mnt/gentoo/etc/cryptsetup-keys.d/zroot.key

# Add the key file as a second way to unlock the volume (asks for your passphrase)
cryptsetup luksAddKey "$LUKS_PART" /mnt/gentoo/etc/cryptsetup-keys.d/zroot.key
```

### 7.3 Save the variables for the chroot

Variables do not carry over into the chroot, so save the ones you need:

```sh
cat > /mnt/gentoo/root/install.env <<EOF
export DISK="$DISK"
export ESP_PART="$ESP_PART"
export LUKS_PART="$LUKS_PART"
export LUKS_UUID="$LUKS_UUID"
export POOL="$POOL"
export BE="$BE"
EOF
```

**Check:**

```sh
cryptsetup luksDump "$LUKS_PART" | grep -E '^\s+[0-9]+: luks2'   # two key slots: 0 and 1
cat /mnt/gentoo/root/install.env
```

## Part 8: Enter the new system

Mount the kernel file systems and *chroot* into Gentoo:

```sh
cp --dereference /etc/resolv.conf /mnt/gentoo/etc/

mount --types proc /proc /mnt/gentoo/proc
mount --rbind /sys /mnt/gentoo/sys && mount --make-rslave /mnt/gentoo/sys
mount --rbind /dev /mnt/gentoo/dev && mount --make-rslave /mnt/gentoo/dev
mount --bind /run /mnt/gentoo/run && mount --make-slave /mnt/gentoo/run

chroot /mnt/gentoo /bin/bash
```

Inside the chroot:

```sh
source /etc/profile
source /root/install.env
export PS1="(chroot) ${PS1}"
```

> **Rule for the rest of the guide:** run `zpool` and `zfs` commands that *change* the pool from the **live system**,
> not from the chroot. The ZFS tools inside Gentoo may be a different version from the ZFS kernel module that the live
> system loaded.

**Check:**

```sh
ping -c 2 gentoo.org        # network works inside the chroot
echo "$LUKS_UUID"           # variables are loaded
```

## Part 9: Configure Portage and the base system

### 9.1 Get the package tree

```sh
emerge-webrsync
```

### 9.2 Confirm the systemd profile

```sh
eselect profile show
```

**Check:** the profile ends in `/systemd`, for example `default/linux/amd64/23.0/systemd`. If not, run
`eselect profile list` and select the matching `systemd` profile with `eselect profile set <number>`.

### 9.3 Edit make.conf

Add these lines to `/etc/portage/make.conf`:

```sh
cat >> /etc/portage/make.conf <<EOF

# Build with all CPU cores
MAKEOPTS="-j$(nproc) -l$(nproc)"

# Distribution kernels; out-of-tree modules such as ZFS are rebuilt automatically
USE="dist-kernel"

# Allow free software and redistributable firmware
ACCEPT_LICENSE="-* @FREE @BINARY-REDISTRIBUTABLE"
EOF
```

Optional, to save hours of compiling: use Gentoo's official binary packages.

```sh
echo 'FEATURES="${FEATURES} getbinpkg binpkg-request-signature"' >> /etc/portage/make.conf
getuto
```

### 9.4 Set package USE flags

```sh
mkdir -p /etc/portage/package.use
cat > /etc/portage/package.use/zfs-luks <<'EOF'
# ZFS: install the dracut module and root-on-ZFS helpers
sys-fs/zfs dracut rootfs
# Never pull in a kernel newer than ZFS supports
sys-fs/zfs-kmod dist-kernel-cap
# Build an initramfs with dracut whenever a kernel is installed
sys-kernel/installkernel dracut
# systemd-cryptsetup unlocks the disk in Gentoo's initramfs
# boot: the UEFI stub (linuxx64.efi.stub) that generate-zbm needs to build the ZFSBootMenu EFI image
sys-apps/systemd cryptsetup boot
EOF
```

### 9.5 Time zone, locale, keyboard and host name

Change the time zone, locale and keyboard to yours:

```sh
# Time zone (list: ls /usr/share/zoneinfo)
ln -sf ../usr/share/zoneinfo/Europe/London /etc/localtime

# Locale
echo "en_US.UTF-8 UTF-8" >> /etc/locale.gen
locale-gen
eselect locale set en_US.utf8

# Console keyboard layout (list: ls /usr/share/keymaps/i386/*/)
echo "KEYMAP=us" > /etc/vconsole.conf

# Host name
echo "gentoo" > /etc/hostname

env-update && source /etc/profile && export PS1="(chroot) ${PS1}"
```

### 9.6 Rebuild for the new USE flags

```sh
emerge --ask --verbose --update --deep --newuse @world
```

**Check:** the command ends without errors. Run `emerge --info | grep ^USE` and make sure `dist-kernel` is listed.

## Part 10: Install the kernel, ZFS and tools

```sh
emerge --ask \
  sys-kernel/linux-firmware \
  sys-kernel/gentoo-kernel-bin \
  sys-fs/zfs \
  sys-fs/cryptsetup \
  sys-fs/dosfstools \
  sys-boot/efibootmgr \
  dev-vcs/git \
  app-shells/fzf \
  sys-apps/kexec-tools \
  sys-block/mbuffer \
  dev-perl/Sort-Versions \
  dev-perl/YAML-PP \
  dev-perl/boolean
```

What these are for:

| Package | Why |
|---|---|
| `linux-firmware` | firmware for Wi-Fi, GPUs and other hardware |
| `gentoo-kernel-bin` | a ready-built Gentoo distribution kernel |
| `zfs` (pulls in `zfs-kmod`) | ZFS tools and kernel module |
| `cryptsetup` | LUKS tools, needed by Gentoo and by the ZFSBootMenu image |
| `dosfstools`, `efibootmgr` | manage the EFI system partition and firmware boot entries |
| `git` | download ZFSBootMenu |
| `fzf`, `kexec-tools`, `mbuffer`, Perl modules | needed to build and run ZFSBootMenu |

You can use `sys-kernel/gentoo-kernel` instead of `gentoo-kernel-bin` to compile the kernel yourself.

If Portage says a package is *masked by ~amd64 keyword*, allow it and run the command again:

```sh
echo "CATEGORY/PACKAGE ~amd64" >> /etc/portage/package.accept_keywords/zfsbootmenu
```

**Check:**

```sh
ls /lib/modules/                                    # one directory, e.g. 6.12.40-gentoo-dist
find /lib/modules -name 'zfs.ko*'                   # the ZFS module was built for that kernel
ls /usr/lib/systemd/boot/efi/linuxx64.efi.stub      # UEFI stub for the ZFSBootMenu EFI image
ls /boot                                            # kernel-<version> and initramfs-<version>.img
```

If `zfs.ko` is missing, the kernel is newer than ZFS supports; see [Troubleshooting](#troubleshooting).

The initramfs created in this step does **not** unlock the disk yet. Part 11 fixes that.

## Part 11: Build Gentoo's initramfs

### 11.1 Tell systemd how to unlock the disk

`/etc/crypttab` lists encrypted volumes. Gentoo's initramfs uses it to open the disk with the key file:

```sh
echo "luks-${LUKS_UUID}  UUID=${LUKS_UUID}  /etc/cryptsetup-keys.d/zroot.key  luks,discard" >> /etc/crypttab
cat /etc/crypttab
```

### 11.2 Configure dracut

```sh
cat > /etc/dracut.conf.d/10-zfs-luks.conf <<'EOF'
# Only include what this computer needs
hostonly="yes"
nofsck="yes"
# ZFS root and LUKS unlocking
add_dracutmodules+=" zfs crypt "
# The crypttab and the key file it names
install_items+=" /etc/crypttab /etc/cryptsetup-keys.d/zroot.key "
EOF
```

### 11.3 Keep kernels in /boot

ZFSBootMenu looks for kernels in the boot environment's `/boot`. This file makes sure Gentoo's kernel installer puts
them there, named `kernel-<version>` (older installkernel versions use `vmlinuz-<version>`) and
`initramfs-<version>.img`:

```sh
mkdir -p /etc/kernel
cat > /etc/kernel/install.conf <<'EOF'
layout=compat
initrd_generator=dracut
EOF
```

### 11.4 Rebuild the initramfs

```sh
emerge --config sys-kernel/gentoo-kernel-bin
```

(Use `sys-kernel/gentoo-kernel` here if you installed that instead.)

**Check:** the initramfs contains ZFS, the crypttab, the key file and the host ID, and is readable only by root.

```sh
INITRD="$(ls -t /boot/initramfs-*.img | head -n 1)"
ls -l "$INITRD"                                     # -rw------- (if not: chmod 600 /boot/initramfs-*)
lsinitrd "$INITRD" | grep -E 'zfs\.ko|zroot\.key|etc/crypttab|etc/hostid|zpool\.cache'
lsinitrd -f etc/crypttab "$INITRD"                  # shows your luks-<UUID> line
```

All five names must appear in the `grep` output.

## Part 12: Configure systemd, network and users

### 12.1 systemd basics

```sh
systemd-machine-id-setup
systemctl preset-all --preset-mode=enable-only
```

### 12.2 ZFS services

```sh
systemctl enable zfs-import-cache.service zfs-mount.service zfs-import.target zfs.target zfs-zed.service
```

### 12.3 Network (systemd-networkd)

This configures every wired interface with DHCP. For Wi-Fi, install `net-wireless/iwd` or
`net-misc/networkmanager` instead.

```sh
cat > /etc/systemd/network/20-wired.network <<'EOF'
[Match]
Name=en* eth*

[Network]
DHCP=yes
EOF

systemctl enable systemd-networkd.service systemd-resolved.service
```

### 12.4 fstab

Only the EFI system partition goes into `/etc/fstab`. ZFS mounts its own datasets.

```sh
echo "UUID=$(blkid -s UUID -o value "$ESP_PART")  /boot/efi  vfat  umask=0077,nofail  0 2" >> /etc/fstab
cat /etc/fstab
```

### 12.5 Root password and a user

```sh
passwd

useradd -m -G wheel,users -s /bin/bash YOURNAME
passwd YOURNAME

# Optional: let members of "wheel" use sudo
emerge --ask app-admin/sudo
echo '%wheel ALL=(ALL:ALL) ALL' > /etc/sudoers.d/wheel
chmod 0440 /etc/sudoers.d/wheel
```

### 12.6 Optional: compressed swap in RAM

Swap on ZFS is not recommended. Use zram instead:

```sh
emerge --ask sys-apps/zram-generator
printf '[zram0]\nzram-size = ram / 2\n' > /etc/systemd/zram-generator.conf
```

**Check:**

```sh
systemctl is-enabled zfs-import-cache.service zfs-mount.service zfs.target systemd-networkd.service
# enabled (four times)
```

## Part 13: Install and configure ZFSBootMenu

This guide uses **this repository's ZFSBootMenu build**, which adds LUKS unlocking. Upstream ZFSBootMenu releases
cannot open LUKS volumes.

### 13.1 Download and install

```sh
# Branch that contains the LUKS support. Use "master" once the LUKS work has been merged there.
ZBM_BRANCH=claude/modest-volta-qv7ugi

git clone --branch "$ZBM_BRANCH" \
  https://github.com/kiloclaw/zfsbootmenu-Luks-GPG-Yubikey-Nitrokey-TPM2.git \
  /usr/local/src/zfsbootmenu

cd /usr/local/src/zfsbootmenu
make core dracut
cd /
```

If Portage could not install one of the Perl modules in Part 10, install them from CPAN instead:

```sh
emerge --ask dev-perl/App-cpanminus
cpanm --installdeps /usr/local/src/zfsbootmenu
```

### 13.2 Configure generate-zbm

`make core` installs a default `/etc/zfsbootmenu/config.yaml`. Replace it:

```sh
cat > /etc/zfsbootmenu/config.yaml <<'EOF'
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
  CommandLine: quiet loglevel=0
EOF
```

ZFSBootMenu uses its own dracut settings in `/etc/zfsbootmenu/dracut.conf.d`, separate from Gentoo's
`/etc/dracut.conf.d`.

### 13.3 Tell ZFSBootMenu which volume to unlock

`/etc/zfsbootmenu/crypttab` is embedded into the ZFSBootMenu image. The key field is `none`, so ZFSBootMenu asks
for your passphrase. Never point it at `zroot.key`: that file must stay inside the encrypted pool.

```sh
echo "luks-${LUKS_UUID}  UUID=${LUKS_UUID}  none  discard" > /etc/zfsbootmenu/crypttab
chmod 0600 /etc/zfsbootmenu/crypttab
cat /etc/zfsbootmenu/crypttab
```

The format is described in `man 5 zfsbootmenu-crypttab`.

### 13.4 Build the image

Run `generate-zbm` twice. The second run keeps the first image as a backup copy.

```sh
generate-zbm
generate-zbm
```

**Check:**

```sh
ls -l /boot/efi/EFI/zbm/
# vmlinuz.EFI
# vmlinuz-backup.EFI
```

### 13.5 Add firmware boot entries

```sh
efibootmgr -c -d "$DISK" -p 1 -L "ZFSBootMenu (Backup)" -l '\EFI\ZBM\VMLINUZ-BACKUP.EFI'
efibootmgr -c -d "$DISK" -p 1 -L "ZFSBootMenu" -l '\EFI\ZBM\VMLINUZ.EFI'
```

The entry created last is tried first.

**Check:**

```sh
efibootmgr -v | grep -i zfsbootmenu    # two entries pointing at \EFI\ZBM\...
```

Some firmware ignores boot entries. As a fallback, also copy the image to the default path that every UEFI firmware
looks for:

```sh
mkdir -p /boot/efi/EFI/BOOT
cp /boot/efi/EFI/zbm/vmlinuz.EFI /boot/efi/EFI/BOOT/BOOTX64.EFI
```

## Part 14: Finish and reboot

### 14.1 Switch DNS to systemd-resolved

Do this last, because it breaks name resolution inside the chroot:

```sh
ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf
```

### 14.2 Leave the chroot and unmount

```sh
exit
```

Back on the live system:

```sh
# A snapshot of the fresh install. ZFSBootMenu can roll back to it if something goes wrong.
zfs snapshot "$POOL/ROOT/$BE@install"

umount /mnt/gentoo/boot/efi
umount -R -l /mnt/gentoo/dev /mnt/gentoo/sys /mnt/gentoo/proc /mnt/gentoo/run

zfs umount -a
zpool export "$POOL"
cryptsetup close "luks-${LUKS_UUID}"
```

**Check:**

```sh
zpool list            # "no pools available"
ls /dev/mapper        # only "control"
```

If `zpool export` reports that the pool is busy, close any terminal that is still inside `/mnt/gentoo`, then repeat
the `umount` and `zpool export` lines. A pool that was not exported cleanly still boots, because the host IDs match.

### 14.3 Reboot

```sh
reboot
```

Remove the live USB stick when the screen goes dark.

## Part 15: First boot

1. ZFSBootMenu starts and shows `Enter passphrase for luks-<UUID>:`. Type your LUKS passphrase.
2. The menu lists `zroot/ROOT/gentoo`. Press **Enter**, or wait for the countdown.
3. Gentoo starts. You are **not** asked for the passphrase again.
4. Log in.

**Check** once logged in:

```sh
zpool status                          # zroot ONLINE, no errors
findmnt /                             # zroot/ROOT/gentoo, type zfs
cryptsetup status "luks-$(cryptsetup luksUUID /dev/disk/by-partlabel/zroot-luks)" | grep flags   # discards
systemctl --failed                    # 0 loaded units listed
ping -c 2 gentoo.org                  # network works
```

The installation is complete.

## Keeping the system healthy

### Updating

```sh
emerge --sync
emerge --ask --verbose --update --deep --newuse @world
emerge --ask --depclean
```

- When a new kernel is installed, Portage rebuilds the ZFS module (`dist-kernel`) and a new initramfs (with your
  key file) automatically. `dist-kernel-cap` stops Portage from installing a kernel that ZFS does not support yet.
- ZFSBootMenu always offers the newest kernel in `/boot`. Older kernels stay selectable from the menu until
  `emerge --depclean` removes them.

### Rebuild ZFSBootMenu when ZFS or LUKS settings change

Run `generate-zbm` again after:

- `sys-fs/zfs` is updated (so ZFSBootMenu understands the same pool features);
- you edit `/etc/zfsbootmenu/crypttab` or `/etc/zfsbootmenu/config.yaml`;
- you add a TPM2, FIDO2 or YubiKey unlock method (see [Next steps](#next-steps)).

### Take snapshots before big changes

```sh
zfs snapshot zroot/ROOT/gentoo@before-upgrade-$(date +%Y%m%d)
```

If an update breaks booting, choose the boot environment in ZFSBootMenu, press **Alt+S** to list its snapshots,
and roll back or boot a clone.

### Back up the LUKS header

If the LUKS header is damaged, the data is lost even with the right passphrase. Keep a copy **off this disk**, for
example on a USB stick you store safely:

```sh
cryptsetup luksHeaderBackup /dev/disk/by-partlabel/zroot-luks \
  --header-backup-file /PATH/ON/USB/zroot-luks-header.img
```

### Relaxing the pool's compatibility setting

The pool was created with `compatibility=openzfs-2.2-linux`. Once you no longer need to import it from older live
systems, you can allow newer features. Run `generate-zbm` **before** enabling them, so ZFSBootMenu can still read
the pool:

```sh
generate-zbm
zpool set compatibility=off zroot
zpool upgrade zroot
```

## Troubleshooting

### ZFSBootMenu does not ask for the passphrase, or shows no pools

Open the recovery shell from the menu with **Alt+R**, then:

```sh
zluks list      # LUKS volumes ZFSBootMenu can see
zluks           # unlock them now
zpool import    # pools that can be imported
```

If `zluks list` shows nothing, the disk's storage driver is missing from the image. On the installed system, check
that the crypttab line in `/etc/zfsbootmenu/crypttab` has the right UUID, then run `generate-zbm` again. See
`man 8 zluks` and `man 7 zfsbootmenu-luks`.

### You are asked for the passphrase twice

Gentoo's initramfs does not contain the key file or crypttab. On the running system:

```sh
lsinitrd /boot/initramfs-$(uname -r).img | grep -E 'zroot\.key|etc/crypttab'
```

If nothing is listed, check `/etc/dracut.conf.d/10-zfs-luks.conf` (Part 11.2) and rebuild:

```sh
emerge --config sys-kernel/gentoo-kernel-bin
```

### Gentoo stops in an emergency shell: "cannot import zroot" or "pool was previously in use from another system"

The host ID or pool cache in the initramfs does not match. From the emergency shell:

```sh
zpool import -f -N zroot
exit
```

Then, once booted, make sure `/etc/hostid` exists (`hostid` prints `00bab10c`), and rebuild the initramfs as above.

### zfs.ko is missing after installing a kernel

The kernel is newer than the ZFS version in Portage supports. Make sure `/etc/portage/package.use/zfs-luks` contains
`sys-fs/zfs-kmod dist-kernel-cap`, then let Portage pick a supported kernel:

```sh
emerge --ask --update --deep --newuse @world
```

To see which kernel series ZFS supports:

```sh
grep -h MODULES_KERNEL_MAX /var/db/repos/gentoo/sys-fs/zfs-kmod/*.ebuild
```

### The computer skips ZFSBootMenu and goes to the firmware setup

- Check that Secure Boot is off.
- Check the boot order with `efibootmgr -v` from a live system, or choose *ZFSBootMenu* in the firmware's boot menu.
- Use the fallback copy at `\EFI\BOOT\BOOTX64.EFI` (Part 13.5).

### Recovering with the live system

Boot the live system again and run:

```sh
export DISK=/dev/disk/by-id/YOUR-DISK
export LUKS_PART="${DISK}-part2"
export LUKS_UUID="$(cryptsetup luksUUID "$LUKS_PART")"

cryptsetup open "$LUKS_PART" "luks-${LUKS_UUID}"
zpool import -f -N -R /mnt/gentoo zroot
zfs mount zroot/ROOT/gentoo
zfs mount -a
mount "${DISK}-part1" /mnt/gentoo/boot/efi
```

Then follow [Part 8](#part-8-enter-the-new-system) to chroot, fix the problem, and [Part 14](#part-14-finish-and-reboot)
to unmount cleanly.

## Next steps

This ZFSBootMenu build can also unlock the disk with hardware keys. The documentation in this repository explains how
to set them up:

| Goal | Where to read |
|---|---|
| Unlock with a TPM2 chip (with or without a PIN) | `docs/luks/administration.rst`, "Key Management" |
| Unlock with a FIDO2 key (YubiKey 5, Nitrokey 3) | `docs/luks/administration.rst` |
| Unlock with an OpenPGP card or YubiKey challenge-response | `docs/luks/administration.rst` |
| Every command, option and file | `docs/luks/tools.rst`, `man 7 zfsbootmenu-luks` |
| Adding more disks (mirror, RAIDZ) later | `docs/luks/vdev-configurations.rst` |

On Gentoo, the extra tools come from these packages. Install the ones you need, then run `generate-zbm`:

| Feature | Packages |
|---|---|
| TPM2 | `app-crypt/tpm2-tools`; for systemd's plugin, add `tpm` to `sys-apps/systemd`'s USE flags |
| FIDO2 | `dev-libs/libfido2`; for systemd's plugin, add `fido2` to `sys-apps/systemd`'s USE flags |
| OpenPGP card (YubiKey, Nitrokey) | `app-crypt/gnupg` (with smart card support) |
| YubiKey challenge-response | `sys-auth/ykpers` |

Enroll new unlock methods with `systemd-cryptenroll` on `/dev/disk/by-partlabel/zroot-luks`. Keep your passphrase
slot as a fallback.
